//! `real-app`: the wt-app shell over wt-backend PTYs and wt-vt terminals.
//!
//! wt's config (`config::Config`, shaped after Windows Terminal's settings.json) converts to
//! wt-app's `Config` here; each pane is a [`RealTerminal`] — a `wt_backend::Pty` read-pump
//! feeding a `wt_vt::Terminal`, snapshotted per frame into wt-app's `GridSnapshot`.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use wt_app::config::{self as ac, KeySpec};
use wt_app::scheme::ColorScheme as AppScheme;
use wt_app::terminal::{
    Attrs as AAttrs, Cell as ACell, CellColor, CursorShape, CursorState as ACursor, GridSnapshot,
    Notify, PaneModes, PaneTerminal, SpawnSpec, TerminalFactory, UiEvent,
};
use wt_backend::{Pty, PtyEvent, PtyEvents, PtySpec};
use wt_vt::{TermEvent, Terminal};
use wt_render::TermSource;

use crate::config::{self, ColorScheme, Profile, ResolvedProfile};
use crate::launch::Launch;

/// The window over real terminals. `rune_args` carry Rune tooling (`--snap`, `--tree`).
pub fn run(launch: Launch) -> ExitCode {
    let scrollback = launch.config.scrollback;
    let config = to_app_config(&launch.config, &launch.first);
    let app = wt_app::app(config, app_config_path(&launch), factory(scrollback))
        .args(launch.rune_args);
    app.run();
    ExitCode::SUCCESS
}

/// A factory spawning one `Pty` + `Terminal` per pane. `notify` wakes the UI on output.
pub fn factory(scrollback: usize) -> TerminalFactory {
    Box::new(move |spec: &SpawnSpec, notify: Notify| {
        match RealTerminal::spawn(spec, scrollback, notify) {
            Ok(t) => Box::new(t) as Box<dyn PaneTerminal>,
            Err(e) => Box::new(FailedTerminal::new(spec, &e.to_string())),
        }
    })
}

/// One pane: child process on a PTY, its output parsed by a VT terminal the shell draws
/// directly through wt-render's `TermSource` (no per-frame grid copy).
pub struct RealTerminal {
    pty: Pty,
    events: PtyEvents,
    term: Terminal,
    writer: Box<dyn Write + Send>,
    grid: GridSnapshot,
    ui_events: Vec<UiEvent>,
    exited: bool,
}

impl RealTerminal {
    fn spawn(spec: &SpawnSpec, scrollback: usize, notify: Notify) -> anyhow::Result<Self> {
        let pty = Pty::spawn(PtySpec {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            cols: spec.cols.max(1),
            rows: spec.rows.max(1),
        })?;
        let events = pty.pump(Some(notify))?;
        let writer = pty.writer();
        let mut term = Terminal::new(spec.cols.max(1), spec.rows.max(1), scrollback);
        term.set_default_cursor_shape(match spec.cursor_shape {
            CursorShape::Bar => wt_vt::CursorShape::Beam,
            CursorShape::Block => wt_vt::CursorShape::Block,
            CursorShape::Underline => wt_vt::CursorShape::Underline,
        });
        Ok(RealTerminal {
            pty,
            events,
            term,
            writer,
            grid: GridSnapshot::new(1, 1),
            ui_events: Vec::new(),
            exited: false,
        })
    }
}

impl PaneTerminal for RealTerminal {
    fn grid(&self) -> &GridSnapshot {
        &self.grid
    }

    fn cursor(&self) -> ACursor {
        let c = self.term.cursor();
        ACursor {
            col: c.col,
            row: c.row,
            visible: c.visible,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(cols, rows);
        let _ = self.pty.resize(cols, rows);
    }

    fn poll(&mut self) -> bool {
        let mut out = Vec::new();
        let mut exited = false;
        for ev in self.events.drain() {
            match ev {
                PtyEvent::Output(b) => out.extend_from_slice(&b),
                PtyEvent::Exited(_) => exited = true,
            }
        }
        self.exited |= exited;
        if !out.is_empty() {
            self.term.feed(&out);
        }
        self.term.flush_expired_sync_update();
        // Replies the VT layer owes the child (DA, DECRPM, OSC answers) go back on the PTY.
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            let _ = self.writer.write_all(&replies);
        }
        for ev in self.term.take_events() {
            match ev {
                TermEvent::ClipboardStore { text, .. } => {
                    self.ui_events.push(UiEvent::CopyText(text))
                }
                TermEvent::ClipboardLoad { format, .. } => {
                    self.ui_events.push(UiEvent::LoadClipboard { format })
                }
                TermEvent::Bell => self.ui_events.push(UiEvent::Bell),
                TermEvent::Title(_)
                | TermEvent::ResetTitle
                | TermEvent::CursorBlinkingChange
                | TermEvent::MouseCursorDirty => {}
            }
        }
        !(out.is_empty() && self.ui_events.is_empty() && !exited)
    }

    fn title(&self) -> Option<String> {
        self.term.title().map(str::to_owned)
    }

    fn exited(&self) -> bool {
        self.exited || self.pty.child_exited().is_some()
    }

    fn term_source(&self) -> Option<&dyn TermSource> {
        Some(&self.term)
    }

    fn modes(&self) -> PaneModes {
        let m = self.term.modes();
        PaneModes {
            alternate_scroll: m.alternate_scroll,
            bracketed_paste: m.bracketed_paste,
            alt_screen: m.alt_screen,
            app_cursor: m.app_cursor,
        }
    }

    fn set_focused(&mut self, focused: bool) {
        self.term.set_focused(focused);
    }

    fn take_ui_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.ui_events)
    }
}

/// A pane shown when the child fails to spawn: the error, in red, instead of a crash.
struct FailedTerminal {
    grid: GridSnapshot,
}

impl FailedTerminal {
    fn new(spec: &SpawnSpec, error: &str) -> Self {
        let mut grid = GridSnapshot::new(spec.cols.max(1), spec.rows.max(1));
        let red = CellColor::Indexed(9);
        let dim = CellColor::Indexed(8);
        let lines = [
            (format!("wt: could not start {}", spec.program), red),
            (error.to_string(), dim),
        ];
        for (r, (text, fg)) in lines.iter().enumerate() {
            for (c, ch) in text.chars().take(grid.cols as usize).enumerate() {
                grid.cells[r * grid.cols as usize + c] = ACell {
                    ch,
                    fg: *fg,
                    bg: CellColor::Default,
                    attrs: AAttrs::default(),
                    wide: false,
                };
            }
        }
        FailedTerminal { grid }
    }
}

impl PaneTerminal for FailedTerminal {
    fn grid(&self) -> &GridSnapshot {
        &self.grid
    }

    fn cursor(&self) -> ACursor {
        ACursor {
            col: 0,
            row: 0,
            visible: false,
        }
    }

    fn write(&mut self, _: &[u8]) {}

    fn resize(&mut self, cols: u16, rows: u16) {
        let mut g = GridSnapshot::new(cols.max(1), rows.max(1));
        let n = (cols as usize).min(self.grid.cols as usize);
        for r in 0..(rows.min(self.grid.rows) as usize) {
            g.cells[r * cols as usize..r * cols as usize + n]
                .copy_from_slice(&self.grid.cells[r * self.grid.cols as usize..][..n]);
        }
        self.grid = g;
    }

    fn exited(&self) -> bool {
        true
    }
}

/// wt's resolved config -> wt-app's. The window size is cells in wt's schema and points in
/// wt-app's, so it's scaled by the resolved font's cell size.
fn to_app_config(cfg: &config::Config, first: &ResolvedProfile) -> ac::Config {
    let font = ac::FontConfig {
        family: cfg
            .defaults
            .font_family
            .clone()
            .unwrap_or_else(|| "mono".into()),
        size: cfg.defaults.font_size.unwrap_or(12.0),
    };
    let (cw, lh) = wt_app::app::cell_size(font.size);
    let mut app = ac::Config {
        default_profile: Some(cfg.default_profile.clone()),
        theme: ac::Theme::Dark,
        window: ac::WindowConfig {
            width: cfg.window.cols as f32 * cw,
            height: cfg.window.rows as f32 * lh,
        },
        font: font.clone(),
        profiles: cfg
            .profiles
            .iter()
            .map(|p| to_app_profile(cfg, p, &font))
            .collect(),
        schemes: cfg.color_schemes.iter().map(to_app_scheme).collect(),
        keybindings: cfg
            .keybindings
            .iter()
            .map(|(a, c)| (a.clone(), KeySpec::One(c.clone())))
            .collect(),
    };
    // `first` is authoritative for the opening tab: --profile and --command already resolved.
    match app.profiles.iter().position(|p| p.name == first.name) {
        Some(i) => patch_profile(&mut app.profiles[i], first),
        None => {
            app.profiles.insert(0, first_profile(first));
            app.default_profile = Some(first.name.clone());
        }
    }
    if !app.schemes.iter().any(|s| s.name == first.color_scheme.name) {
        app.schemes.push(to_app_scheme(&first.color_scheme));
    }
    app
}

fn to_app_profile(cfg: &config::Config, p: &Profile, font: &ac::FontConfig) -> ac::Profile {
    let mut merged = cfg.defaults.clone();
    merged.overlay(&p.settings);
    ac::Profile {
        name: p.name.clone(),
        program: merged.command.clone().unwrap_or_default(),
        args: merged.args.clone().unwrap_or_default(),
        cwd: merged.cwd.clone(),
        env: merged.env.clone(),
        color_scheme: merged
            .color_scheme
            .clone()
            .unwrap_or_else(|| "Campbell".into()),
        font: (merged.font_family.is_some() || merged.font_size.is_some()).then(|| {
            ac::FontConfig {
                family: merged.font_family.clone().unwrap_or_else(|| font.family.clone()),
                size: merged.font_size.unwrap_or(font.size),
            }
        }),
        opacity: 1.0,
        cursor_shape: merged.cursor_shape.map(app_shape).unwrap_or_default(),
        hidden: merged.hidden.unwrap_or(false),
    }
}

fn app_shape(s: config::CursorShape) -> CursorShape {
    match s {
        config::CursorShape::Bar => CursorShape::Bar,
        config::CursorShape::Block => CursorShape::Block,
        config::CursorShape::Underline => CursorShape::Underline,
    }
}

/// The fields of a resolved profile written over a converted one.
fn patch_profile(p: &mut ac::Profile, first: &ResolvedProfile) {
    p.program = first.program.clone();
    p.args = first.args.clone();
    p.cwd = first.cwd.as_ref().map(|c| c.to_string_lossy().into_owned());
    p.env = first.env.iter().cloned().collect();
    p.color_scheme = first.color_scheme.name.clone();
    p.font = Some(ac::FontConfig {
        family: first.font_family.clone(),
        size: first.font_size,
    });
    p.cursor_shape = app_shape(first.cursor_shape);
}

fn first_profile(first: &ResolvedProfile) -> ac::Profile {
    let mut p = ac::Profile {
        name: first.name.clone(),
        opacity: 1.0,
        hidden: false,
        ..ac::Profile::default()
    };
    patch_profile(&mut p, first);
    p
}

fn to_app_scheme(s: &ColorScheme) -> AppScheme {
    let ansi = |i: usize| s.palette.get(i).cloned().unwrap_or_else(|| "#000000".into());
    AppScheme {
        name: s.name.clone(),
        foreground: s.foreground.clone(),
        background: s.background.clone(),
        cursor: Some(s.cursor.clone()),
        selection_background: s.selection.clone(),
        black: ansi(0),
        red: ansi(1),
        green: ansi(2),
        yellow: ansi(3),
        blue: ansi(4),
        purple: ansi(5),
        cyan: ansi(6),
        white: ansi(7),
        bright_black: ansi(8),
        bright_red: ansi(9),
        bright_green: ansi(10),
        bright_yellow: ansi(11),
        bright_blue: ansi(12),
        bright_purple: ansi(13),
        bright_cyan: ansi(14),
        bright_white: ansi(15),
    }
}

/// The config file wt loaded, for wt-app's settings tab to show and save back.
pub fn app_config_path(launch: &Launch) -> Option<PathBuf> {
    match &launch.source {
        config::Source::File(p) | config::Source::Created(p) => Some(p.clone()),
        config::Source::Builtin => None,
    }
}
