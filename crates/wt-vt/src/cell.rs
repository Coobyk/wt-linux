//! A single grid cell as seen by renderers.

use crate::color::Color;

/// Text attributes of a cell (a small bitset).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attrs(u16);

impl std::fmt::Debug for Attrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const NAMES: [&str; 12] = [
            "BOLD",
            "DIM",
            "ITALIC",
            "UNDERLINE",
            "DOUBLE_UNDERLINE",
            "UNDERCURL",
            "DOTTED_UNDERLINE",
            "DASHED_UNDERLINE",
            "BLINK",
            "INVERSE",
            "HIDDEN",
            "STRIKETHROUGH",
        ];
        let set: Vec<&str> = NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| self.0 & (1 << i) != 0)
            .map(|(_, n)| *n)
            .collect();
        write!(
            f,
            "Attrs({})",
            if set.is_empty() {
                "-".to_owned()
            } else {
                set.join(" | ")
            }
        )
    }
}

impl Attrs {
    pub const BOLD: Self = Self(1 << 0);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    /// Any underline style; see also the more specific underline flags below.
    pub const UNDERLINE: Self = Self(1 << 3);
    pub const DOUBLE_UNDERLINE: Self = Self(1 << 4);
    pub const UNDERCURL: Self = Self(1 << 5);
    pub const DOTTED_UNDERLINE: Self = Self(1 << 6);
    pub const DASHED_UNDERLINE: Self = Self(1 << 7);
    pub const BLINK: Self = Self(1 << 8);
    pub const INVERSE: Self = Self(1 << 9);
    pub const HIDDEN: Self = Self(1 << 10);
    pub const STRIKETHROUGH: Self = Self(1 << 11);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True if all bits of `other` are set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.insert(other)
        } else {
            self.remove(other)
        }
    }
}

impl std::ops::BitOr for Attrs {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Attrs {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// How a cell participates in double-width characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CellWidth {
    /// Ordinary one-column cell.
    #[default]
    Narrow,
    /// First column of a two-column character (CJK, emoji). Draw the glyph here over 2 cells.
    Wide,
    /// Second column of a wide character. Draw nothing (its bg equals the wide cell's).
    WideSpacer,
    /// Last column of a row that was left empty because a wide char didn't fit and
    /// wrapped to the next row. Draw as a blank.
    LeadingSpacer,
}

/// One grid cell.
///
/// `ch` plus `combining` is the full grapheme cluster (e.g. `e` + U+0301).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    pub ch: char,
    /// Zero-width / combining characters appended to `ch`.
    pub combining: &'a [char],
    pub fg: Color,
    pub bg: Color,
    /// SGR 58 underline color; `None` means "use fg".
    pub underline_color: Option<Color>,
    pub attrs: Attrs,
    pub width: CellWidth,
}

impl Cell<'_> {
    /// True for the first column of a double-width character.
    pub fn is_wide(&self) -> bool {
        self.width == CellWidth::Wide
    }

    /// True for cells a renderer should not draw a glyph in (spacers).
    pub fn is_spacer(&self) -> bool {
        matches!(self.width, CellWidth::WideSpacer | CellWidth::LeadingSpacer)
    }

    /// Number of columns the glyph occupies (0 for spacers).
    pub fn columns(&self) -> usize {
        match self.width {
            CellWidth::Narrow => 1,
            CellWidth::Wide => 2,
            _ => 0,
        }
    }

    /// The grapheme cluster as a `String` (`ch` + combining marks).
    pub fn text(&self) -> String {
        let mut s = String::with_capacity(4);
        s.push(self.ch);
        s.extend(self.combining);
        s
    }

    /// True if the cell is a blank with default colors and no attributes.
    pub fn is_blank(&self) -> bool {
        self.ch == ' '
            && self.combining.is_empty()
            && self.fg == Color::Default
            && self.bg == Color::Default
            && self.attrs.is_empty()
    }
}
