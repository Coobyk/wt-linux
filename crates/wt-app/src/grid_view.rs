//! Draws a pane's grid as Rune elements: one row per line, one fixed-width box per run of
//! same-styled cells, and a cursor block. A stand-in for wt-render's `TerminalView` until it
//! lands; that component should replace `grid_view` with the same inputs.
use rune::{Color, El, TextStyle, col, el, row, text};

use crate::app::cell_size;
use crate::scheme::{ColorScheme, rgb};
use crate::terminal::{Attrs, Cell, CellColor, CursorState, GridSnapshot};

pub struct GridLook<'a> {
    pub scheme: &'a ColorScheme,
    pub style: TextStyle,
    pub focused: bool,
}

/// Bold brightens the 8 base colours, as Windows Terminal does by default.
fn resolve(c: CellColor, scheme: &ColorScheme, default: Color, bold: bool) -> Color {
    match c {
        CellColor::Default => default,
        CellColor::Indexed(i) if bold && i < 8 => scheme.indexed(i + 8),
        CellColor::Indexed(i) => scheme.indexed(i),
        CellColor::Rgb(r, g, b) => rgb(r, g, b),
    }
}

/// `(fg, bg)` for a cell; `bg` is `None` when it's the scheme background.
fn colors(cell: &Cell, scheme: &ColorScheme) -> (Color, Option<Color>) {
    let bold = cell.attrs.has(Attrs::BOLD);
    let mut fg = resolve(cell.fg, scheme, scheme.fg(), bold);
    let mut bg = match cell.bg {
        CellColor::Default => None,
        c => Some(resolve(c, scheme, scheme.bg(), false)),
    };
    if cell.attrs.has(Attrs::INVERSE) {
        let old_bg = bg.unwrap_or(scheme.bg());
        bg = Some(fg);
        fg = old_bg;
    }
    if cell.attrs.has(Attrs::DIM) {
        fg = fg.alpha(0.6);
    }
    (fg, bg)
}

fn same_style(a: &Cell, b: &Cell) -> bool {
    a.fg == b.fg && a.bg == b.bg && a.attrs == b.attrs
}

fn row_view(cells: &[Cell], look: &GridLook, cw: f32, lh: f32) -> El {
    let mut out = row().h(lh);
    // Trailing default blanks draw nothing.
    let end = cells
        .iter()
        .rposition(|c| c.ch != ' ' || c.bg != CellColor::Default || c.attrs.has(Attrs::INVERSE))
        .map_or(0, |i| i + 1);
    let mut i = 0;
    while i < end {
        let start = i;
        while i < end && same_style(&cells[start], &cells[i]) {
            i += 1;
        }
        let run = &cells[start..i];
        let (fg, bg) = colors(&run[0], look.scheme);
        let mut style = look.style;
        if run[0].attrs.has(Attrs::BOLD) {
            style.weight = 700.0;
        }
        if run[0].attrs.has(Attrs::ITALIC) {
            style = style.italic();
        }
        let s: String = run.iter().map(|c| c.ch).collect();
        let mut cell_box = el().w(run.len() as f32 * cw).h(lh).center_y();
        if let Some(bg) = bg {
            cell_box = cell_box.fill(bg);
        }
        if !s.trim().is_empty() {
            cell_box = cell_box.child(text(s).font(style).color(fg).nowrap());
        }
        out = out.child(cell_box);
    }
    out
}

pub fn grid_view(grid: &GridSnapshot, cursor: CursorState, look: &GridLook) -> El {
    let (cw, lh) = cell_size(look.style.size);
    let mut g = col()
        .key("grid")
        .w(grid.cols as f32 * cw)
        .h(grid.rows as f32 * lh);
    if cursor.visible && cursor.row < grid.rows && cursor.col < grid.cols {
        let c = look.scheme.cursor_color();
        let mut block = el()
            .key("cursor")
            .size(cw, lh)
            .absolute(cursor.col as f32 * cw, cursor.row as f32 * lh);
        block = if look.focused {
            block.fill(c.alpha(0.75))
        } else {
            block.border(1.0, c)
        };
        g = g.child(block);
    }
    for r in 0..grid.rows {
        g = g.child(row_view(grid.row(r), look, cw, lh).key(format!("r{r}")));
    }
    g
}
