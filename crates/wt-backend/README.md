# wt-backend

PTY + child-process lifecycle for wt-linux, on top of `portable-pty` 0.9.
No Rune dependency; fully headless.

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
- `writer()` handles are clones of one shared writer, usable from any thread;
  `write_all` holds the lock for the whole buffer, so pastes don't interleave.
- Dropping the `Pty` kills the child (SIGHUP), like closing a Windows Terminal pane.

## Consumption model (what wt-app polls)

Channel: **`std::sync::mpsc`** (no extra dependency). `Pty::pump(waker)` starts two threads:

| Thread         | Does |
| -------------- | ---- |
| `wt-pty-read`  | blocking `read` on the PTY master in 64 KiB chunks → `PtyEvent::Output(Vec<u8>)` |
| `wt-pty-exit`  | waits for the child's exit, lets the reader drain (up to 250 ms), then sends one `PtyEvent::Exited(status)` |

A third thread, `wt-pty-wait`, is started by `spawn` itself and reaps the child so
`child_exited()` works with or without a pump.

Event order: all `Output` the child wrote before exiting, then `Exited`. `Exited` still arrives
if a background grandchild keeps the PTY open (`sleep 99 &`); any output it writes later
comes after `Exited` until the Pty is dropped. Once both pump threads end,
`PtyEvents::is_closed()` is true.

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
cat, concurrent writers, raw `reader()`, initial size and `resize` (checked with `stty size`),
exit codes, ^D EOF, kill, grandchild holding the PTY, env/cwd/TERM, default shell, spawn
failure, waker coalescing, and drop killing the child.

## Notes for siblings

- Resolving this workspace needs Rune's `[patch.crates-io]` entries in the root `Cargo.toml`
  (`rune-text` asks for `parley_engine`'s `line-dictionaries` feature, which only Rune's
  vendored copy has; `winit`, `wgpu-hal`, `naga`, `sctk-adwaita`, `accesskit_*` are patched
  the same way). Without them `cargo build -p wt-backend` fails during resolution even
  though this crate doesn't use Rune. Until the root gets them, build with
  `--config "patch.crates-io.<crate>.git='https://github.com/anomalyco/rune'"` (plus a matching
  `rev`) for each of: `wgpu-hal naga winit sctk-adwaita parley_engine accesskit_windows
  accesskit_atspi_common accesskit_macos`.
- The root's `rune` dependency tracks `branch = "dev"`; it should pin a `rev`.
- Unix only so far (tests use `/bin/sh`); Windows ConPTY paths in portable-pty are untested.
