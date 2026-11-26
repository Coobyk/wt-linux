//! Unix fd plumbing. The master is non-blocking, so nothing that touches it can wedge a
//! thread forever: every reader and writer polls, and the pump's ones also watch `closing`.

use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use portable_pty::MasterPty;

/// How often a blocked pump reader or writer checks whether its `Pty` was dropped.
pub(crate) const CLOSE_POLL_MS: i32 = 100;

pub(crate) enum Readiness {
    /// The `revents` poll reported.
    Ready(i16),
    Timeout,
    Failed,
}

pub(crate) fn poll(fd: RawFd, events: i16, timeout_ms: i32) -> Readiness {
    let mut pfd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    // SAFETY: one valid pollfd, owned by this frame.
    match unsafe { libc::poll(&mut pfd, 1, timeout_ms) } {
        0 => Readiness::Timeout,
        n if n > 0 => Readiness::Ready(pfd.revents),
        _ if io::Error::last_os_error().kind() == ErrorKind::Interrupted => Readiness::Timeout,
        _ => Readiness::Failed,
    }
}

fn master_fd(master: &dyn MasterPty) -> Result<RawFd> {
    master.as_raw_fd().context("pty master has no fd")
}

/// A dup of the master: same open file description, so it shares `O_NONBLOCK`.
pub(crate) fn dup_master(master: &dyn MasterPty) -> Result<File> {
    let fd = master_fd(master)?;
    // SAFETY: `fd` is owned by `master`, which is borrowed (and so open) for this call.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .context("dup pty fd")?;
    Ok(File::from(owned))
}

pub(crate) fn set_nonblocking(master: &dyn MasterPty) -> Result<()> {
    let fd = master_fd(master)?;
    // SAFETY: fcntl on an fd `master` keeps open.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            bail!("set pty non-blocking: {}", io::Error::last_os_error());
        }
    }
    Ok(())
}

/// `Pty::reader()`: blocking reads over the non-blocking master. Like portable-pty's reader,
/// `EIO` (Linux: every slave closed) is EOF.
pub(crate) struct BlockingReader(pub(crate) File);

impl Read for BlockingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0.read(buf) {
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if let Readiness::Failed = poll(self.0.as_raw_fd(), libc::POLLIN, -1) {
                        return Err(io::Error::last_os_error());
                    }
                }
                Err(e) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                r => return r,
            }
        }
    }
}

/// The writer thread's end. Unlike portable-pty's writer it sends nothing when dropped
/// (that one types `\n` + EOF, which would submit a half-typed shell command), and a write
/// stalled on a full tty queue gives up once `closing` is set.
pub(crate) struct MasterWriter {
    pub(crate) file: File,
    pub(crate) closing: Arc<AtomicBool>,
}

impl Write for MasterWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            if self.closing.load(Ordering::SeqCst) {
                return Err(ErrorKind::BrokenPipe.into());
            }
            match self.file.write(buf) {
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    match poll(self.file.as_raw_fd(), libc::POLLOUT, CLOSE_POLL_MS) {
                        // Every slave closed: a full queue stays full (writes keep saying
                        // EAGAIN), so stop instead of spinning.
                        Readiness::Ready(ev) if ev & (libc::POLLHUP | libc::POLLERR) != 0 => {
                            return Err(ErrorKind::BrokenPipe.into());
                        }
                        Readiness::Failed => return Err(io::Error::last_os_error()),
                        _ => {}
                    }
                }
                r => return r,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
