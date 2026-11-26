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

## Run

```
cargo run -p wt --features real-app
```

The `real-app` feature wires wt-app's shell over wt-backend PTYs and
wt-vt terminals. Without it, `wt` runs the in-crate stub UI; with neither
`ui` nor `real-app` it runs headless and prints the session's final screen.

Config lives at `$XDG_CONFIG_HOME/wt-linux/config.toml`
(`wt-app/CONFIG.md` documents the schema; `wt --print-config` shows the
effective config). `--profile NAME` and `--command CMD` override the first
tab. `--rune <args> --` forwards to Rune's tooling:
`wt --rune --snap shot.png --` renders a frame to a PNG without a display.
