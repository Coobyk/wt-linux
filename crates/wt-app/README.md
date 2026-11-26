# wt-app

The Rune app shell for wt-linux: the window, tab bar, split-pane layout tree, command palette,
profiles and TOML config ([CONFIG.md](CONFIG.md)), settings tab, and a rebindable keymap.
Until the sibling crates land, panes run `FakeTerminal`s behind the `PaneTerminal` trait.

Headless: `cargo run -p wt-app --example demo -- --scene split --snap split.png`
(scenes: `tabs`, `split`, `palette`, `settings`; `--tree` dumps the element tree).

## Missing cross-crate APIs

- **wt-vt**: export the cell types behind `GridView` (character, fg/bg colour as default /
  indexed / RGB, bold/italic/underline/inverse/dim attributes, wide flag). `wt_app::terminal`
  mirrors them as `Cell`, `CellColor`, `Attrs`, `CursorState` until then. Also a window title
  set by OSC 0/2, for tab titles.
- **wt-render**: `TerminalView` should expose the measured cell size, or a call mapping a pixel
  box to columns × rows, so panes fit exactly. `app::cell_size` estimates it from the font size
  (0.6 em advance), and `grid_view` stands in for `TerminalView`.
- **wt-backend**: `Pty` needs non-blocking output delivery (a channel or a wake callback) so the
  UI thread never blocks on `reader()`. The shell drains terminals in `PaneTerminal::poll`.
- **wt**: build a `TerminalFactory` over wt-backend + wt-vt and call
  `wt_app::app(config, path, factory).run()`.
