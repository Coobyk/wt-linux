# wt-linux

**Windows Terminal, for Linux.** A GPU-accelerated terminal emulator written in
Rust, with the UI built on [Rune](https://github.com/anomalyco/rune)
(`anomalyco/rune`, pre-release — pin a rev of the `dev` branch).

Feature targets, à la Windows Terminal:

- Tabs and split panes
- Multiple profiles (bash/zsh/fish/ssh, per-profile colors, fonts, cwd)
- Command palette, searchable settings
- Scrollback, selection, clipboard integration
- GPU rendering at 120 Hz via Rune's own renderer

## Workspace layout

| Crate          | Responsibility |
| -------------- | -------------- |
| `wt-backend`   | PTY lifecycle: spawn, read/write, resize, exit notification |
| `wt-vt`        | Terminal emulation: VT sequence parsing → cell grid + scrollback |
| `wt-render`    | Draw a grid in Rune: glyph shaping, colors, cursor, selection |
| `wt-app`       | App shell in Rune: tabs, splits, palette, profiles, settings |
| `wt`           | `main()`: config load, wiring, window bootstrap |

Each crate compiles independently against the contracts in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
