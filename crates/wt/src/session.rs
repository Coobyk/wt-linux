//! One tab = one [`Session`]: a `Pty`, a read pump thread, and the `Terminal` it feeds.
//!
//! ```text
//! Pty::reader() --(pump thread, mpsc)--> Session::drain() --> Terminal::feed() --> view
//! ```
//!
//! The pump never touches the terminal: it only ships bytes and calls `notify` (Rune's
//! `Waker`) once per batch, so the UI thread drains on its next `poll` and `Terminal` stays
//! single-threaded.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};

use crate::config::ResolvedProfile;
use crate::contract::{ExitStatus, Pty, PtySpec, Terminal};

pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Bytes fed per `drain`, so a flood (`cat bigfile`) can't stall a frame.
pub const DRAIN_BUDGET: usize = 1 << 20;

/// Chunks (up to 64 KiB each) queued before the pump blocks, applying backpressure to the child.
const QUEUE_CHUNKS: usize = 64;

pub struct Session {
    pub title: String,
    pub profile: ResolvedProfile,
    pub term: Terminal,
    pty: Pty,
    /// Input goes through a writer thread so a child that isn't reading never blocks the UI.
    input_tx: mpsc::Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    pending: Arc<AtomicBool>,
    eof: bool,
    exit: Option<ExitStatus>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Drained {
    /// The terminal (or the session's liveness) changed: redraw.
    pub changed: bool,
    /// The budget ran out with bytes still queued: drain again next frame.
    pub more: bool,
}

impl Session {
    pub fn spawn(
        profile: ResolvedProfile,
        cols: u16,
        rows: u16,
        scrollback: usize,
        notify: Notify,
    ) -> anyhow::Result<Session> {
        let pty = Pty::spawn(PtySpec {
            program: profile.program.clone(),
            args: profile.args.clone(),
            cwd: profile.cwd.clone(),
            env: profile.env.clone(),
            cols,
            rows,
        })?;
        let (tx, rx) = mpsc::sync_channel(QUEUE_CHUNKS);
        let pending = Arc::new(AtomicBool::new(false));
        let reader = pty.reader();
        let flag = pending.clone();
        std::thread::Builder::new()
            .name(format!("wt-pump-{}", profile.name))
            .spawn(move || pump(reader, tx, flag, notify))?;
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let mut writer = pty.writer();
        std::thread::Builder::new()
            .name(format!("wt-input-{}", profile.name))
            .spawn(move || {
                for bytes in input_rx {
                    if writer
                        .write_all(&bytes)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Session {
            title: profile.name.clone(),
            term: Terminal::new(cols, rows, scrollback),
            input_tx,
            profile,
            pty,
            rx,
            pending,
            eof: false,
            exit: None,
        })
    }

    /// Feed queued output into the terminal, up to `budget` bytes.
    pub fn drain(&mut self, budget: usize) -> Drained {
        self.pending.store(false, Ordering::Release);
        let mut out = Drained::default();
        let mut fed = 0;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => {
                    self.term.feed(&bytes);
                    fed += bytes.len();
                    out.changed = true;
                    if fed >= budget {
                        out.more = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    out.changed |= !self.eof;
                    self.eof = true;
                    break;
                }
            }
        }
        if self.eof && self.exit.is_none() {
            self.exit = self.pty.child_exited().map(ExitStatus::from);
            out.changed |= self.exit.is_some();
        }
        out
    }

    /// Output has ended (the child closed the pty). The tab stays open showing its last screen.
    pub fn is_finished(&self) -> bool {
        self.eof
    }

    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit
    }

    /// Another handle on the child's input, for forwarding a stream (headless stdin).
    pub fn input(&self) -> Box<dyn Write + Send> {
        self.pty.writer()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if !self.eof && self.input_tx.send(bytes.to_vec()).is_err() {
            self.eof = true;
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(cols, rows);
        let _ = self.pty.resize(cols, rows);
    }
}

fn pump(
    mut reader: Box<dyn Read + Send>,
    tx: SyncSender<Vec<u8>>,
    pending: Arc<AtomicBool>,
    notify: Notify,
) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    return;
                }
                if !pending.swap(true, Ordering::AcqRel) {
                    notify();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // EIO is how Linux ptys report "child gone".
            Err(_) => break,
        }
    }
    drop(tx);
    notify();
}

/// The window's tabs, one per `Pty`.
#[derive(Default)]
pub struct Tabs {
    pub sessions: Vec<Session>,
    pub active: usize,
}

impl Tabs {
    pub fn push(&mut self, s: Session) {
        self.sessions.push(s);
        self.active = self.sessions.len() - 1;
    }

    pub fn active(&self) -> Option<&Session> {
        self.sessions.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active)
    }

    pub fn close(&mut self, i: usize) {
        if i < self.sessions.len() {
            self.sessions.remove(i);
            if self.active >= self.sessions.len() || self.active > i {
                self.active = self.active.saturating_sub(1);
            }
        }
    }

    pub fn select(&mut self, i: usize) {
        if i < self.sessions.len() {
            self.active = i;
        }
    }

    pub fn cycle(&mut self, forward: bool) {
        let n = self.sessions.len();
        if n > 0 {
            self.active = if forward {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
        }
    }

    pub fn drain_all(&mut self) -> Drained {
        let mut out = Drained::default();
        for s in &mut self.sessions {
            let d = s.drain(DRAIN_BUDGET);
            out.changed |= d.changed;
            out.more |= d.more;
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
