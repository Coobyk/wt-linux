//! The window: a tab bar and the active tab's terminal, one tab per `Pty`.
//!
//! This is the stand-in for wt-app's shell until it lands (`real-app`). The terminal body is
//! wt-render's `TerminalView` under `real-vt`; without it, a plain-text grid of the stub
//! terminal (no colours or attributes, an approximate cell size).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{App, AppSpec, Color, Cx, El, KeyEvent, TextStyle, Tx, Waker, col, row, text};

use crate::config::{Chord, ColorScheme, Config, ResolvedProfile};
#[cfg(not(feature = "real-vt"))]
use crate::config::CursorShape;
use crate::launch::{Launch, process_env};
use crate::session::{Notify, Session, Tabs};

const TAB_BAR_H: f32 = 32.0;
const PAD: f32 = 6.0;

pub struct State {
    pub tabs: Tabs,
    config: Config,
    keymap: Vec<(String, Chord)>,
    waker: Arc<Mutex<Option<Waker>>>,
    notify: Notify,
    /// Grid size in cells every tab is sized to.
    grid: (u16, u16),
    font_size: f32,
    /// Only the stub text view reads this; `real-vt` draws through `wt_render`.
    #[cfg_attr(feature = "real-vt", allow(dead_code))]
    font_family_mono: bool,
    scheme: ColorScheme,
    /// Last laid-out screen rect, to refit the grid when the font size changes.
    screen: Option<[f32; 4]>,
    error: Option<String>,
}

impl State {
    pub fn new(launch: Launch) -> State {
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let slot = waker.clone();
        let notify: Notify = Arc::new(move || {
            if let Some(w) = slot.lock().unwrap().as_ref() {
                w();
            }
        });
        let first = launch.first;
        let mut s = State {
            tabs: Tabs::default(),
            keymap: launch.config.keymap(),
            grid: (launch.config.window.cols, launch.config.window.rows),
            font_size: first.font_size,
            font_family_mono: true,
            scheme: first.color_scheme.clone(),
            screen: None,
            config: launch.config,
            waker,
            notify,
            error: None,
        };
        s.open_tab(first);
        s
    }

    fn cell(&self) -> (f32, f32) {
        cell_size(self.font_size)
    }

    pub fn open_tab(&mut self, profile: ResolvedProfile) {
        let (cols, rows) = self.grid;
        match Session::spawn(
            profile,
            cols,
            rows,
            self.config.scrollback,
            self.notify.clone(),
        ) {
            Ok(s) => {
                self.tabs.push(s);
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn new_tab(&mut self) {
        match self.config.resolve_profile(None, &process_env) {
            Ok(p) => self.open_tab(p),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// Block until the first tab's child exits or goes quiet, so headless snapshots
    /// (`--rune --snap`) show its output instead of an empty screen.
    pub fn settle(&mut self, timeout: Duration, quiet: Duration) {
        let start = Instant::now();
        let mut last = Instant::now();
        while start.elapsed() < timeout {
            let d = self.tabs.drain_all();
            if d.changed {
                last = Instant::now();
            }
            let done = self
                .tabs
                .sessions
                .iter()
                .all(|s| s.is_finished() && s.exit_status().is_some());
            if done || (last.elapsed() >= quiet && !self.tabs.is_empty() && start.elapsed() > quiet)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The active tab's colour scheme (the first profile's while no tab is open).
    fn scheme(&self) -> &ColorScheme {
        self.tabs
            .active()
            .map_or(&self.scheme, |s| &s.profile.color_scheme)
    }

    fn fit(&mut self, rect: [f32; 4]) {
        self.screen = Some(rect);
        let (cw, ch) = self.cell();
        let cols = ((rect[2] - 2.0 * PAD) / cw).floor().clamp(1.0, 1000.0) as u16;
        let rows = ((rect[3] - 2.0 * PAD) / ch).floor().clamp(1.0, 1000.0) as u16;
        if (cols, rows) != self.grid {
            self.grid = (cols, rows);
            for s in &mut self.tabs.sessions {
                s.resize(cols, rows);
            }
        }
    }

    /// Run a keymap action; false leaves the key for the terminal.
    fn run_action(&mut self, action: &str, tx: &mut Tx) -> bool {
        match action {
            "new_tab" => self.new_tab(),
            "close_tab" => {
                self.tabs.close(self.tabs.active);
                if self.tabs.is_empty() {
                    tx.quit();
                }
            }
            "next_tab" => self.tabs.cycle(true),
            "prev_tab" => self.tabs.cycle(false),
            "quit" => tx.quit(),
            "increase_font_size" => self.font_size = (self.font_size + 1.0).min(72.0),
            "decrease_font_size" => self.font_size = (self.font_size - 1.0).max(6.0),
            "reset_font_size" => {
                self.font_size = self.tabs.active().map_or(12.0, |s| s.profile.font_size)
            }
            // TODO(wt-app): copy/paste/find/palette/splits/scrolling/fullscreen live in the shell.
            _ => return false,
        }
        if action.ends_with("_font_size")
            && let Some(r) = self.screen
        {
            self.fit(r);
        }
        true
    }
}

/// Approximate monospace cell for `size` points until wt-render reports real metrics.
pub fn cell_size(size: f32) -> (f32, f32) {
    (size * 0.6, (size * 1.3).round())
}

fn color(hex: &str, fallback: &str) -> Color {
    rune::color::hex(hex)
        .or_else(|| rune::color::hex(fallback))
        .unwrap_or(Color::TRANSPARENT)
}

fn view(s: &State, _cx: &mut Cx<State>) -> El {
    let fg = color(&s.scheme().foreground, "#cccccc");
    let bg = color(&s.scheme().background, "#0c0c0c");
    let tab_style = TextStyle::new(12.0, 500.0);
    let mut bar = row()
        .key("tabs")
        .h(TAB_BAR_H)
        .w_full()
        .px(PAD)
        .gap(4.0)
        .center_y()
        .fill(bg.mix(fg, 0.08));
    for (i, sess) in s.tabs.sessions.iter().enumerate() {
        let active = i == s.tabs.active;
        let mut title = sess.title.clone();
        match sess.exit_status().map(|e| e.code()) {
            Some(Some(code)) => title.push_str(&format!(" (exited: {code})")),
            Some(None) => title.push_str(" (killed)"),
            None if sess.is_finished() => title.push_str(" (exited)"),
            None => {}
        }
        bar = bar.child(
            row()
                .key(format!("tab-{i}"))
                .role("tab")
                .label(title.clone())
                .h(24.0)
                .px(10.0)
                .center_y()
                .radius(4.0)
                .fill(if active { bg } else { Color::TRANSPARENT })
                .on_press(move |s: &mut State| s.tabs.select(i))
                .child(
                    text(title)
                        .font(tab_style)
                        .color(if active { fg } else { fg.alpha(0.6) }),
                ),
        );
    }
    bar = bar.child(
        row()
            .key("new-tab")
            .role("button")
            .label("New tab")
            .h(24.0)
            .px(8.0)
            .center_y()
            .on_press(|s: &mut State| s.new_tab())
            .child(text("+").font(tab_style).color(fg)),
    );

    let mut screen = col()
        .key("screen")
        .grow(1.0)
        .w_full()
        .min_h(0.0)
        .px(PAD)
        .py(PAD)
        .fill(bg)
        .on_layout(|s: &mut State, r: [f32; 4]| s.fit(r));
    if let Some(e) = &s.error {
        screen = screen.child(
            text(format!("wt: {e}"))
                .key("error")
                .font(TextStyle::new(s.font_size, 400.0).mono())
                .color(color(&s.scheme().palette[9], "#e74856")),
        );
    }
    if let Some(sess) = s.tabs.active() {
        screen = screen.child(terminal_view(s, sess, fg));
    }
    col()
        .key("window")
        .w_full()
        .h_full()
        .fill(bg)
        .child(bar)
        .child(screen)
}

#[cfg(not(feature = "real-vt"))]
fn terminal_view(s: &State, sess: &Session, fg: Color) -> El {
    let (_, ch) = s.cell();
    let mut style = TextStyle::new(s.font_size, 400.0).lh(ch);
    if s.font_family_mono {
        style = style.mono();
    }
    let grid = sess.term.grid();
    let cur = sess.term.cursor();
    let mut body = col().key("terminal");
    for r in 0..grid.rows() {
        let mut line = grid.line_text(r);
        if cur.visible && cur.row == r && !sess.is_finished() {
            let mut chars: Vec<char> = line.chars().collect();
            chars.resize(chars.len().max(cur.col as usize + 1), ' ');
            chars[cur.col as usize] = match sess.profile.cursor_shape {
                CursorShape::Block => '█',
                CursorShape::Bar => '▏',
                CursorShape::Underline => '▁',
            };
            line = chars.into_iter().collect::<String>().trim_end().to_owned();
        }
        body = body.child(
            text(line)
                .key(format!("line-{r}"))
                .h(ch)
                .font(style)
                .color(fg),
        );
    }
    body
}

/// TODO(wt-render): written to the ARCHITECTURE.md signature; confirm once wt-render lands.
#[cfg(feature = "real-vt")]
fn terminal_view(_s: &State, sess: &Session, _fg: Color) -> El {
    wt_render::TerminalView::default().view(&sess.term, wt_render::Selection::default(), 0)
}

fn setup(s: &mut State, waker: Waker) {
    *s.waker.lock().unwrap() = Some(waker.clone());
    // Output that arrived before the waker existed set `pending` without waking anyone.
    waker();
}

fn poll(s: &mut State, _t: f64, tx: &mut Tx) -> bool {
    let d = s.tabs.drain_all();
    if d.more {
        tx.request_poll();
    }
    d.changed
}

fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let matched: Vec<String> = crate::input::actions(&s.keymap, ev)
        .map(str::to_owned)
        .collect();
    if matched.iter().any(|a| s.run_action(a, tx)) {
        return true;
    }
    match (crate::input::encode(ev), s.tabs.active_mut()) {
        (Some(bytes), Some(sess)) => {
            sess.write(&bytes);
            true
        }
        _ => false,
    }
}

fn fallback_init() -> State {
    let cli = crate::cli::Cli::default();
    State::new(crate::launch::prepare(&cli, &process_env).expect("default launch"))
}

pub fn app(launch: Launch) -> App<State> {
    let (cw, ch) = cell_size(launch.first.font_size);
    let w = &launch.config.window;
    let size = (
        w.cols as f32 * cw + 2.0 * PAD,
        w.rows as f32 * ch + 2.0 * PAD + TAB_BAR_H,
    );
    let title: &'static str = Box::leak(w.title.clone().into_boxed_str());
    let tooling = !launch.rune_args.is_empty();
    let args = std::iter::once("wt".to_owned()).chain(launch.rune_args.clone());
    App::new(AppSpec {
        min_size: Some((320.0, 200.0)),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new(title, size, fallback_init, view)
    })
    .args(args)
    .init(move || {
        let mut s = State::new(launch);
        if tooling {
            s.settle(Duration::from_secs(3), Duration::from_millis(300));
        }
        s
    })
}

pub fn run(launch: Launch) {
    app(launch).run();
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
