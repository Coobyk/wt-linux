# wt

The `wt` executable: command line, config loading, and the wiring
`Pty -> read pump -> Terminal::feed -> view`, one tab per `Pty`.

```text
wt [--config <path>] [--profile <name>] [--rune <rune args...> --] [--command <cmd...>]
wt --version | --help | --print-config
```

## Config

`$XDG_CONFIG_HOME/wt-linux/config.toml` (or `~/.config/...`). On first run the built-in
defaults are written there (`wt --print-config` shows the merged result). Precedence:
built-in defaults < file < `--profile` / `--command`. Profiles inherit `[defaults]`; a
`[[profiles]]` entry with a built-in name merges into it, colour schemes replace by name,
`[keybindings]` merge per action (`""` unbinds).

**Mismatch with docs/ARCHITECTURE.md:** the schema is wt-app's, but wt-app has no types yet,
so `src/config/schema.rs` defines it. When wt-app publishes its schema, that module should
become a re-export.

## Features

| Feature | Effect |
|---|---|
| `ui` (default) | Rune window with a tab bar and a plain-text grid view (stand-in for wt-app) |
| `real-backend` | `wt_backend::Pty` instead of `src/stub/backend.rs` (std::process, no real pty) |
| `real-vt` | `wt_vt::Terminal` + `wt_render::TerminalView` instead of `src/stub/vt.rs` |
| `real-app` | Hand the window to `wt_app::run` (proposed entry point) |

Without `ui`, `wt` runs the first tab to completion and prints its final screen.

## Headless self-check

```sh
wt --rune --snap out.png -- --command "ls /"
wt --rune --tree tree.json -- -e echo hi
```
