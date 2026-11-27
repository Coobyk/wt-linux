//! wt-vt — terminal emulation for wt-linux: VT bytes in, cell grid + scrollback out.
//!
//! The parser and grid are [`alacritty_terminal`] (0.26) wrapped behind wt-linux's own API
//! (see `docs/ARCHITECTURE.md`); nothing from alacritty leaks through the public types.
//!
//! ```
//! let mut term = wt_vt::Terminal::new(80, 24, 10_000);
//! term.feed(b"a\x1b[31mB");
//! let cell = term.grid().cell(0, 1);
//! assert_eq!(cell.ch, 'B');
//! assert_eq!(cell.fg, wt_vt::Color::Indexed(1));
//! ```
//!
//! # Resize policy: reflow
//! On the primary screen, changing the column count **reflows** soft-wrapped lines
//! (lines that auto-wrapped are re-joined/re-split; hard newlines are kept). Shrinking rows
//! pushes the top lines into scrollback (cursor stays on its content); growing rows pulls
//! lines back out of scrollback. The alternate screen is never reflowed: it is truncated /
//! padded, since full-screen apps redraw on `SIGWINCH` anyway.

mod cell;
mod color;
mod grid;
mod shim;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{self, ClipboardType, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{self, CursorShape as ACursorShape, NamedColor};

pub use cell::{Attrs, Cell, CellWidth};
pub use color::{CAMPBELL_ANSI, Color, ColorRole, Palette, Rgb, xterm_color};
pub use grid::{GridLine, GridView, Point};

/// Spare bit in alacritty's cell flags used to carry SGR 5/6 blink.
pub(crate) const BLINK_FLAG: Flags = Flags::from_bits_retain(1 << 15);

/// Collects alacritty's events during `feed`; drained right after.
#[derive(Clone, Default)]
pub(crate) struct Listener(Arc<Mutex<Vec<Event>>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Size {
    fn clamped(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(2) as usize,
            rows: rows.max(1) as usize,
        }
    }
}

/// `base` with the terminal's OSC 4/10/11/12 overrides applied.
pub(crate) fn effective_palette(term: &Term<Listener>, base: &Palette) -> Palette {
    let mut p = *base;
    let c = term.colors();
    let conv = |rgb: ansi::Rgb| Rgb::new(rgb.r, rgb.g, rgb.b);
    for i in 0..256 {
        if let Some(rgb) = c[i] {
            p.colors[i] = conv(rgb);
        }
    }
    if let Some(rgb) = c[NamedColor::Foreground] {
        p.foreground = conv(rgb);
    }
    if let Some(rgb) = c[NamedColor::Background] {
        p.background = conv(rgb);
    }
    if let Some(rgb) = c[NamedColor::Cursor] {
        p.cursor = conv(rgb);
    }
    p
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Cursor shape requested by the application (DECSCUSR) or the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    HollowBlock,
}

/// Where and how to draw the cursor. `row`/`col` are live-screen coordinates
/// (row 0 = top of the screen, not of the scrolled viewport).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub shape: CursorShape,
    /// False when the app hid the cursor (DECTCEM `CSI ?25l`).
    pub visible: bool,
    /// The app asked for a blinking cursor (DECSCUSR odd values / `CSI ?12h`).
    pub blinking: bool,
    /// The cursor sits on a double-width character; draw it 2 cells wide.
    pub wide: bool,
}

/// Terminal modes the app shell needs for input encoding and UI decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modes {
    /// DECCKM: arrow keys send `ESC O A` instead of `ESC [ A`.
    pub app_cursor: bool,
    /// DECKPAM: keypad sends application sequences.
    pub app_keypad: bool,
    /// `CSI ?2004h`: wrap pastes in `ESC[200~` / `ESC[201~`.
    pub bracketed_paste: bool,
    /// `CSI ?1004h`: send `ESC[I` / `ESC[O` on focus change.
    pub focus_events: bool,
    pub alt_screen: bool,
    /// `CSI ?1007h` (default on): wheel on the alt screen sends arrow keys.
    pub alternate_scroll: bool,
    /// DECAWM autowrap.
    pub autowrap: bool,
    /// LNM: Enter should send CR LF.
    pub line_feed_new_line: bool,
    pub mouse: MouseMode,
    /// `CSI ?1006h` SGR mouse encoding.
    pub sgr_mouse: bool,
    /// `CSI ?1005h` UTF-8 mouse encoding.
    pub utf8_mouse: bool,
    /// Kitty keyboard protocol flags (`CSI > flags u`), raw bits; 0 = legacy encoding.
    pub kitty_keyboard: u8,
}

/// Which mouse events the app wants reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseMode {
    #[default]
    Off,
    /// `?1000`: press/release only.
    Click,
    /// `?1002`: press/release + motion while a button is held.
    Drag,
    /// `?1003`: all motion.
    Motion,
}

/// Rows changed since the last [`Terminal::take_damage`] call, in live-screen coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Redraw everything (first frame, resize, alt-screen switch, full clear, palette change...).
    Full,
    /// Only these rows changed; columns `left..=right` within each.
    Rows(Vec<RowDamage>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowDamage {
    pub row: u16,
    pub left: u16,
    pub right: u16,
}

/// Things the terminal wants the app shell to act on. Drain with [`Terminal::take_events`].
#[derive(Clone)]
pub enum TermEvent {
    /// OSC 0/2 window/tab title.
    Title(String),
    /// Title reset to the profile default.
    ResetTitle,
    /// BEL.
    Bell,
    /// OSC 52 copy: put `text` on the clipboard (`primary` = X11 PRIMARY selection).
    ClipboardStore { primary: bool, text: String },
    /// OSC 52 paste request: call `format(clipboard_text)` and write the result to the PTY.
    ClipboardLoad {
        primary: bool,
        format: Arc<dyn Fn(&str) -> String + Send + Sync>,
    },
    /// Cursor blink setting changed.
    CursorBlinkingChange,
    /// The mouse cursor icon may need to change (mouse mode toggled, OSC 22).
    MouseCursorDirty,
}

impl std::fmt::Debug for TermEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Title(t) => write!(f, "Title({t:?})"),
            Self::ResetTitle => write!(f, "ResetTitle"),
            Self::Bell => write!(f, "Bell"),
            Self::ClipboardStore { primary, text } => {
                write!(f, "ClipboardStore {{ primary: {primary}, text: {text:?} }}")
            }
            Self::ClipboardLoad { primary, .. } => {
                write!(f, "ClipboardLoad {{ primary: {primary} }}")
            }
            Self::CursorBlinkingChange => write!(f, "CursorBlinkingChange"),
            Self::MouseCursorDirty => write!(f, "MouseCursorDirty"),
        }
    }
}

/// A terminal: VT parser + screen grid + scrollback + modes.
///
/// Not tied to a PTY or a renderer: feed it child output with [`Terminal::feed`], write
/// [`Terminal::take_replies`] back to the child, and render from [`Terminal::grid`] /
/// [`Terminal::cursor`].
pub struct Terminal {
    term: Term<Listener>,
    parser: ansi::Processor,
    listener: Listener,
    palette: Palette,
    replies: Vec<u8>,
    events: Vec<TermEvent>,
    title: Option<String>,
    cell_px: Option<(u16, u16)>,
    full_damage: bool,
    /// The shape [`Self::cursor`] reports until the child sets DECSCUSR (the profile's
    /// `cursorShape`). DECSCUSR 0 (reset) returns the cursor to this.
    default_shape: CursorShape,
    shape_set: bool,
}

impl Terminal {
    /// `scrollback` is the maximum number of history lines kept for the primary screen.
    /// `cols` is clamped to at least 2 (room for one wide character) and `rows` to at least 1.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let listener = Listener::default();
        let config = term::Config {
            scrolling_history: scrollback,
            ..Default::default()
        };
        let size = Size::clamped(cols, rows);
        Self {
            term: Term::new(config, &size, listener.clone()),
            parser: ansi::Processor::new(),
            listener,
            palette: Palette::default(),
            replies: Vec::new(),
            events: Vec::new(),
            title: None,
            cell_px: None,
            full_damage: false,
            default_shape: CursorShape::Block,
            shape_set: false,
        }
    }

    /// The shape drawn while the child hasn't sent DECSCUSR (Windows Terminal's per-profile
    /// `cursorShape`). Does not change what a DECSCUSR 0 reset restores — that's alacritty's.
    pub fn set_default_cursor_shape(&mut self, shape: CursorShape) {
        self.default_shape = shape;
    }

    /// Parse child output and update state. Partial escape sequences and partial UTF-8
    /// are buffered across calls, so arbitrary chunking is fine.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.flush_expired_sync_update();
        let mut shim = shim::Shim {
            term: &mut self.term,
            listener: &self.listener,
            palette: &self.palette,
            shape_set: &mut self.shape_set,
        };
        self.parser.advance(&mut shim, bytes);
        self.drain_alacritty_events();
    }

    /// Deadline of the pending synchronized update (`CSI ?2026h`), if one is buffering.
    /// While it is pending, output is held back; the event loop should call
    /// [`Self::flush_expired_sync_update`] once this instant passes, even if no more
    /// bytes arrive.
    pub fn sync_update_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    /// Apply a stalled synchronized update whose deadline has passed. Returns whether
    /// anything was flushed. Also done automatically at the start of [`Self::feed`].
    pub fn flush_expired_sync_update(&mut self) -> bool {
        match self.sync_update_deadline() {
            Some(deadline) if Instant::now() >= deadline => {
                self.flush_sync_update();
                true
            }
            _ => false,
        }
    }

    /// Apply any buffered synchronized update now (e.g. when the child exits mid-update).
    pub fn flush_sync_update(&mut self) {
        if self.sync_update_deadline().is_none() {
            return;
        }
        let mut shim = shim::Shim {
            term: &mut self.term,
            listener: &self.listener,
            palette: &self.palette,
            shape_set: &mut self.shape_set,
        };
        self.parser.stop_sync(&mut shim);
        self.drain_alacritty_events();
    }

    /// Resize the grid (see the crate docs for the reflow policy). Remember to also
    /// resize the PTY. Sizes are clamped like in [`Self::new`].
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(Size::clamped(cols, rows));
    }

    pub fn grid(&self) -> GridView<'_> {
        GridView { term: &self.term }
    }

    pub fn cursor(&self) -> CursorState {
        let point = self.term.grid().cursor.point;
        let mut col = point.column.0;
        let flags = self.term.grid()[point].flags;
        if flags.contains(Flags::WIDE_CHAR_SPACER) && col > 0 {
            col -= 1;
        }
        let wide = self.term.grid()[point.line][alacritty_terminal::index::Column(col)]
            .flags
            .contains(Flags::WIDE_CHAR);
        let style = self.term.cursor_style();
        let shape = if self.shape_set {
            match style.shape {
                ACursorShape::Block | ACursorShape::Hidden => CursorShape::Block,
                ACursorShape::Underline => CursorShape::Underline,
                ACursorShape::Beam => CursorShape::Beam,
                ACursorShape::HollowBlock => CursorShape::HollowBlock,
            }
        } else {
            self.default_shape
        };
        CursorState {
            row: point.line.0.max(0) as u16,
            col: col as u16,
            shape,
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR)
                && style.shape != ACursorShape::Hidden,
            blinking: style.blinking,
            wide,
        }
    }

    pub fn cols(&self) -> u16 {
        self.term.columns() as u16
    }

    pub fn rows(&self) -> u16 {
        self.term.screen_lines() as u16
    }

    pub fn modes(&self) -> Modes {
        let m = *self.term.mode();
        let mouse = if m.contains(TermMode::MOUSE_MOTION) {
            MouseMode::Motion
        } else if m.contains(TermMode::MOUSE_DRAG) {
            MouseMode::Drag
        } else if m.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseMode::Click
        } else {
            MouseMode::Off
        };
        let kitty = [
            TermMode::DISAMBIGUATE_ESC_CODES,
            TermMode::REPORT_EVENT_TYPES,
            TermMode::REPORT_ALTERNATE_KEYS,
            TermMode::REPORT_ALL_KEYS_AS_ESC,
            TermMode::REPORT_ASSOCIATED_TEXT,
        ]
        .iter()
        .enumerate()
        .fold(
            0u8,
            |acc, (i, f)| if m.contains(*f) { acc | (1 << i) } else { acc },
        );
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            focus_events: m.contains(TermMode::FOCUS_IN_OUT),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            autowrap: m.contains(TermMode::LINE_WRAP),
            line_feed_new_line: m.contains(TermMode::LINE_FEED_NEW_LINE),
            mouse,
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            utf8_mouse: m.contains(TermMode::UTF8_MOUSE),
            kitty_keyboard: kitty,
        }
    }

    /// Bytes the terminal must send back to the child (DSR/CPR, DA, DECRQM, OSC color
    /// queries, ...). Write these to the PTY after every [`Terminal::feed`].
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Title, bell, clipboard and other UI events since the last call.
    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// Current OSC 0/2 title (`None` = never set or reset).
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Rows changed since the last call, then resets damage tracking.
    ///
    /// Coordinates are live-screen rows. If the renderer shows a scrolled viewport
    /// (`scroll > 0`), it should treat a change of `scroll` as full damage itself.
    /// Cursor movement damages both the old and new cursor cells.
    pub fn take_damage(&mut self) -> Damage {
        let last_col = self.term.columns() - 1;
        let full = std::mem::take(&mut self.full_damage);
        let damage = match self.term.damage() {
            _ if full => Damage::Full,
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(it) => Damage::Rows(
                it.map(|d| RowDamage {
                    row: d.line as u16,
                    left: d.left.min(last_col) as u16,
                    right: d.right.min(last_col) as u16,
                })
                .collect(),
            ),
        };
        self.term.reset_damage();
        damage
    }

    /// Effective palette: the base palette with any OSC 4/10/11/12 overrides applied.
    pub fn palette(&self) -> Palette {
        effective_palette(&self.term, &self.palette)
    }

    /// Set the profile's color scheme. App OSC overrides still win over it.
    /// The next [`Self::take_damage`] reports full damage.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.full_damage = true;
    }

    /// Cell size in pixels, used to answer `CSI 14 t` (text area size in pixels).
    pub fn set_cell_size_px(&mut self, width: u16, height: u16) {
        self.cell_px = Some((width, height));
    }

    /// Tell the terminal whether its pane has keyboard focus. Sends `ESC[I`/`ESC[O`
    /// to the child when focus reporting (`?1004`) is enabled.
    pub fn set_focused(&mut self, focused: bool) {
        if self.term.is_focused != focused {
            self.term.is_focused = focused;
            if self.term.mode().contains(TermMode::FOCUS_IN_OUT) {
                self.replies
                    .extend_from_slice(if focused { b"\x1b[I" } else { b"\x1b[O" });
            }
        }
    }

    /// Drop all scrollback history.
    pub fn clear_scrollback(&mut self) {
        self.term.grid_mut().clear_history();
    }

    fn drain_alacritty_events(&mut self) {
        let events =
            std::mem::take(&mut *self.listener.0.lock().unwrap_or_else(|e| e.into_inner()));
        for ev in events {
            match ev {
                Event::PtyWrite(s) => self.replies.extend_from_slice(s.as_bytes()),
                // Answered in order by the shim; alacritty's own request path is unused.
                Event::ColorRequest(..) => {}
                Event::TextAreaSizeRequest(fmt) => {
                    let (cw, ch) = self.cell_px.unwrap_or((0, 0));
                    let ws = WindowSize {
                        num_lines: self.rows(),
                        num_cols: self.cols(),
                        cell_width: cw,
                        cell_height: ch,
                    };
                    self.replies.extend_from_slice(fmt(ws).as_bytes());
                }
                Event::Title(t) => {
                    self.title = Some(t.clone());
                    self.events.push(TermEvent::Title(t));
                }
                Event::ResetTitle => {
                    self.title = None;
                    self.events.push(TermEvent::ResetTitle);
                }
                Event::Bell => self.events.push(TermEvent::Bell),
                Event::ClipboardStore(ty, text) => self.events.push(TermEvent::ClipboardStore {
                    primary: matches!(ty, ClipboardType::Selection),
                    text,
                }),
                Event::ClipboardLoad(ty, format) => self.events.push(TermEvent::ClipboardLoad {
                    primary: matches!(ty, ClipboardType::Selection),
                    format,
                }),
                Event::CursorBlinkingChange => self.events.push(TermEvent::CursorBlinkingChange),
                Event::MouseCursorDirty => self.events.push(TermEvent::MouseCursorDirty),
                Event::Wakeup | Event::Exit | Event::ChildExit(_) => {}
            }
        }
    }
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("cols", &self.cols())
            .field("rows", &self.rows())
            .field("history", &self.grid().history_len())
            .field("cursor", &self.cursor())
            .finish_non_exhaustive()
    }
}
