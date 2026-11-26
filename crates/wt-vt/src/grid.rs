//! Read-only view of the screen and scrollback.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as APoint};
use alacritty_terminal::term::cell::{Cell as ACell, Flags};
use alacritty_terminal::term::{Term, TermMode};

use crate::Listener;
use crate::cell::{Attrs, Cell, CellWidth};
use crate::color::{Color, Rgb};

/// A position in the grid.
///
/// `line` 0..rows is the live screen (0 = top). Negative lines are scrollback:
/// -1 is the most recent line scrolled off the top, `-history_len()` the oldest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Point {
    pub line: i32,
    pub col: u16,
}

impl Point {
    pub const fn new(line: i32, col: u16) -> Self {
        Self { line, col }
    }
}

/// A borrowed, read-only view of the terminal grid (screen + scrollback).
///
/// Obtained from [`crate::Terminal::grid`]. Cheap to create; holds a shared borrow.
#[derive(Clone, Copy)]
pub struct GridView<'a> {
    pub(crate) term: &'a Term<Listener>,
}

impl<'a> GridView<'a> {
    pub fn cols(&self) -> u16 {
        self.term.columns() as u16
    }

    /// Number of rows on the live screen.
    pub fn rows(&self) -> u16 {
        self.term.screen_lines() as u16
    }

    /// Number of scrollback lines currently stored (0 on the alt screen).
    pub fn history_len(&self) -> usize {
        self.term.grid().history_size()
    }

    /// True while the alternate screen (vim, less, htop...) is active.
    pub fn is_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Valid range of `line` values for [`Self::line`] / [`Self::cell_at`].
    pub fn line_range(&self) -> std::ops::Range<i32> {
        -(self.history_len() as i32)..self.rows() as i32
    }

    /// Map a viewport row to a grid line when the user has scrolled `scroll` lines up
    /// into history. `scroll` is clamped to [`Self::history_len`].
    pub fn viewport_line(&self, scroll: usize, row: u16) -> i32 {
        row as i32 - scroll.min(self.history_len()) as i32
    }

    /// Cell on the live screen. Panics if out of bounds.
    pub fn cell(&self, row: u16, col: u16) -> Cell<'a> {
        self.cell_at(Point::new(row as i32, col))
    }

    /// Cell anywhere in screen or scrollback. Panics if out of bounds.
    pub fn cell_at(&self, p: Point) -> Cell<'a> {
        self.check(p.line);
        convert(&self.term.grid()[APoint::new(Line(p.line), Column(p.col as usize))])
    }

    /// One line of cells (screen or scrollback). Panics if `line` is out of range.
    pub fn line(&self, line: i32) -> GridLine<'a> {
        self.check(line);
        GridLine {
            term: self.term,
            line,
        }
    }

    /// The `rows()` lines visible when scrolled `scroll` lines into history.
    pub fn viewport(&self, scroll: usize) -> impl Iterator<Item = GridLine<'a>> + 'a {
        let this = *self;
        (0..self.rows()).map(move |r| this.line(this.viewport_line(scroll, r)))
    }

    /// Text between two points (inclusive), as a selection copy would produce:
    /// soft-wrapped lines are joined, trailing blanks trimmed, wide spacers skipped.
    pub fn text_range(&self, start: Point, end: Point) -> String {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let clamp = |p: Point| {
            let r = self.line_range();
            let line = p.line.clamp(r.start, r.end - 1);
            APoint::new(
                Line(line),
                Column((p.col as usize).min(self.cols() as usize - 1)),
            )
        };
        let (start, end) = (clamp(start), clamp(end));
        let last = Column(self.cols() as usize - 1);
        let end_row = &self.term.grid()[end.line];
        if end.column == last
            && end_row[last]
                .flags
                .contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            // Alacritty appends the wrapped wide char from the *previous* row here (and
            // panics at the oldest row); take it from the next row ourselves.
            let mut text = self
                .term
                .bounds_to_string(start, APoint::new(end.line, last - 1));
            if end.line.0 + 1 < self.rows() as i32 {
                let next = &self.term.grid()[Line(end.line.0 + 1)][Column(0)];
                if next.flags.contains(Flags::WIDE_CHAR) {
                    text.push(next.c);
                    text.extend(next.zerowidth().into_iter().flatten());
                }
            }
            return text;
        }
        self.term.bounds_to_string(start, end)
    }

    /// The whole live screen as text, one `\n`-terminated row per screen row,
    /// trailing blanks trimmed. Handy for tests and debugging.
    pub fn screen_text(&self) -> String {
        let mut out = String::new();
        for r in 0..self.rows() {
            out.push_str(self.line(r as i32).text().trim_end());
            out.push('\n');
        }
        out
    }

    /// OSC 8 hyperlink URI attached to a cell, if any.
    pub fn hyperlink(&self, p: Point) -> Option<String> {
        self.check(p.line);
        self.term.grid()[APoint::new(Line(p.line), Column(p.col as usize))]
            .hyperlink()
            .map(|h| h.uri().to_owned())
    }

    fn check(&self, line: i32) {
        let r = self.line_range();
        assert!(r.contains(&line), "line {line} out of range {r:?}");
    }
}

/// One row of the grid.
#[derive(Clone, Copy)]
pub struct GridLine<'a> {
    term: &'a Term<Listener>,
    line: i32,
}

impl<'a> GridLine<'a> {
    /// Grid line index (see [`Point`]).
    pub fn index(&self) -> i32 {
        self.line
    }

    pub fn len(&self) -> usize {
        self.term.columns()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn cell(&self, col: u16) -> Cell<'a> {
        convert(&self.term.grid()[Line(self.line)][Column(col as usize)])
    }

    pub fn cells(&self) -> impl Iterator<Item = Cell<'a>> + 'a {
        let row = &self.term.grid()[Line(self.line)];
        (0..self.term.columns()).map(move |c| convert(&row[Column(c)]))
    }

    /// True if this line soft-wraps into the next one (DECAWM autowrap).
    pub fn is_wrapped(&self) -> bool {
        let row = &self.term.grid()[Line(self.line)];
        row[Column(self.term.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    }

    /// Row text with spacers skipped (not trimmed; one char per narrow column).
    pub fn text(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.cells() {
            if !c.is_spacer() {
                s.push(c.ch);
                s.extend(c.combining);
            }
        }
        s
    }
}

fn convert_color(c: alacritty_terminal::vte::ansi::Color) -> Color {
    use alacritty_terminal::vte::ansi::{Color as A, NamedColor as N};
    match c {
        A::Spec(rgb) => Color::Rgb(Rgb::new(rgb.r, rgb.g, rgb.b)),
        A::Indexed(i) => Color::Indexed(i),
        A::Named(n) => {
            let i = n as usize;
            if i < 16 {
                Color::Indexed(i as u8)
            } else {
                match n {
                    N::DimBlack => Color::Indexed(0),
                    N::DimRed => Color::Indexed(1),
                    N::DimGreen => Color::Indexed(2),
                    N::DimYellow => Color::Indexed(3),
                    N::DimBlue => Color::Indexed(4),
                    N::DimMagenta => Color::Indexed(5),
                    N::DimCyan => Color::Indexed(6),
                    N::DimWhite => Color::Indexed(7),
                    _ => Color::Default,
                }
            }
        }
    }
}

pub(crate) fn convert(c: &'_ ACell) -> Cell<'_> {
    let f = c.flags;
    let mut attrs = Attrs::empty();
    attrs.set(Attrs::BOLD, f.contains(Flags::BOLD));
    attrs.set(Attrs::DIM, f.contains(Flags::DIM));
    attrs.set(Attrs::ITALIC, f.contains(Flags::ITALIC));
    attrs.set(Attrs::UNDERLINE, f.intersects(Flags::ALL_UNDERLINES));
    attrs.set(Attrs::DOUBLE_UNDERLINE, f.contains(Flags::DOUBLE_UNDERLINE));
    attrs.set(Attrs::UNDERCURL, f.contains(Flags::UNDERCURL));
    attrs.set(Attrs::DOTTED_UNDERLINE, f.contains(Flags::DOTTED_UNDERLINE));
    attrs.set(Attrs::DASHED_UNDERLINE, f.contains(Flags::DASHED_UNDERLINE));
    attrs.set(Attrs::INVERSE, f.contains(Flags::INVERSE));
    attrs.set(Attrs::HIDDEN, f.contains(Flags::HIDDEN));
    attrs.set(Attrs::STRIKETHROUGH, f.contains(Flags::STRIKEOUT));
    attrs.set(Attrs::BLINK, f.contains(crate::BLINK_FLAG));
    let width = if f.contains(Flags::WIDE_CHAR) {
        CellWidth::Wide
    } else if f.contains(Flags::WIDE_CHAR_SPACER) {
        CellWidth::WideSpacer
    } else if f.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        CellWidth::LeadingSpacer
    } else {
        CellWidth::Narrow
    };
    Cell {
        // alacritty stores HT as '\t' in the cell it starts at (for copy); draw it blank.
        ch: if c.c == '\t' { ' ' } else { c.c },
        combining: c.zerowidth().unwrap_or(&[]),
        fg: convert_color(c.fg),
        bg: convert_color(c.bg),
        underline_color: c.underline_color().map(convert_color),
        attrs,
        width,
    }
}
