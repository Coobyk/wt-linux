# wt-backend

PTY + child-process lifecycle for wt-linux, on top of `portable-pty` 0.9.
No Rune dependency; fully headless. Deps: `anyhow`, `portable-pty`, and `libc` on Unix (`poll`, `kill`).

## API

```rust
use wt_backend::{Pty, PtySpec, PtyEvent, PtyEvents, ExitStatus, Waker};

let pty = Pty::spawn(PtySpec::new("/bin/bash").arg("-l").cwd(home).size(cols, rows))?;
let events: PtyEvents = pty.pump(Some(waker))?;   // start the read-pump (once)
let mut w = pty.writer();                         // Box<dyn Write + Send>
w.write_all(b"ls\r")?;
pty.resize(cols, rows)?;                          // child gets SIGWINCH
pty.child_exited();                               // Option<ExitStatus>, never blocks
```

The contract from `docs/ARCHITECTURE.md` is implemented as written
(`spawn`, `reader`, `writer`, `resize`, `child_exited`, `PtySpec` with its six pub fields).
Extras: `PtySpec::{new, arg, args, cwd, env, size}` builders and `Default`
(80x24, empty program = `$SHELL` else `/bin/sh`), `Pty::{pump, size, pid, kill, wait}`.

- `Pty` is `Send + Sync`; every method takes `&self`.
- `ExitStatus` is `portable_pty::ExitStatus` re-exported: `success()`, `exit_code()`, `signal()`.
- The child's env is ours plus `TERM=xterm-256color`, `COLORTERM=truecolor`,
  `TERM_PROGRAM=wt-linux`; anything in `PtySpec::env` overrides those.
- `PtySpec::cwd = None` inherits our working directory (portable-pty alone would use
  `$HOME`); a `cwd` that isn't a directory makes `spawn` fail.
- `writer()` handles never block: each `write` queues its bytes, in order, to a
  `wt-pty-write` thread, so a big paste into a program that stopped reading can't freeze the
  UI. One `write_all` is one message, so pastes from different handles don't interleave.
  After the `Pty` is dropped (or the pty write fails), writes return `BrokenPipe`. `flush()`
  is a no-op and doesn't confirm delivery (waiting would be the blocking this avoids).
  Dropping the `Pty` types nothing into the child (portable-pty's own writer sends `\n` + EOF
  on drop, which would run a half-typed shell command), and a write stalled on a full tty
  queue is abandoned within 100 ms, so the master fd is released.
- On Unix the master is `O_NONBLOCK`; `reader()` still blocks like a normal `Read`.
- `kill()` sends `SIGHUP`, then `SIGKILL` 2 s later if the child is still running; it
  returns at once. Signals are only sent while the child is unreaped, so they can't hit a
  recycled pid. Dropping the `Pty` does the same, like closing a Windows Terminal pane,
  and also stops the read-pump even if a background grandchild still holds the pty.

## Consumption model (what wt-app polls)

Channel: **`std::sync::mpsc`** (no extra dependency). `Pty::pump(waker)` starts two threads:

| Thread         | Does |
| -------------- | ---- |
| `wt-pty-read`  | `poll` + `read` on a dup of the PTY master in 64 KiB chunks → `PtyEvent::Output(Vec<u8>)`; checks every 100 ms whether the `Pty` was dropped |
| `wt-pty-exit`  | waits for the child's exit, lets the reader drain to EOF (while it keeps making progress, up to 3 s), then sends one `PtyEvent::Exited(status)` |

A third thread, `wt-pty-wait`, is started by `spawn` itself and reaps the child so
`child_exited()` works with or without a pump.

Event order: all `Output` the child wrote before exiting, then `Exited`. `Exited` still arrives
if a background grandchild keeps the PTY open (`sleep 99 &`); any output it writes later
comes after `Exited` until the Pty is dropped. Once both pump threads end,
`PtyEvents::is_closed()` is true (checking it never drops an event).

The waker is called from the pump threads when events are waiting, **coalesced**: one call
until the consumer next drains (`drain`, `drain_into`, `try_recv`, `recv_timeout`).
`wt_backend::Waker` is `Arc<dyn Fn() + Send + Sync>`, the same type as `rune::Waker`, so the
app passes its `AppSpec::setup` waker straight in, and its `poll` does:

```rust
fn poll(state: &mut App, ..) -> bool {
    let mut changed = false;
    for pane in &mut state.panes {
        let mut bytes = Vec::new();
        let exited = pane.events.drain_into(&mut bytes);   // never blocks
        if !bytes.is_empty() {
            pane.term.feed(&bytes);                        // wt_vt::Terminal
            changed = true;
        }
        if let Some(status) = exited {
            pane.on_exit(status);                          // close pane / show "[exited N]"
            changed = true;
        }
    }
    changed
}
```

`pump` may be called once per `Pty` (a second call errors). `reader()` is the raw blocking
stream for consumers that want their own thread; it shares the same bytes as the pump, so
use one or the other.

## Tests

`cargo test -p wt-backend`: headless, spawns `/bin/sh` and `/bin/cat`. Covers echo through
cat, concurrent and non-blocking writers, raw `reader()`, initial size and `resize`
(checked with `stty size`), exit codes, ^D EOF, kill (including a `SIGHUP`-ignoring child),
a grandchild holding the PTY (both for `Exited` and for releasing the pump on drop),
env/cwd/TERM, default shell, spawn failures, `is_closed` not losing events, waker
coalescing, drop killing the child, drop typing nothing into the shell, and drop
releasing a writer stalled on a full tty queue.

## Notes for siblings

- Unix only so far (tests use `/bin/sh`); Windows ConPTY paths in portable-pty are untested.
