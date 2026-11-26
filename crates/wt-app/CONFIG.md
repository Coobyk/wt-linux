# wt-linux config schema

Path: `$XDG_CONFIG_HOME/wt-linux/config.toml`, falling back to
`$HOME/.config/wt-linux/config.toml`. A missing file means all defaults. Unknown keys are
errors (typos don't silently do nothing). See [`config.sample.toml`](config.sample.toml).

## Top level

| Key | Type | Default | Meaning |
|---|---|---|---|
| `default_profile` | string | first profile | Profile name new tabs use |
| `theme` | `"dark"` \| `"light"` | `"dark"` | Window chrome theme |
| `window` | table | | `width`, `height` in points (1100 × 680) |
| `font` | table | | Default `family` (`"mono"`) and `size` (13) for profiles without `font` |
| `profiles` | array of tables | one `Shell` profile | See below |
| `schemes` | array of tables | | Extra colour schemes, see below |
| `keybindings` | table | | `action = "chord"` or `action = ["chord", ...]` |

## `[[profiles]]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | `"Shell"` | Shown in tab titles and the new-tab menu |
| `program` | string | `$SHELL`, then `/bin/sh` | Executable to spawn |
| `args` | array of strings | `[]` | Arguments |
| `cwd` | string | inherit | Starting directory; leading `~` expands to `$HOME` |
| `env` | table of strings | `{}` | Extra environment variables |
| `color_scheme` | string | `"Campbell"` | Scheme name; unknown names fall back to Campbell |
| `font` | `{ family, size }` | top-level `font` | Per-profile font |
| `opacity` | float 0–1 | `1.0` | Background opacity (clamped) |
| `hidden` | bool | `false` | Leave out of the new-tab menu and palette |

## `[[schemes]]`

Windows Terminal's scheme fields in `snake_case`, each `#rrggbb`: `name`, `foreground`,
`background`, optional `cursor` and `selection_background`, and the 16 ANSI colours
`black red green yellow blue purple cyan white` plus `bright_` variants of each. A user scheme
with a built-in's name replaces it. Built-ins: `Campbell`, `One Half Dark`, `One Half Light`.

## `[keybindings]`

Keys are action names; values are [Rune `Chord`](https://github.com/anomalyco/rune) strings:
modifiers `ctrl`, `alt`, `shift`, `super`, `mod` (Ctrl on Linux), joined with `+` to a key —
a character (`t`, `=`, `-`, `,`), or a name (`tab`, `enter`, `escape`, `left`, `f1`, `plus`,
`comma`, `minus`, …). Configuring an action replaces all its default keys; a chord claimed by a
configured action is taken from whichever action had it. `""` or `[]` unbinds.

| Action | Default |
|---|---|
| `new_tab` | `ctrl+shift+t` |
| `new_tab_profile:N` (1-based) | `ctrl+shift+N` for N = 1–3 |
| `close_tab` | `ctrl+shift+f4` |
| `rename_tab` | `ctrl+shift+r` |
| `next_tab` / `prev_tab` | `ctrl+tab` / `ctrl+shift+tab` |
| `switch_to_tab:N` (1-based) | `ctrl+alt+N` for N = 1–5 |
| `split_vertical` (new pane right) | `alt+shift+=` |
| `split_horizontal` (new pane below) | `alt+shift+-` |
| `close_pane` | `ctrl+shift+w` |
| `focus_next_pane` / `focus_prev_pane` | `alt+right` / `alt+left` |
| `open_palette` | `ctrl+shift+p` |
| `open_settings` | `ctrl+comma` |
| `font_size_increase` / `font_size_decrease` / `font_size_reset` | `ctrl+=` / `ctrl+-` / `ctrl+0` |
| `quit` | unbound |
