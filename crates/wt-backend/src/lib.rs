//! wt-backend — PTY + child-process lifecycle for wt-linux.
//!
//! [`Pty::spawn`] starts a program on a fresh pseudo-terminal (via `portable-pty`).
//! The UI thread never blocks on the PTY: call [`Pty::pump`] once to start a reader
//! thread that forwards output into a `std::sync::mpsc` channel, then drain
//! [`PtyEvents`] from the app's `poll`. See the crate README for the full model.

mod pump;

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::{Context, Result, bail};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use portable_pty::ExitStatus;
pub use pump::{PtyEvent, PtyEvents};

/// Called from a background thread when new events are queued.
/// Same type as `rune::Waker`, so the app's setup waker can be passed straight in.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What to run and how big the terminal starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtySpec {
    /// Program to run. Empty means the user's login shell (`$SHELL`, else `/bin/sh`).
    pub program: String,
    pub args: Vec<String>,
    /// Working directory; `None` inherits ours.
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
        if let Some(cwd) = &self.cwd {
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
}

impl ExitCell {
    fn set(&self, status: ExitStatus) {
        *self.status.lock().unwrap() = Some(status);
        self.cond.notify_all();
    }

    fn get(&self) -> Option<ExitStatus> {
        self.status.lock().unwrap().clone()
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
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    exit: Arc<ExitCell>,
    pid: Option<u32>,
    pumped: AtomicBool,
}

impl Pty {
    pub fn spawn(spec: PtySpec) -> Result<Pty> {
        let pair = native_pty_system()
            .openpty(pty_size(spec.cols, spec.rows))
            .context("openpty failed")?;
        let mut child = pair
            .slave
            .spawn_command(spec.command())
            .with_context(|| format!("failed to spawn {:?}", spec.program))?;
        // Our copy of the slave must close, or the master never sees EOF when the child exits.
        drop(pair.slave);

        let pid = child.process_id();
        let killer = child.clone_killer();
        let writer = pair.master.take_writer().context("pty writer")?;

        let exit = Arc::new(ExitCell::default());
        let cell = exit.clone();
        thread::Builder::new()
            .name("wt-pty-wait".into())
            .spawn(move || {
                let status = child
                    .wait()
                    .unwrap_or_else(|_| ExitStatus::with_exit_code(1));
                cell.set(status);
            })
            .context("spawn pty waiter thread")?;

        Ok(Pty {
            master: Mutex::new(pair.master),
            writer: Arc::new(Mutex::new(writer)),
            killer: Mutex::new(killer),
            exit,
            pid,
            pumped: AtomicBool::new(false),
        })
    }

    /// A blocking reader of the child's output. Each call dups the master fd; all readers
    /// share one stream, so don't mix this with [`Pty::pump`]. Reads return `Ok(0)` (or an
    /// `EIO` error on Linux) once the child side is closed.
    pub fn reader(&self) -> Box<dyn Read + Send> {
        match self.master.lock().unwrap().try_clone_reader() {
            Ok(r) => r,
            Err(e) => Box::new(FailedReader(Some(io::Error::other(e)))),
        }
    }

    /// A handle that writes to the child's stdin (keystrokes, pastes). Handles are cheap
    /// clones of one shared writer and may be used from any thread.
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

    /// Kill the child (`SIGHUP` on Unix, as a closing terminal would).
    pub fn kill(&self) -> Result<()> {
        if self.child_exited().is_some() {
            return Ok(());
        }
        self.killer.lock().unwrap().kill().context("kill child")
    }

    /// Start the read-pump: a thread that forwards output into a channel, followed by one
    /// [`PtyEvent::Exited`]. `waker` is called (coalesced) whenever events are waiting.
    /// May be called once per `Pty`.
    pub fn pump(&self, waker: Option<Waker>) -> Result<PtyEvents> {
        if self.pumped.swap(true, Ordering::SeqCst) {
            bail!("Pty::pump called twice");
        }
        let reader = self
            .master
            .lock()
            .unwrap()
            .try_clone_reader()
            .context("pty reader")?;
        pump::start(reader, self.exit.clone(), waker)
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
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

struct PtyWriter(Arc<Mutex<Box<dyn Write + Send>>>);

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        // One lock for the whole buffer so concurrent writers don't interleave mid-paste.
        self.0.lock().unwrap().write_all(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

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
