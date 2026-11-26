//! `TermSource` for `wt_vt::Terminal`: the adapter [`crate::TerminalView`] renders through.

use wt_vt::{Attrs as VAttrs, Cell as VCell, CellWidth, Color as VCol, Terminal};

use crate::model::{Attrs, Cell, CursorShape, CursorState, TermSource, VtColor, Width};

impl TermSource for Terminal {
    fn size(&self) -> (u16, u16) {
        (self.cols(), self.rows())
    }

    fn history(&self) -> usize {
        self.grid().history_len()
    }

    fn line(&self, line: i32, out: &mut Vec<Cell>) {
        out.extend(self.grid().line(line).cells().map(cell));
    }

    fn cursor(&self) -> CursorState {
        let c = Terminal::cursor(self);
        CursorState {
            col: c.col,
            row: c.row,
            shape: match c.shape {
                wt_vt::CursorShape::Block | wt_vt::CursorShape::HollowBlock => CursorShape::Block,
                wt_vt::CursorShape::Beam => CursorShape::Line,
                wt_vt::CursorShape::Underline => CursorShape::Underline,
            },
            visible: c.visible,
        }
    }
}

fn color(c: VCol) -> VtColor {
    match c {
        VCol::Default => VtColor::Default,
        VCol::Indexed(i) => VtColor::Indexed(i),
        VCol::Rgb(rgb) => VtColor::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

fn cell(c: VCell<'_>) -> Cell {
    let mut attrs = Attrs::default();
    let set = |f: VAttrs, to: Attrs, out: &mut Attrs| {
        if c.attrs.contains(f) {
            *out = *out | to;
        }
    };
    set(VAttrs::BOLD, Attrs::BOLD, &mut attrs);
    set(VAttrs::DIM, Attrs::DIM, &mut attrs);
    set(VAttrs::ITALIC, Attrs::ITALIC, &mut attrs);
    set(
        VAttrs::UNDERLINE
            | VAttrs::DOUBLE_UNDERLINE
            | VAttrs::UNDERCURL
            | VAttrs::DOTTED_UNDERLINE
            | VAttrs::DASHED_UNDERLINE,
        Attrs::UNDERLINE,
        &mut attrs,
    );
    set(VAttrs::INVERSE, Attrs::INVERSE, &mut attrs);
    set(VAttrs::HIDDEN, Attrs::HIDDEN, &mut attrs);
    set(VAttrs::STRIKETHROUGH, Attrs::STRIKE, &mut attrs);
    Cell {
        ch: c.ch,
        zerowidth: (!c.combining.is_empty())
            .then(|| c.combining.iter().collect::<String>().into_boxed_str()),
        fg: color(c.fg),
        bg: color(c.bg),
        attrs,
        width: match c.width {
            CellWidth::Narrow => Width::Narrow,
            CellWidth::Wide => Width::Wide,
            CellWidth::WideSpacer | CellWidth::LeadingSpacer => Width::Spacer,
        },
    }
}
