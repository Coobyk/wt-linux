//! The read-pump: PTY output -> `std::sync::mpsc` channel, so the UI never blocks on a read.

use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use portable_pty::MasterPty;

use crate::{ExitCell, ExitStatus, Waker};

const READ_CHUNK: usize = 64 * 1024;
/// After the child exits, `Exited` waits for the reader to hit EOF. While the reader keeps
/// making progress it waits in slices of this long, up to `DRAIN_MAX` in total, since a
/// background grandchild may hold the pty open forever.
const DRAIN_SLICE: Duration = Duration::from_millis(250);
const DRAIN_MAX: Duration = Duration::from_secs(3);

/// One message from the pump.
#[derive(Debug, Clone)]
pub enum PtyEvent {
    /// Raw bytes the child wrote, in order. Feed them to `wt_vt::Terminal::feed`.
    Output(Vec<u8>),
    /// The child exited. Sent once, after the output that preceded the exit.
    Exited(ExitStatus),
}

/// Receiving end of [`crate::Pty::pump`]. Poll it from the UI thread; nothing here blocks
/// except [`PtyEvents::recv_timeout`].
pub struct PtyEvents {
    rx: Receiver<PtyEvent>,
    /// An event `is_closed` had to take off the channel to look; handed out first.
    peeked: Mutex<Option<PtyEvent>>,
    shared: Arc<Shared>,
}

struct Shared {
    waker: Option<Waker>,
    /// True while a wake is outstanding; cleared when the consumer drains.
    pending: AtomicBool,
    /// Chunks read so far; lets `exit_loop` tell a slow drain from a stuck one.
    chunks: AtomicU64,
    reader_done: Mutex<bool>,
    reader_cond: Condvar,
}

impl Shared {
    fn wake(&self) {
        if let Some(w) = &self.waker
            && !self.pending.swap(true, Ordering::AcqRel)
        {
            w();
        }
    }

    fn finish_reader(&self) {
        *self.reader_done.lock().unwrap() = true;
        self.reader_cond.notify_all();
    }
}

impl PtyEvents {
    fn take_peeked(&self) -> Option<PtyEvent> {
        self.shared.pending.store(false, Ordering::Release);
        self.peeked.lock().unwrap().take()
    }

    /// Next event if one is queued.
    pub fn try_recv(&self) -> Option<PtyEvent> {
        self.take_peeked().or_else(|| self.rx.try_recv().ok())
    }

    /// Everything queued right now, without blocking.
    pub fn drain(&self) -> Vec<PtyEvent> {
        self.take_peeked()
            .into_iter()
            .chain(self.rx.try_iter())
            .collect()
    }

    /// Drain and concatenate queued output; returns the exit status if `Exited` was among them.
    pub fn drain_into(&self, out: &mut Vec<u8>) -> Option<ExitStatus> {
        let mut exited = None;
        for ev in self.drain() {
            match ev {
                PtyEvent::Output(b) => out.extend_from_slice(&b),
                PtyEvent::Exited(s) => exited = Some(s),
            }
        }
        exited
    }

    /// Block up to `timeout` for the next event (tests, non-UI consumers).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<PtyEvent> {
        self.take_peeked()
            .or_else(|| self.rx.recv_timeout(timeout).ok())
    }

    /// True once both pump threads have finished and every event has been received:
    /// nothing more will come. Never loses an event.
    pub fn is_closed(&self) -> bool {
        let mut peeked = self.peeked.lock().unwrap();
        if peeked.is_some() {
            return false;
        }
        match self.rx.try_recv() {
            Ok(ev) => {
                *peeked = Some(ev);
                false
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => true,
        }
    }
}

/// What the read thread reads from: on Unix a dup of the master fd, so it can `poll` and
/// notice its `Pty` being dropped instead of sitting in `read` forever.
#[cfg(unix)]
pub(crate) type Source = std::fs::File;
#[cfg(not(unix))]
pub(crate) type Source = Box<dyn Read + Send>;

#[cfg(unix)]
pub(crate) fn source(master: &dyn MasterPty) -> Result<Source> {
    crate::fd::dup_master(master)
}

#[cfg(not(unix))]
pub(crate) fn source(master: &dyn MasterPty) -> Result<Source> {
    master.try_clone_reader().context("pty reader")
}

pub(crate) fn start(
    source: Source,
    exit: Arc<ExitCell>,
    closing: Arc<AtomicBool>,
    waker: Option<Waker>,
) -> Result<PtyEvents> {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        waker,
        pending: AtomicBool::new(false),
        chunks: AtomicU64::new(0),
        reader_done: Mutex::new(false),
        reader_cond: Condvar::new(),
    });

    let (s, t) = (shared.clone(), tx.clone());
    thread::Builder::new()
        .name("wt-pty-read".into())
        .spawn(move || {
            read_loop(source, &closing, &t, &s);
            s.finish_reader();
        })
        .context("spawn pty read thread")?;

    let s = shared.clone();
    thread::Builder::new()
        .name("wt-pty-exit".into())
        .spawn(move || exit_loop(&exit, &tx, &s))
        .context("spawn pty exit thread")?;

    Ok(PtyEvents {
        rx,
        peeked: Mutex::new(None),
        shared,
    })
}

fn read_loop(mut src: Source, closing: &AtomicBool, tx: &Sender<PtyEvent>, shared: &Shared) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        if closing.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(unix)]
        {
            use crate::fd::{CLOSE_POLL_MS, Readiness, poll};
            use std::os::fd::AsRawFd;
            match poll(src.as_raw_fd(), libc::POLLIN, CLOSE_POLL_MS) {
                // POLLHUP/POLLERR land here too; the read reports them.
                Readiness::Ready(_) => {}
                Readiness::Timeout => continue,
                Readiness::Failed => return,
            }
        }
        match src.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                shared.chunks.fetch_add(1, Ordering::Release);
                if tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                    return;
                }
                shared.wake();
            }
            Err(e) if matches!(e.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock) => {
                continue;
            }
            // Linux reports a closed slave as EIO; any error ends the stream.
            Err(_) => return,
        }
    }
}

fn exit_loop(exit: &ExitCell, tx: &Sender<PtyEvent>, shared: &Shared) {
    let status = exit.wait();
    let start = Instant::now();
    let mut done = shared.reader_done.lock().unwrap();
    loop {
        let seen = shared.chunks.load(Ordering::Acquire);
        let (guard, _) = shared
            .reader_cond
            .wait_timeout_while(done, DRAIN_SLICE, |d| !*d)
            .unwrap();
        done = guard;
        let progressed = shared.chunks.load(Ordering::Acquire) != seen;
        if *done || !progressed || start.elapsed() >= DRAIN_MAX {
            break;
        }
    }
    drop(done);
    if tx.send(PtyEvent::Exited(status)).is_ok() {
        shared.wake();
    }
}
