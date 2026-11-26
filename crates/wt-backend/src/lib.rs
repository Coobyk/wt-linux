//! wt-backend — PTY + child-process lifecycle for wt-linux.
//!
//! [`Pty::spawn`] starts a program on a fresh pseudo-terminal (via `portable-pty`).
//! The UI thread never blocks on the PTY: call [`Pty::pump`] once to start a reader
//! thread that forwards output into a `std::sync::mpsc` channel, then drain
//! [`PtyEvents`] from the app's `poll`. See the crate README for the full model.

#[cfg(unix)]
mod fd;
mod pump;

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
#[cfg(not(unix))]
use portable_pty::ChildKiller;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use portable_pty::ExitStatus;
pub use pump::{PtyEvent, PtyEvents};

/// How long a child gets to exit after `SIGHUP` before it is sent `SIGKILL`.
const KILL_GRACE: Duration = Duration::from_secs(2);

/// Called from a background thread when new events are queued.
/// Same type as `rune::Waker`, so the app's setup waker can be passed straight in.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What to run and how big the terminal starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtySpec {
    /// Program to run. Empty means the user's login shell (`$SHELL`, else `/bin/sh`).
    pub program: String,
    pub args: Vec<String>,
    /// Working directory; `None` inherits ours. A path that isn't a directory fails `spawn`.
    pub cwd: Option<PathBuf>,
    /// Extra environment on top of ours. Overrides the `TERM`/`COLORTERM` defaults.
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

impl Default for PtySpec {
    fn default() -> Self {
        Self {
            program: String::new(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
        }
    }
}

impl PtySpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }

    fn command(&self) -> CommandBuilder {
        let mut cmd = if self.program.is_empty() {
            CommandBuilder::new_default_prog()
        } else {
            let mut c = CommandBuilder::new(&self.program);
            c.args(&self.args);
            c
        };
        // portable-pty falls back to $HOME without a cwd, so pass ours explicitly.
        if let Some(cwd) = self.cwd.clone().or_else(|| std::env::current_dir().ok()) {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "wt-linux");
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

/// Exit status shared between the waiter thread and everyone asking.
#[derive(Default)]
pub(crate) struct ExitCell {
    status: Mutex<Option<ExitStatus>>,
    cond: Condvar,
    /// Set (under this lock) once the child is a zombie and before it is reaped, so a
    /// signal sent while holding it can never reach a recycled pid.
    #[cfg(unix)]
    dead: Mutex<bool>,
}

impl ExitCell {
    #[cfg(unix)]
    fn mark_dead(&self) {
        *self.dead.lock().unwrap() = true;
    }

    /// Signal the child unless it has already exited.
    #[cfg(unix)]
    fn signal(&self, pid: u32, sig: libc::c_int) -> io::Result<()> {
        let dead = self.dead.lock().unwrap();
        // SAFETY: plain kill(2); `dead` is false, so `pid` is still our unreaped child.
        if !*dead && unsafe { libc::kill(pid as libc::pid_t, sig) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn set(&self, status: ExitStatus) {
        *self.status.lock().unwrap() = Some(status);
        self.cond.notify_all();
    }

    fn get(&self) -> Option<ExitStatus> {
        self.status.lock().unwrap().clone()
    }

    fn wait_timeout(&self, timeout: Duration) -> Option<ExitStatus> {
        let guard = self.status.lock().unwrap();
        let (guard, _) = self
            .cond
            .wait_timeout_while(guard, timeout, |s| s.is_none())
            .unwrap();
        guard.clone()
    }

    pub(crate) fn wait(&self) -> ExitStatus {
        let guard = self
            .cond
            .wait_while(self.status.lock().unwrap(), |s| s.is_none())
            .unwrap();
        guard.clone().expect("exit status set")
    }
}

/// A child process running on a pseudo-terminal.
///
/// `Send + Sync`; every method takes `&self`. Dropping the `Pty` kills the child.
pub struct Pty {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Sender<WriteMsg>,
    #[cfg(not(unix))]
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    exit: Arc<ExitCell>,
    pid: Option<u32>,
    pumped: AtomicBool,
    /// Set on drop; tells the pump's reader to let go of the master even if a grandchild
    /// still holds the slave open.
    closing: Arc<AtomicBool>,
}

impl Pty {
    pub fn spawn(spec: PtySpec) -> Result<Pty> {
        if let Some(cwd) = &spec.cwd
            && !cwd.is_dir()
        {
            bail!("cwd {} is not a directory", cwd.display());
        }
        let pair = native_pty_system()
            .openpty(pty_size(spec.cols, spec.rows))
            .context("openpty failed")?;
        let closing = Arc::new(AtomicBool::new(false));
        // Everything fallible that doesn't need the child happens before it exists.
        #[cfg(unix)]
        let w: Box<dyn Write + Send> = {
            fd::set_nonblocking(&*pair.master)?;
            Box::new(fd::MasterWriter {
                file: fd::dup_master(&*pair.master)?,
                closing: closing.clone(),
            })
        };
        #[cfg(not(unix))]
        let w = pair.master.take_writer().context("pty writer")?;
        let writer = start_writer(w)?;
        let mut child = pair
            .slave
            .spawn_command(spec.command())
            .with_context(|| format!("failed to spawn {:?}", spec.program))?;
        // Our copy of the slave must close, or the master never sees EOF when the child exits.
        drop(pair.slave);

        let pid = child.process_id();
        #[cfg(not(unix))]
        let killer = child.clone_killer();
        let exit = Arc::new(ExitCell::default());
        let cell = exit.clone();
        let waiter = thread::Builder::new()
            .name("wt-pty-wait".into())
            .spawn(move || {
                #[cfg(unix)]
                if let Some(pid) = pid {
                    wait_until_dead(pid);
                }
                #[cfg(unix)]
                cell.mark_dead();
                let status = child
                    .wait()
                    .unwrap_or_else(|_| ExitStatus::with_exit_code(1));
                cell.set(status);
            });
        if let Err(e) = waiter {
            // The closure (and the child handle) are gone; reap by pid so no zombie is left.
            if let Some(pid) = pid {
                force_kill_and_reap(pid);
            }
            return Err(e).context("spawn pty waiter thread");
        }

        Ok(Pty {
            master: Mutex::new(pair.master),
            writer,
            #[cfg(not(unix))]
            killer: Mutex::new(killer),
            exit,
            pid,
            pumped: AtomicBool::new(false),
            closing,
        })
    }

    /// A blocking reader of the child's output. Each call dups the master fd; all readers
    /// share one stream, so don't mix this with [`Pty::pump`]. Reads return `Ok(0)` once every
    /// holder of the child side has closed it.
    pub fn reader(&self) -> Box<dyn Read + Send> {
        let master = self.master.lock().unwrap();
        #[cfg(unix)]
        let reader = fd::dup_master(&**master)
            .map(|f| Box::new(fd::BlockingReader(f)) as Box<dyn Read + Send>);
        #[cfg(not(unix))]
        let reader = master.try_clone_reader();
        match reader {
            Ok(r) => r,
            Err(e) => Box::new(FailedReader(Some(io::Error::other(e)))),
        }
    }

    /// A handle that writes to the child's stdin (keystrokes, pastes). Writes never block:
    /// bytes are queued, in order, to a writer thread, so a paste into a program that has
    /// stopped reading can't freeze the UI. Handles may be used from any thread. Once the
    /// child side is gone (or the `Pty` dropped), writes fail with `BrokenPipe`. `flush` is a
    /// no-op: it doesn't wait for queued bytes, since that is exactly the blocking this avoids.
    pub fn writer(&self) -> Box<dyn Write + Send> {
        Box::new(PtyWriter(self.writer.clone()))
    }

    /// Resize the terminal; the child gets `SIGWINCH`.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(pty_size(cols, rows))
            .context("pty resize")
    }

    /// `(cols, rows)` as the kernel currently has them.
    pub fn size(&self) -> Result<(u16, u16)> {
        let s = self
            .master
            .lock()
            .unwrap()
            .get_size()
            .context("pty get_size")?;
        Ok((s.cols, s.rows))
    }

    /// `Some` once the child has exited. Never blocks.
    pub fn child_exited(&self) -> Option<ExitStatus> {
        self.exit.get()
    }

    /// Block until the child exits.
    pub fn wait(&self) -> ExitStatus {
        self.exit.wait()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Kill the child: `SIGHUP` on Unix, as a closing terminal would, then `SIGKILL` if it
    /// is still running 2 s later (on a background thread; this returns at once).
    pub fn kill(&self) -> Result<()> {
        if self.child_exited().is_some() {
            return Ok(());
        }
        #[cfg(not(unix))]
        self.killer.lock().unwrap().kill().context("kill child")?;
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            self.exit.signal(pid, libc::SIGHUP).context("kill child")?;
            let exit = self.exit.clone();
            let _ = thread::Builder::new()
                .name("wt-pty-kill".into())
                .spawn(move || {
                    if exit.wait_timeout(KILL_GRACE).is_none() {
                        let _ = exit.signal(pid, libc::SIGKILL);
                    }
                });
        }
        Ok(())
    }

    /// Start the read-pump: a thread that forwards output into a channel, followed by one
    /// [`PtyEvent::Exited`]. `waker` is called (coalesced) whenever events are waiting.
    /// May be called once per `Pty`.
    pub fn pump(&self, waker: Option<Waker>) -> Result<PtyEvents> {
        if self.pumped.swap(true, Ordering::SeqCst) {
            bail!("Pty::pump called twice");
        }
        let source = pump::source(&**self.master.lock().unwrap())?;
        pump::start(source, self.exit.clone(), self.closing.clone(), waker)
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.writer.send(WriteMsg::Close);
        let _ = self.kill();
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

enum WriteMsg {
    Data(Vec<u8>),
    Close,
}

fn start_writer(mut w: Box<dyn Write + Send>) -> Result<Sender<WriteMsg>> {
    let (tx, rx) = mpsc::channel::<WriteMsg>();
    thread::Builder::new()
        .name("wt-pty-write".into())
        .spawn(move || {
            while let Ok(WriteMsg::Data(bytes)) = rx.recv() {
                if w.write_all(&bytes).and_then(|_| w.flush()).is_err() {
                    break;
                }
            }
        })
        .context("spawn pty writer thread")?;
    Ok(tx)
}

struct PtyWriter(Sender<WriteMsg>);

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // One message per call, so a `write_all` paste is never interleaved with other handles.
        self.0
            .send(WriteMsg::Data(buf.to_vec()))
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Block until `pid` has exited, without reaping it (the pid stays reserved).
#[cfg(unix)]
fn wait_until_dead(pid: u32) {
    loop {
        // SAFETY: waitid with a zeroed out-param it fills in.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if r == 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return;
        }
    }
}

#[cfg(unix)]
fn force_kill_and_reap(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
        libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), 0);
    }
}

#[cfg(not(unix))]
fn force_kill_and_reap(_pid: u32) {}

struct FailedReader(Option<io::Error>);

impl Read for FailedReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        match self.0.take() {
            Some(e) => Err(e),
            None => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests;
