# Architecture

wt-linux is a cargo workspace. Five crates, strict dependency direction:

```
wt-backend ──> (no wt deps)         PTY + process lifecycle
wt-vt      ──> (no wt deps)         bytes -> terminal state
wt-render  ──> wt-vt, rune          terminal state -> Rune draw calls
wt-app     ──> wt-backend, wt-vt, wt-render, rune
wt         ──> everything           binary, config, wiring
```

Crates lower in the table must not depend on crates above them.
`wt-backend` and `wt-vt` must not depend on `rune` — they stay headless and
unit-testable on CI with no GPU.

## Contracts

The owner of each crate owns its public API, but these cross-crate contracts
are fixed:

### wt-backend

```rust
pub struct Pty { /* spawn a child in a pty */ }
impl Pty {
    pub fn spawn(spec: PtySpec) -> anyhow::Result<Pty>;
    pub fn reader(&self) -> Box<dyn Read + Send>;   // stream of child output bytes
    pub fn writer(&self) -> Box<dyn Write + Send>;  // keystrokes -> child stdin
    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()>;
    pub fn child_exited(&self) -> Option<ExitStatus>;
}
pub struct PtySpec { pub program: String, pub args: Vec<String>,
                     pub cwd: Option<PathBuf>, pub env: Vec<(String,String)>,
                     pub cols: u16, pub rows: u16 }
```

Use `portable-pty`. Nonblocking reads: reader thread -> `mpsc`/`crossbeam` channel
is the expected integration; document whatever you pick.

### wt-vt

```rust
pub struct Terminal { /* grid + scrollback + attrs + modes */ }
impl Terminal {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self;
    pub fn feed(&mut self, bytes: &[u8]);           // parse VT, mutate state
    pub fn resize(&mut self, cols: u16, rows: u16);
    pub fn grid(&self) -> GridView<'_>;             // stable view for renderers
    pub fn cursor(&self) -> CursorState;
}
```

Implementing a full VT parser is huge — wrapping `alacritty_terminal`'s core
(or `wezterm-term`) is acceptable; own the public API either way.
Grid cells carry: char/cluster, fg/bg (palette idx or RGB), attrs
(bold/italic/underline/strikethrough/inverse/dim/blink), wide-char flag.

### wt-render

Owns turning `GridView` + `CursorState` + selection/scroll offset into Rune
scene nodes. Expectation: a `TerminalView` Rune component with
`fn view(&self, term: &Terminal, sel: Selection, scroll: usize)`.
Glyph atlas is Rune's — just feed it text runs with fg/bg/attrs.
Must hit a dirty-rect / damage model: don't re-shade unchanged rows.

### wt-app

Rune app shell. Owns: window, tab bar, split-pane layout tree
(`Pane | VSplit(a,b) | HSplit(a,b)`), profile model + TOML config schema,
command palette, settings screen. Keybinding model: actions enum ->
keymap table, rebindable in config.

### wt

`fn main()`: load `$XDG_CONFIG_HOME/wt-linux/config.toml`, build default
profile, open window via wt-app. Also `--version`, `--config <path>`,
`--command <cmd>` launch-time overrides.

## Coordination rules for agents

- One crate per agent, one git branch per agent (`agent/<crate>`).
- Do not edit files outside your crate, `Cargo.toml` workspace root
  (only under `[workspace.dependencies]`, additive only), or other agents'
  branches.
- If you need an API from another crate that doesn't exist yet, implement
  against the contract above and note the gap in your crate's README.
- `cargo build -p <your-crate>` and `cargo test -p <your-crate>` must pass
  on Linux headless (no display required). Rune apps run headless —
  render to PNG / dump element tree for verification.
- Push your branch when done; open a PR to `main` with `gh` if available.
