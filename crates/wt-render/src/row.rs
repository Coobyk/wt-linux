//! One terminal row as a Rune `GridRow`: background runs as fills, ASCII as one shaped line with
//! colour spans, everything else pinned to its cell, decorations and the cursor as a vector layer.

use rune::{Color, GridRow, Shape, Span, TextStyle};

use crate::model::{Attrs, Cell, CursorShape, VtColor, Width};
use crate::palette::{Palette, color};

/// Cell geometry in points and the font rows are set in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub style: TextStyle,
    pub cell_w: f32,
    pub cell_h: f32,
}

/// The cursor as it shows on one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RowCursor {
    pub col: u16,
    pub shape: CursorShape,
    /// Unfocused terminals draw a hollow box whatever the shape.
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RowInput<'a> {
    pub cells: &'a [Cell],
    pub cols: u16,
    pub sel: Option<(u16, u16)>,
    pub cursor: Option<RowCursor>,
}

struct Look {
    fg: Color,
    bg: [u8; 3],
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    hidden: bool,
}

fn look(cell: &Cell, pal: &Palette, selected: bool, block_cursor: bool) -> Look {
    let a = cell.attrs;
    let bold = a.contains(Attrs::BOLD);
    let fg_vt = match cell.fg {
        VtColor::Indexed(i) if bold && i < 8 => VtColor::Indexed(i + 8),
        c => c,
    };
    let mut fg = pal.resolve(fg_vt, pal.fg);
    let mut bg = pal.resolve(cell.bg, pal.bg);
    if a.contains(Attrs::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if selected {
        bg = pal.selection;
    }
    if block_cursor {
        fg = bg;
        bg = pal.cursor;
    }
    let mut fg_c = color(fg);
    if a.contains(Attrs::DIM) {
        fg_c = fg_c.mix(color(bg), 0.4);
    }
    Look {
        fg: fg_c,
        bg,
        bold,
        italic: a.contains(Attrs::ITALIC),
        underline: a.contains(Attrs::UNDERLINE),
        strike: a.contains(Attrs::STRIKE),
        hidden: a.contains(Attrs::HIDDEN),
    }
}

fn rect(shape: Shape, x: f32, y: f32, w: f32, h: f32, c: Color) -> Shape {
    shape.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], c)
}

fn styled(base: TextStyle, bold: bool, italic: bool) -> TextStyle {
    let s = if bold {
        TextStyle {
            weight: 700.0,
            ..base
        }
    } else {
        base
    };
    if italic { s.italic() } else { s }
}

#[derive(PartialEq)]
struct RunKey {
    fg: [u32; 4],
    bold: bool,
    italic: bool,
    underline: bool,
}

fn span_for(start: usize, end: usize, k: &RunKey, fg: Color) -> Span {
    let mut s = Span::new(start, end).color(fg);
    if k.bold {
        s = s.bold();
    }
    if k.italic {
        s = s.italic();
    }
    if k.underline {
        s = s.underline(fg);
    }
    s
}

/// One row's draw lists before they become a `GridRow`.
pub(crate) struct RowParts {
    pub line: String,
    pub spans: Vec<Span>,
    pub pinned: Vec<(u16, u16, String, TextStyle, Color)>,
    pub fills: Vec<(u16, u16, [u8; 3])>,
    pub over: Option<Shape>,
}

/// Builds the `GridRow` for one visible row. Pure: same input, same row.
pub(crate) fn build(inp: &RowInput, pal: &Palette, m: &Metrics) -> GridRow {
    let p = parts(inp, pal, m);
    let mut row = GridRow::new(p.line, p.spans, m.style);
    for (start, end, rgb) in p.fills {
        row = row.fill(start, end, color(rgb));
    }
    for (col, w, text, style, fg) in p.pinned {
        row = row.cell(col, w, text, style, fg);
    }
    if let Some(over) = p.over {
        row = row.shape(over);
    }
    row
}

pub(crate) fn parts(inp: &RowInput, pal: &Palette, m: &Metrics) -> RowParts {
    let blank = Cell::default();
    let cols = inp.cols as usize;
    let (cw, ch) = (m.cell_w, m.cell_h);
    let cursor_w = inp.cursor.map_or(1, |c| {
        if inp
            .cells
            .get(c.col as usize)
            .is_some_and(|x| x.width == Width::Wide)
        {
            2
        } else {
            1
        }
    });
    let in_cursor = |c: u16| {
        inp.cursor
            .is_some_and(|k| c >= k.col && c < k.col + cursor_w)
    };
    let block = inp
        .cursor
        .is_some_and(|k| k.focused && k.shape == CursorShape::Block);

    let mut line = String::with_capacity(cols);
    let mut spans: Vec<Span> = Vec::new();
    let mut run: Option<(usize, RunKey, Color)> = None;
    let mut pinned: Vec<(u16, u16, String, TextStyle, Color)> = Vec::new();
    let mut fills: Vec<(u16, u16, [u8; 3])> = Vec::new();
    let mut over = Shape::new();
    let mut has_over = false;

    for c in 0..cols {
        let cell = inp.cells.get(c).unwrap_or(&blank);
        let col = c as u16;
        let selected = inp.sel.is_some_and(|(s, e)| col >= s && col < e);
        let l = look(cell, pal, selected, block && in_cursor(col));

        if l.bg != pal.bg {
            match fills.last_mut() {
                Some((_, end, rgb)) if *end == col && *rgb == l.bg => *end = col + 1,
                _ => fills.push((col, col + 1, l.bg)),
            }
        }

        let ascii = cell.width == Width::Narrow
            && cell.zerowidth.is_none()
            && (cell.ch.is_ascii_graphic() || cell.ch == ' ');
        let shown = !l.hidden && cell.width != Width::Spacer;
        if ascii && shown {
            line.push(cell.ch);
            let key = RunKey {
                fg: [l.fg.r, l.fg.g, l.fg.b, l.fg.a].map(f32::to_bits),
                bold: l.bold,
                italic: l.italic,
                underline: l.underline,
            };
            match &run {
                Some((_, k, _)) if *k == key => {}
                _ => {
                    if let Some((s, k, fg)) = run.take() {
                        spans.push(span_for(s, c, &k, fg));
                    }
                    run = Some((c, key, l.fg));
                }
            }
        } else {
            if let Some((s, k, fg)) = run.take() {
                spans.push(span_for(s, c, &k, fg));
            }
            line.push(' ');
            if shown && cell.ch != ' ' {
                let mut text = String::from(cell.ch);
                if let Some(z) = &cell.zerowidth {
                    text.push_str(z);
                }
                let w = if cell.width == Width::Wide { 2 } else { 1 };
                pinned.push((col, w, text, styled(m.style, l.bold, l.italic), l.fg));
                if l.underline {
                    over = rect(over, c as f32 * cw, ch - 2.0, w as f32 * cw, 1.0, l.fg);
                    has_over = true;
                }
            }
        }
        if l.strike && shown {
            over = rect(over, c as f32 * cw, (ch * 0.5).round(), cw, 1.0, l.fg);
            has_over = true;
        }
    }
    if let Some((s, k, fg)) = run.take() {
        spans.push(span_for(s, cols, &k, fg));
    }

    if let Some(k) = inp.cursor {
        let x = k.col as f32 * cw;
        let w = cursor_w as f32 * cw;
        let c = color(pal.cursor);
        let thin = (cw * 0.12).max(1.0);
        // A focused block is drawn as the cell's own fill and text colours above.
        if !(k.focused && k.shape == CursorShape::Block) {
            has_over = true;
            over = match (k.focused, k.shape) {
                (true, CursorShape::Line) => rect(over, x, 0.0, thin, ch, c),
                (true, _) => rect(over, x, ch - thin.max(2.0), w, thin.max(2.0), c),
                (false, _) => {
                    let o = rect(over, x, 0.0, w, 1.0, c);
                    let o = rect(o, x, ch - 1.0, w, 1.0, c);
                    let o = rect(o, x, 0.0, 1.0, ch, c);
                    rect(o, x + w - 1.0, 0.0, 1.0, ch, c)
                }
            };
        }
    }

    RowParts {
        line,
        spans,
        pinned,
        fills,
        over: has_over.then_some(over),
    }
}
