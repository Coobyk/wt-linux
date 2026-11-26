//! The read-pump: PTY output -> `std::sync::mpsc` channel, so the UI never blocks on a read.

use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::{ExitCell, ExitStatus, Waker};

const READ_CHUNK: usize = 64 * 1024;
/// After the child exits, how long to let its last output drain before sending `Exited`
/// (a background grandchild may hold the pty open forever).
const DRAIN_GRACE: Duration = Duration::from_millis(250);

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
    shared: Arc<Shared>,
}

struct Shared {
    waker: Option<Waker>,
    /// True while a wake is outstanding; cleared when the consumer drains.
    pending: AtomicBool,
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
    /// Next event if one is queued.
    pub fn try_recv(&self) -> Option<PtyEvent> {
        self.shared.pending.store(false, Ordering::Release);
        self.rx.try_recv().ok()
    }

    /// Everything queued right now, without blocking.
    pub fn drain(&self) -> Vec<PtyEvent> {
        self.shared.pending.store(false, Ordering::Release);
        self.rx.try_iter().collect()
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
        self.shared.pending.store(false, Ordering::Release);
        self.rx.recv_timeout(timeout).ok()
    }

    /// True once both pump threads have finished and the queue is empty: nothing more will come.
    pub fn is_closed(&self) -> bool {
        matches!(self.rx.try_recv(), Err(TryRecvError::Disconnected))
    }
}

pub(crate) fn start(
    reader: Box<dyn Read + Send>,
    exit: Arc<ExitCell>,
    waker: Option<Waker>,
) -> Result<PtyEvents> {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        waker,
        pending: AtomicBool::new(false),
        reader_done: Mutex::new(false),
        reader_cond: Condvar::new(),
    });

    let (s, t) = (shared.clone(), tx.clone());
    thread::Builder::new()
        .name("wt-pty-read".into())
        .spawn(move || read_loop(reader, t, s))
        .context("spawn pty read thread")?;

    let s = shared.clone();
    thread::Builder::new()
        .name("wt-pty-exit".into())
        .spawn(move || exit_loop(exit, tx, s))
        .context("spawn pty exit thread")?;

    Ok(PtyEvents { rx, shared })
}

fn read_loop(mut reader: Box<dyn Read + Send>, tx: Sender<PtyEvent>, shared: Arc<Shared>) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                    break;
                }
                shared.wake();
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            // Linux reports a closed slave as EIO; any error ends the stream.
            Err(_) => break,
        }
    }
    shared.finish_reader();
}

fn exit_loop(exit: Arc<ExitCell>, tx: Sender<PtyEvent>, shared: Arc<Shared>) {
    let status = exit.wait();
    let done = shared.reader_done.lock().unwrap();
    let _ = shared
        .reader_cond
        .wait_timeout_while(done, DRAIN_GRACE, |d| !*d);
    if tx.send(PtyEvent::Exited(status)).is_ok() {
        shared.wake();
    }
}
