//! A fake terminal for tests and for building on `TerminalView` before `wt-vt` lands.

use crate::model::{Cell, CursorState, TermSource, Width};

pub struct MockTerminal {
    pub cols: u16,
    pub rows: u16,
    pub screen: Vec<Vec<Cell>>,
    /// Oldest first; the last entry is line -1.
    pub history: Vec<Vec<Cell>>,
    pub cursor: CursorState,
}

impl MockTerminal {
    pub fn new(cols: u16, rows: u16) -> MockTerminal {
        MockTerminal {
            cols,
            rows,
            screen: vec![vec![Cell::default(); cols as usize]; rows as usize],
            history: Vec::new(),
            cursor: CursorState {
                visible: true,
                ..CursorState::default()
            },
        }
    }

    /// Writes `text` from (`col`, `row`) with `proto`'s colours and attributes; wide characters
    /// take two cells.
    pub fn write(&mut self, col: u16, row: u16, text: &str, proto: &Cell) {
        let mut c = col as usize;
        for ch in text.chars() {
            let wide = is_wide(ch);
            let Some(line) = self.screen.get_mut(row as usize) else {
                return;
            };
            if c + usize::from(wide) >= line.len() {
                return;
            }
            line[c] = Cell {
                ch,
                width: if wide { Width::Wide } else { Width::Narrow },
                ..proto.clone()
            };
            if wide {
                line[c + 1] = Cell {
                    ch: ' ',
                    width: Width::Spacer,
                    ..proto.clone()
                };
            }
            c += 1 + usize::from(wide);
        }
    }

    pub fn set(&mut self, col: u16, row: u16, cell: Cell) {
        self.screen[row as usize][col as usize] = cell;
    }

    pub fn push_history(&mut self, text: &str) {
        let mut line = vec![Cell::default(); self.cols as usize];
        for (cell, ch) in line.iter_mut().zip(text.chars()) {
            cell.ch = ch;
        }
        self.history.push(line);
    }
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32, 0x1100..=0x115f | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f300..=0x1faff | 0x20000..=0x3fffd)
}

impl TermSource for MockTerminal {
    fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn history(&self) -> usize {
        self.history.len()
    }

    fn line(&self, line: i32, out: &mut Vec<Cell>) {
        let src = if line >= 0 {
            self.screen.get(line as usize)
        } else {
            self.history
                .len()
                .checked_sub((-line) as usize)
                .and_then(|i| self.history.get(i))
        };
        if let Some(src) = src {
            out.extend_from_slice(src);
        }
    }

    fn cursor(&self) -> CursorState {
        self.cursor
    }
}
