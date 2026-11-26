//! wt-vt contract stub: a plain-text screen. Handles printable UTF-8, CR/LF/BS/TAB, scrollback
//! and the few CSI sequences shells emit constantly (cursor position, erase); every other
//! escape sequence is parsed and dropped. No attributes, no colours.

use std::collections::VecDeque;

pub struct Terminal {
    cols: u16,
    rows: u16,
    lines: Vec<Vec<char>>,
    scrollback: VecDeque<Vec<char>>,
    max_scrollback: usize,
    row: u16,
    col: u16,
    utf8: Vec<u8>,
    esc: Esc,
}

#[derive(Default)]
enum Esc {
    #[default]
    Ground,
    Escape,
    Csi(String),
    Osc,
    OscEscape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// Borrowed view of the visible screen.
pub struct GridView<'a> {
    term: &'a Terminal,
}

impl GridView<'_> {
    pub fn cols(&self) -> u16 {
        self.term.cols
    }
    pub fn rows(&self) -> u16 {
        self.term.rows
    }
    /// Row `i` of the visible screen, trailing blanks trimmed.
    pub fn line_text(&self, i: u16) -> String {
        let s: String = self.term.lines[i as usize].iter().collect();
        s.trim_end().to_owned()
    }
    pub fn scrollback_len(&self) -> usize {
        self.term.scrollback.len()
    }
}

impl Terminal {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Terminal {
            cols,
            rows,
            lines: vec![vec![' '; cols as usize]; rows as usize],
            scrollback: VecDeque::new(),
            max_scrollback: scrollback,
            row: 0,
            col: 0,
            utf8: Vec::new(),
            esc: Esc::Ground,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        for l in &mut self.lines {
            l.resize(cols as usize, ' ');
        }
        while self.lines.len() > rows as usize {
            let top = self.lines.remove(0);
            self.push_scrollback(top);
            self.row = self.row.saturating_sub(1);
        }
        while self.lines.len() < rows as usize {
            self.lines.push(vec![' '; cols as usize]);
        }
        self.cols = cols;
        self.rows = rows;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols);
    }

    pub fn grid(&self) -> GridView<'_> {
        GridView { term: self }
    }

    pub fn cursor(&self) -> CursorState {
        CursorState {
            row: self.row,
            col: self.col.min(self.cols - 1),
            visible: true,
        }
    }

    fn byte(&mut self, b: u8) {
        match std::mem::take(&mut self.esc) {
            Esc::Ground => self.ground(b),
            Esc::Escape => {
                self.esc = match b {
                    b'[' => Esc::Csi(String::new()),
                    b']' => Esc::Osc,
                    _ => Esc::Ground,
                }
            }
            Esc::Csi(mut p) => {
                if (0x40..=0x7e).contains(&b) {
                    self.csi(&p, b as char);
                } else {
                    p.push(b as char);
                    self.esc = Esc::Csi(p);
                }
            }
            Esc::Osc => {
                self.esc = match b {
                    0x07 => Esc::Ground,
                    0x1b => Esc::OscEscape,
                    _ => Esc::Osc,
                }
            }
            Esc::OscEscape => self.esc = if b == b'\\' { Esc::Ground } else { Esc::Osc },
        }
    }

    fn ground(&mut self, b: u8) {
        match b {
            0x1b => self.esc = Esc::Escape,
            b'\r' => self.col = 0,
            b'\n' | 0x0b | 0x0c => self.line_feed(),
            0x08 => self.col = self.col.saturating_sub(1),
            b'\t' => self.col = ((self.col / 8 + 1) * 8).min(self.cols - 1),
            0x00..=0x1f | 0x7f => {}
            _ => {
                self.utf8.push(b);
                match std::str::from_utf8(&self.utf8) {
                    Ok(s) => {
                        let c = s.chars().next().unwrap_or(' ');
                        self.utf8.clear();
                        self.print(c);
                    }
                    Err(e) if e.error_len().is_some() || self.utf8.len() >= 4 => {
                        self.utf8.clear();
                        self.print('\u{fffd}');
                    }
                    Err(_) => {}
                }
            }
        }
    }

    fn print(&mut self, c: char) {
        if self.col >= self.cols {
            self.col = 0;
            self.line_feed();
        }
        self.lines[self.row as usize][self.col as usize] = c;
        self.col += 1;
    }

    fn line_feed(&mut self) {
        if self.row + 1 < self.rows {
            self.row += 1;
        } else {
            let top = self.lines.remove(0);
            self.push_scrollback(top);
            self.lines.push(vec![' '; self.cols as usize]);
        }
    }

    fn push_scrollback(&mut self, line: Vec<char>) {
        if self.max_scrollback == 0 {
            return;
        }
        if self.scrollback.len() == self.max_scrollback {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(line);
    }

    fn csi(&mut self, params: &str, f: char) {
        if params.starts_with(['?', '>', '<', '=']) {
            return;
        }
        let n: Vec<u16> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let arg = |i: usize, d: u16| n.get(i).copied().filter(|&v| v != 0).unwrap_or(d);
        let blank = |l: &mut [char]| l.fill(' ');
        let (row, col) = (self.row as usize, (self.col.min(self.cols - 1)) as usize);
        match f {
            'A' => self.row = self.row.saturating_sub(arg(0, 1)),
            'B' => self.row = (self.row + arg(0, 1)).min(self.rows - 1),
            'C' => self.col = (self.col + arg(0, 1)).min(self.cols - 1),
            'D' => self.col = self.col.saturating_sub(arg(0, 1)),
            'G' => self.col = (arg(0, 1) - 1).min(self.cols - 1),
            'H' | 'f' => {
                self.row = (arg(0, 1) - 1).min(self.rows - 1);
                self.col = (arg(1, 1) - 1).min(self.cols - 1);
            }
            'K' => match n[0] {
                0 => blank(&mut self.lines[row][col..]),
                1 => blank(&mut self.lines[row][..=col]),
                _ => blank(&mut self.lines[row]),
            },
            'J' => match n[0] {
                0 => {
                    blank(&mut self.lines[row][col..]);
                    self.lines[row + 1..].iter_mut().for_each(|l| blank(l));
                }
                1 => {
                    self.lines[..row].iter_mut().for_each(|l| blank(l));
                    blank(&mut self.lines[row][..=col]);
                }
                _ => self.lines.iter_mut().for_each(|l| blank(l)),
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(t: &Terminal) -> Vec<String> {
        (0..t.grid().rows())
            .map(|i| t.grid().line_text(i))
            .collect()
    }

    #[test]
    fn prints_wraps_scrolls_and_drops_escapes() {
        let mut t = Terminal::new(5, 2, 10);
        t.feed(b"\x1b[1;32mab\x1b[0m\r\n\x1b]0;title\x07h\xc3");
        t.feed(b"\xa9llo!");
        assert_eq!(screen(&t), ["héllo", "!"]);
        assert_eq!(t.grid().scrollback_len(), 1);
        t.feed(b"\x1b[2J\x1b[2;3Hx");
        assert_eq!(screen(&t), ["", "  x"]);
        assert_eq!(
            t.cursor(),
            CursorState {
                row: 1,
                col: 3,
                visible: true
            }
        );
    }

    #[test]
    fn resize_keeps_bottom_rows() {
        let mut t = Terminal::new(4, 3, 10);
        t.feed(b"1\r\n2\r\n3");
        t.resize(2, 2);
        assert_eq!(screen(&t), ["2", "3"]);
        assert_eq!(t.cursor().row, 1);
    }
}
