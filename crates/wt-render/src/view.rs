use std::cell::{Cell as Slot, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rune::{El, GridRow, TextStyle, el, grid};

use crate::model::{Cell, Selection, TermSource};
use crate::palette::{Palette, color};
use crate::row::{Metrics, RowCursor, RowInput, build};

struct CachedRow {
    hash: u64,
    row: Rc<GridRow>,
}

/// Which visible rows the last `view` built afresh; the rest reused last frame's `GridRow`, and
/// with it everything Rune laid out and drew for that row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Damage {
    pub rebuilt: Vec<u16>,
    pub rows: u16,
}

/// A Rune component that draws a terminal grid: call [`TerminalView::view`] from a view function.
pub struct TerminalView {
    palette: Palette,
    font_size: f32,
    cell: Slot<Option<(f32, f32)>>,
    focused: bool,
    config: Slot<u64>,
    rows: RefCell<Vec<CachedRow>>,
    damage: RefCell<Damage>,
    scratch: RefCell<Vec<Cell>>,
}

impl Default for TerminalView {
    fn default() -> TerminalView {
        TerminalView::new(Palette::default(), 14.0)
    }
}

impl TerminalView {
    pub fn new(palette: Palette, font_size: f32) -> TerminalView {
        let v = TerminalView {
            palette,
            font_size,
            cell: Slot::new(None),
            focused: true,
            config: Slot::new(0),
            rows: RefCell::new(Vec::new()),
            damage: RefCell::new(Damage::default()),
            scratch: RefCell::new(Vec::new()),
        };
        v.rehash_config();
        v
    }

    /// Fixes the cell size in points instead of measuring the monospace font.
    pub fn cell_size(self, w: f32, h: f32) -> TerminalView {
        self.cell.set(Some((w, h)));
        self.rehash_config();
        self
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.rehash_config();
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Unfocused terminals draw their cursor as a hollow box.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn metrics(&self) -> Metrics {
        let style = TextStyle::new(self.font_size, 400.0).mono();
        let (w, h) = self.cell.get().unwrap_or_else(|| {
            let w = rune::text::advance("0000000000", style) / 10.0;
            let h = (self.font_size * 1.25).ceil();
            let w = if w > 0.0 { w } else { self.font_size * 0.6 };
            self.cell.set(Some((w, h)));
            self.rehash_config();
            (w, h)
        });
        Metrics {
            style: style.lh(h),
            cell_w: w,
            cell_h: h,
        }
    }

    /// `(cols, rows)` that fit in `size` points: what to `resize` the PTY and terminal to.
    pub fn grid_size_for(&self, size: (f32, f32)) -> (u16, u16) {
        let m = self.metrics();
        let fit = |avail: f32, cell: f32| {
            ((avail / cell).floor() as i64).clamp(1, u16::MAX as i64) as u16
        };
        (fit(size.0, m.cell_w), fit(size.1, m.cell_h))
    }

    pub fn last_damage(&self) -> Damage {
        self.damage.borrow().clone()
    }

    fn rehash_config(&self) {
        let mut h = DefaultHasher::new();
        self.palette.hash(&mut h);
        self.font_size.to_bits().hash(&mut h);
        if let Some((w, ch)) = self.cell.get() {
            (w.to_bits(), ch.to_bits()).hash(&mut h);
        }
        self.config.set(h.finish());
    }

    /// The terminal drawn with `scroll` lines of scrollback showing above the screen (0 follows
    /// the output) and `sel` highlighted. Rows whose content, selection and cursor are as last
    /// frame reuse last frame's row, so Rune draws them from its cache.
    pub fn view<T: TermSource + ?Sized>(&self, term: &T, sel: Selection, scroll: usize) -> El {
        let m = self.metrics();
        let (cols, rows) = term.size();
        let scroll = scroll.min(term.history()) as i32;
        let cursor = term.cursor();
        let mut cache = self.rows.borrow_mut();
        cache.truncate(rows as usize);
        let mut cells = self.scratch.borrow_mut();
        let mut damage = Damage {
            rebuilt: Vec::new(),
            rows,
        };
        let mut out = Vec::with_capacity(rows as usize);

        for r in 0..rows {
            let line = r as i32 - scroll;
            cells.clear();
            term.line(line, &mut cells);
            let row_cursor = (cursor.visible && line == cursor.row as i32 && cursor.col < cols)
                .then_some(RowCursor {
                    col: cursor.col,
                    shape: cursor.shape,
                    focused: self.focused,
                });
            let inp = RowInput {
                cells: &cells[..cells.len().min(cols as usize)],
                cols,
                sel: sel.cols_on(line, cols),
                cursor: row_cursor,
            };
            let mut h = DefaultHasher::new();
            (self.config.get(), &inp).hash(&mut h);
            let hash = h.finish();
            let i = r as usize;
            match cache.get(i) {
                Some(c) if c.hash == hash => out.push(c.row.clone()),
                _ => {
                    let row = Rc::new(build(&inp, &self.palette, &m));
                    damage.rebuilt.push(r);
                    let fresh = CachedRow {
                        hash,
                        row: row.clone(),
                    };
                    if i < cache.len() {
                        cache[i] = fresh
                    } else {
                        cache.push(fresh)
                    }
                    out.push(row);
                }
            }
        }
        *self.damage.borrow_mut() = damage;

        let (w, h) = (cols as f32 * m.cell_w, rows as f32 * m.cell_h);
        let mut g = grid(cols, m.cell_w, m.cell_h, out).w(w).h(h);
        let caret_row = cursor.row as i32 + scroll;
        if cursor.visible && caret_row < rows as i32 {
            g = g.grid_caret(cursor.col.min(cols.saturating_sub(1)), caret_row as u16);
        }
        el().w(w).h(h).fill(color(self.palette.bg)).child(g)
    }
}
