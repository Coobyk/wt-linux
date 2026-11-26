//! The render-side view of a terminal grid: what `TerminalView` reads from a `wt-vt` `Terminal`.

/// A colour as the VT layer stores it, resolved against a [`crate::Palette`] at draw time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VtColor {
    /// The palette's default foreground (for `fg`) or background (for `bg`).
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// SGR attributes, a bit set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attrs(pub u16);

impl Attrs {
    pub const BOLD: Attrs = Attrs(1);
    pub const DIM: Attrs = Attrs(1 << 1);
    pub const ITALIC: Attrs = Attrs(1 << 2);
    pub const UNDERLINE: Attrs = Attrs(1 << 3);
    pub const INVERSE: Attrs = Attrs(1 << 4);
    pub const HIDDEN: Attrs = Attrs(1 << 5);
    pub const STRIKE: Attrs = Attrs(1 << 6);

    pub fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Attrs {
    type Output = Attrs;
    fn bitor(self, rhs: Attrs) -> Attrs {
        Attrs(self.0 | rhs.0)
    }
}

/// How many columns a cell takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Width {
    #[default]
    Narrow,
    /// The first column of a double-width character.
    Wide,
    /// The second column of a double-width character: draws nothing itself.
    Spacer,
}

/// One grid cell.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub ch: char,
    /// Combining marks that follow `ch` in the same grapheme cluster.
    pub zerowidth: Option<Box<str>>,
    pub fg: VtColor,
    pub bg: VtColor,
    pub attrs: Attrs,
    pub width: Width,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            ch: ' ',
            zerowidth: None,
            fg: VtColor::Default,
            bg: VtColor::Default,
            attrs: Attrs::default(),
            width: Width::Narrow,
        }
    }
}

impl Cell {
    pub fn new(ch: char) -> Cell {
        Cell {
            ch,
            ..Cell::default()
        }
    }

    pub fn fg(mut self, fg: VtColor) -> Cell {
        self.fg = fg;
        self
    }

    pub fn bg(mut self, bg: VtColor) -> Cell {
        self.bg = bg;
        self
    }

    pub fn attrs(mut self, attrs: Attrs) -> Cell {
        self.attrs = attrs;
        self
    }

    pub fn width(mut self, width: Width) -> Cell {
        self.width = width;
        self
    }
}

/// DECSCUSR cursor shapes (blink is the host's business: toggle `visible`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    #[default]
    Block,
    /// A bar at the cell's left edge.
    Line,
    Underline,
}

/// Where the cursor is, on the live screen (row 0 is the top screen row, not scrollback).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CursorState {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub visible: bool,
}

/// A grid position. `line` 0 is the top screen row; negative lines are scrollback, -1 the newest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub line: i32,
    pub col: u16,
}

impl Point {
    pub fn new(line: i32, col: u16) -> Point {
        Point { line, col }
    }
}

/// What is selected. Both ends are inclusive and may come in either order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Selection {
    #[default]
    None,
    /// Text-flow selection: from `start` to the end of its line, whole lines between, up to `end`.
    Range { start: Point, end: Point },
    /// Rectangular (alt-drag) selection.
    Block { start: Point, end: Point },
}

impl Selection {
    /// The selected columns `start..end` (exclusive) of `line`, in a grid `cols` wide.
    pub fn cols_on(&self, line: i32, cols: u16) -> Option<(u16, u16)> {
        let (span, a, b) = match *self {
            Selection::None => return None,
            Selection::Range { start, end } => (true, start.min(end), start.max(end)),
            Selection::Block { start, end } => (false, start, end),
        };
        let (top, bottom) = (a.line.min(b.line), a.line.max(b.line));
        if line < top || line > bottom {
            return None;
        }
        let (s, e) = if span {
            let s = if line == a.line { a.col } else { 0 };
            let e = if line == b.line {
                b.col.saturating_add(1)
            } else {
                cols
            };
            (s, e)
        } else {
            (a.col.min(b.col), a.col.max(b.col).saturating_add(1))
        };
        let (s, e) = (s.min(cols), e.min(cols));
        (s < e).then_some((s, e))
    }
}

/// What `TerminalView` reads from a terminal. Until `wt-vt` ships `GridView`, only fakes
/// implement it (see the crate README); the `wt_vt::Terminal` adapter goes in this crate.
pub trait TermSource {
    /// `(cols, rows)` of the live screen.
    fn size(&self) -> (u16, u16);
    /// How many scrollback lines exist above the screen.
    fn history(&self) -> usize;
    /// Appends the cells of `line` (see [`Point::line`]) to `out`; a short line is padded blank.
    fn line(&self, line: i32, out: &mut Vec<Cell>);
    fn cursor(&self) -> CursorState;
}
