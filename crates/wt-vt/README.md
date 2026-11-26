# wt-vt

Terminal emulation for wt-linux: VT bytes in, cell grid + scrollback out. Headless, no
`rune` dependency.

Parsing and grid storage are [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal)
0.26 (vte 0.15), wrapped behind our own API — no alacritty types are public.

## API (contract from `docs/ARCHITECTURE.md`, plus extras)

```rust
let mut term = Terminal::new(cols, rows, scrollback_lines);
term.feed(&bytes_from_pty);            // any chunking; partial sequences are buffered
pty.write_all(&term.take_replies())?;  // DSR/CPR, DA, DECRQM, OSC color queries, focus
for ev in term.take_events() { /* Title, Bell, ClipboardStore/Load, ... */ }
term.resize(cols, rows);               // also resize the PTY

let grid: GridView = term.grid();      // rows(), cols(), history_len(), cell(r, c),
                                       // line(i) (i < 0 = scrollback), viewport(scroll),
                                       // text_range(a, b), screen_text(), hyperlink(p)
let cur: CursorState = term.cursor();  // row, col, shape, visible, blinking, wide
let m: Modes = term.modes();           // app_cursor, bracketed_paste, mouse, alt_screen, ...
let dmg: Damage = term.take_damage();  // Full | Rows([{row, left, right}])
let pal: Palette = term.palette();     // base palette + OSC 4/10/11/12 overrides
```

`Cell { ch, combining, fg, bg, underline_color, attrs, width }`:

- `fg`/`bg`: `Color::Default | Color::Indexed(0..=255) | Color::Rgb(Rgb)`.
  Resolve with `Palette::resolve(color, ColorRole::Foreground/Background)`.
- `attrs`: `Attrs` bitset — `BOLD DIM ITALIC UNDERLINE (+DOUBLE/UNDERCURL/DOTTED/DASHED)
  BLINK INVERSE HIDDEN STRIKETHROUGH`.
- `width`: `Narrow | Wide | WideSpacer | LeadingSpacer`. Draw a `Wide` glyph across two
  cells; draw nothing for spacers.

Presentation (inverse swap, dim, bold-as-bright, hidden, blink timing) is left to the renderer.

## Line coordinates

`0..rows` is the live screen (0 = top). Negative lines are scrollback: `-1` is the newest
history line, `-history_len()` the oldest. A renderer scrolled up by `scroll` lines draws
`grid.viewport(scroll)` (or `grid.line(grid.viewport_line(scroll, row))`). Scrolling is
renderer state; the `Terminal` never moves its own viewport.

## Resize policy: reflow

Primary screen: column changes **reflow** soft-wrapped lines (hard newlines kept); shrinking
rows pushes top lines into scrollback, growing pulls them back. The alt screen is truncated /
padded, not reflowed. Tested in `resize_*` tests.

## Damage

`take_damage()` returns rows changed since the last call (live-screen coordinates) and resets
tracking. `Full` after creation, resize, alt-screen switch, full clears, etc. Cursor moves
damage the old and new cursor cells. When the user's scroll offset changes, the renderer
must redraw fully itself.

## Testing

```
cargo test -p wt-vt
printf 'a\033[31mB\033[0m\n' | cargo run -p wt-vt --example vtdump -- 40 5
```

Tests in `tests/vt.rs` feed real escape sequences (SGR, cursor movement, erase, scroll
regions, DECAWM, alt screen, scrollback, wide/combining chars, OSC, DSR, resize) and assert
grid contents. Plus a random-bytes no-panic test.

## Notes / gaps for sibling crates

- **Workspace currently fails to resolve** (any `cargo build -p …`), not because of this
  crate: Rune's `dev` branch vendors patched crates (`parley_engine` with the
  `line-dictionaries` feature, `winit`, `wgpu-hal`, `naga`, `sctk-adwaita`, `accesskit_*`)
  and says "apps that build Rune patch it too". The root `Cargo.toml` needs a
  `[patch.crates-io]` section pointing those at Rune's `vendor/` dirs (same git rev). Until
  then, build with a local `--config` file containing that patch section.
- Synchronized updates (`CSI ?2026h`) are buffered by the parser. wt-app should call
  `flush_expired_sync_update()` when `sync_update_deadline()` passes (no new bytes may
  arrive), and `flush_sync_update()` when the child exits.
- Kitty keyboard protocol is not supported: `CSI > u` etc. are ignored and
  `Modes::kitty_keyboard` is always 0. Alacritty 0.26's implementation is buggy (panics on
  stack overflow, `CSI = u` not reported), so enabling it needs our own stack in the shim.
- Grid width is clamped to at least 2 columns so a wide character always fits.
- `src/shim.rs` also works around alacritty bugs: DCH past the row end, relative cursor
  moves in origin mode, color-query reply ordering, and RIS not resetting the title.
- Blink: alacritty drops SGR 5/6/25; `src/shim.rs` intercepts SGR to carry it in a spare
  cell-flag bit. It delegates every other `vte::ansi::Handler` method to alacritty's `Term`;
  re-check the method list when bumping vte.
- `HT` cells read as `' '` in `Cell::ch` (alacritty stores `'\t'`); `text_range` still
  copies tabs.
- SGR 21 is "cancel bold" (vte behaviour), not double underline; use `4:2`.
- No vi mode, search, or selection state here — selection is the renderer/app's concern;
  use `GridView::text_range` to copy.
- Keyboard/mouse input encoding is not in this crate; `Modes` exposes everything needed
  (`app_cursor`, `app_keypad`, `bracketed_paste`, `mouse`, `sgr_mouse`, `kitty_keyboard`).
