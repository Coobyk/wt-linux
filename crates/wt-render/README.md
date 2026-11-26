# wt-render

`TerminalView`: draws a terminal grid as one Rune `grid` element (see `docs/ARCHITECTURE.md`).

```rust
let view = TerminalView::new(Palette::default(), 14.0);
view.view(&term, selection, scroll) // -> rune::El
view.grid_size_for((w, h))          // -> (cols, rows) that fit, for Terminal/Pty resize
view.last_damage().rebuilt          // visible rows rebuilt by the last `view`
```

Rows hash their cells, selection columns, cursor and the view's config. An unchanged row hands Rune
the same `Rc<GridRow>` as last frame, so Rune re-emits that row's cached instances instead of laying
out and shading it again.

## Gaps against the contract

- **`wt-vt::GridView` doesn't exist yet.** `view` is generic over `TermSource` (`size`, `history`,
  `line`, `cursor`), and this crate has its own `Cell`/`VtColor`/`Attrs`/`Width`/`CursorState`.
  Once `wt-vt` lands, add `impl TermSource for wt_vt::Terminal` here on top of `grid()`/`cursor()`.
  The contract's `view(&self, term: &Terminal, sel, scroll)` then works unchanged.
- **Line numbering:** line 0 is the top screen row and -1 is the newest scrollback line.
  `Selection` points use the same numbering.
- **Not drawn yet:** blink, curly/double/coloured underlines, hyperlinks.
- **Before `wt-vt` lands:** `wt-app` can build against `wt_render::mock::MockTerminal`
  (`features = ["mock"]`).

## Tests

`cargo test -p wt-render` renders a fake grid offscreen through Rune's GPU path. On a host with no
GPU it uses the Mesa lavapipe driver; set `RUNE_ALLOW_NO_GPU=1` to skip the GPU tests when no
Vulkan driver exists.

The tests write PNGs, the element tree and the a11y JSON to `target/wt-render-snapshots/`.
`WT_UPDATE_SNAPSHOTS=1` refreshes the committed `snapshots/`.
