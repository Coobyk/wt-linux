//! What a pane needs from a live terminal, and `FakeTerminal`, a canned implementation so the
//! shell runs and renders headless before wt-backend/wt-vt land.
//!
//! `Cell`, `CellColor`, `Attrs`, `CursorState` and `GridSnapshot` mirror the wt-vt contract in
//! docs/ARCHITECTURE.md (char, fg/bg as palette index or RGB, attrs, wide flag). Once wt-vt
//! exports `GridView`/`CursorState`, `PaneTerminal` gets an impl over `wt_backend::Pty` +
//! `wt_vt::Terminal` in the `wt` crate and these mirrors become re-exports.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CellColor {
    #[default]
    Default,
    /// xterm 256-colour palette index.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Attrs(pub u8);

impl Attrs {
    pub const BOLD: Attrs = Attrs(1);
    pub const ITALIC: Attrs = Attrs(2);
    pub const UNDERLINE: Attrs = Attrs(4);
    pub const STRIKETHROUGH: Attrs = Attrs(8);
    pub const INVERSE: Attrs = Attrs(16);
    pub const DIM: Attrs = Attrs(32);
    pub const BLINK: Attrs = Attrs(64);

    pub fn has(self, a: Attrs) -> bool {
        self.0 & a.0 == a.0 && a.0 != 0
    }
}

impl std::ops::BitOr for Attrs {
    type Output = Attrs;
    fn bitor(self, o: Attrs) -> Attrs {
        Attrs(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub attrs: Attrs,
    /// First half of a double-width character; the next cell is its spacer.
    pub wide: bool,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            ch: ' ',
            fg: CellColor::Default,
            bg: CellColor::Default,
            attrs: Attrs::default(),
            wide: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// An owned copy of the visible grid, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct GridSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
}

impl GridSnapshot {
    pub fn new(cols: u16, rows: u16) -> GridSnapshot {
        GridSnapshot {
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
        }
    }

    pub fn row(&self, r: u16) -> &[Cell] {
        let c = self.cols as usize;
        &self.cells[r as usize * c..(r as usize + 1) * c]
    }

    /// Row `r` as plain text, trailing blanks trimmed.
    pub fn row_text(&self, r: u16) -> String {
        self.row(r)
            .iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// One pane's terminal session: PTY + VT state behind one object.
pub trait PaneTerminal {
    fn grid(&self) -> &GridSnapshot;
    fn cursor(&self) -> CursorState;
    /// Bytes for the child (already encoded keystrokes, pastes).
    fn write(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    /// Drains pending child output into the grid; true if the grid changed.
    fn poll(&mut self) -> bool {
        false
    }
    /// The title the child set (OSC 0/2), if any.
    fn title(&self) -> Option<String> {
        None
    }
    fn exited(&self) -> bool {
        false
    }
}

/// What a pane is spawned from: the fields of `wt_backend::PtySpec` resolved from a profile.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    /// Profile name, for fakes and diagnostics.
    pub profile: String,
}

/// Creates a pane's terminal. The `wt` binary installs one backed by wt-backend + wt-vt.
pub type TerminalFactory = Box<dyn Fn(&SpawnSpec) -> Box<dyn PaneTerminal>>;

pub fn fake_factory() -> TerminalFactory {
    Box::new(|spec| Box::new(FakeTerminal::new(spec)))
}

/// A canned shell: a banner, a coloured `ls`, and a prompt that echoes typed keys. Enter
/// "runs" the line by printing a note that no PTY is attached.
pub struct FakeTerminal {
    grid: GridSnapshot,
    cursor: CursorState,
    line: String,
    prompt: String,
}

const GREEN: CellColor = CellColor::Indexed(10);
const BLUE: CellColor = CellColor::Indexed(12);
const CYAN: CellColor = CellColor::Indexed(14);
const YELLOW: CellColor = CellColor::Indexed(11);
const GREY: CellColor = CellColor::Indexed(8);

impl FakeTerminal {
    pub fn new(spec: &SpawnSpec) -> FakeTerminal {
        let mut t = FakeTerminal {
            grid: GridSnapshot::new(spec.cols.max(1), spec.rows.max(1)),
            cursor: CursorState {
                col: 0,
                row: 0,
                visible: true,
            },
            line: String::new(),
            prompt: "user@wt-linux".into(),
        };
        let cmd = std::iter::once(spec.program.as_str())
            .chain(spec.args.iter().map(String::as_str))
            .collect::<Vec<_>>();
        t.print(
            &format!("wt-linux fake terminal ({})", spec.profile),
            YELLOW,
            Attrs::BOLD,
        );
        t.newline();
        t.print(
            &format!("would spawn: {}", cmd.join(" ")),
            GREY,
            Attrs::ITALIC,
        );
        t.newline();
        t.prompt_line();
        t.print("ls --color", CellColor::Default, Attrs::default());
        t.newline();
        for (name, color) in [
            ("Cargo.toml", CellColor::Default),
            ("crates/", BLUE),
            ("docs/", BLUE),
            ("run.sh", GREEN),
            ("README.md", CellColor::Default),
        ] {
            t.print(
                name,
                color,
                if color == BLUE {
                    Attrs::BOLD
                } else {
                    Attrs::default()
                },
            );
            t.print("  ", CellColor::Default, Attrs::default());
        }
        t.newline();
        t.prompt_line();
        t
    }

    /// Typed-but-not-entered input on the prompt line.
    pub fn pending_line(&self) -> &str {
        &self.line
    }

    fn prompt_line(&mut self) {
        let p = self.prompt.clone();
        self.print(&p, GREEN, Attrs::BOLD);
        self.print(":", CellColor::Default, Attrs::default());
        self.print("~", BLUE, Attrs::BOLD);
        self.print("$ ", CellColor::Default, Attrs::default());
    }

    fn print(&mut self, s: &str, fg: CellColor, attrs: Attrs) {
        for ch in s.chars() {
            if self.cursor.col >= self.grid.cols {
                self.newline();
            }
            let at = self.cursor.row as usize * self.grid.cols as usize + self.cursor.col as usize;
            self.grid.cells[at] = Cell {
                ch,
                fg,
                bg: CellColor::Default,
                attrs,
                wide: false,
            };
            self.cursor.col += 1;
        }
    }

    fn newline(&mut self) {
        self.cursor.col = 0;
        if self.cursor.row + 1 < self.grid.rows {
            self.cursor.row += 1;
        } else {
            let c = self.grid.cols as usize;
            self.grid.cells.drain(..c);
            self.grid
                .cells
                .extend(std::iter::repeat_n(Cell::default(), c));
        }
    }

    fn backspace(&mut self) {
        if self.line.pop().is_some() {
            if self.cursor.col > 0 {
                self.cursor.col -= 1;
            } else if self.cursor.row > 0 {
                self.cursor.row -= 1;
                self.cursor.col = self.grid.cols - 1;
            } else {
                return;
            }
            let at = self.cursor.row as usize * self.grid.cols as usize + self.cursor.col as usize;
            self.grid.cells[at] = Cell::default();
        }
    }

    fn enter(&mut self) {
        let line = std::mem::take(&mut self.line);
        self.newline();
        let cmd = line.split_whitespace().next().unwrap_or("");
        if !cmd.is_empty() {
            self.print(
                &format!("{cmd}: fake terminal, no PTY attached"),
                CYAN,
                Attrs::default(),
            );
            self.newline();
        }
        self.prompt_line();
    }
}

impl PaneTerminal for FakeTerminal {
    fn grid(&self) -> &GridSnapshot {
        &self.grid
    }

    fn cursor(&self) -> CursorState {
        self.cursor
    }

    fn write(&mut self, bytes: &[u8]) {
        let s = String::from_utf8_lossy(bytes).into_owned();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' | '\n' => self.enter(),
                '\x7f' | '\x08' => self.backspace(),
                '\x1b' => {
                    // Swallow a CSI/SS3 sequence (arrows etc.).
                    if matches!(chars.peek(), Some('[') | Some('O')) {
                        chars.next();
                        while let Some(n) = chars.next() {
                            if n.is_ascii_alphabetic() || n == '~' {
                                break;
                            }
                        }
                    }
                }
                c if !c.is_control() => {
                    self.line.push(c);
                    self.print(&c.to_string(), CellColor::Default, Attrs::default());
                }
                _ => {}
            }
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if (cols, rows) == (self.grid.cols, self.grid.rows) {
            return;
        }
        let mut g = GridSnapshot::new(cols, rows);
        // Keep the bottom rows so the prompt stays visible.
        let skip = (self.cursor.row + 1).saturating_sub(rows);
        for r in 0..rows.min(self.grid.rows - skip) {
            let src = self.grid.row(r + skip);
            let n = (cols as usize).min(src.len());
            let at = r as usize * cols as usize;
            g.cells[at..at + n].copy_from_slice(&src[..n]);
        }
        self.cursor.row -= skip;
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.grid = g;
    }

    fn title(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SpawnSpec {
        SpawnSpec {
            program: "bash".into(),
            args: vec!["-l".into()],
            cwd: None,
            env: vec![],
            cols: 40,
            rows: 8,
            profile: "Bash".into(),
        }
    }

    #[test]
    fn fake_has_content_and_echoes() {
        let mut t = FakeTerminal::new(&spec());
        assert!(t.grid().row_text(0).contains("fake terminal (Bash)"));
        assert!(t.grid().row_text(1).contains("bash -l"));
        let row = t.cursor().row;
        t.write(b"echo hi");
        assert_eq!(t.pending_line(), "echo hi");
        assert!(t.grid().row_text(row).ends_with("$ echo hi"));
        t.write(b"\x7f\x7f\x1b[A");
        assert_eq!(t.pending_line(), "echo ");
        t.write(b"\r");
        assert!(
            t.grid()
                .row_text(t.cursor().row - 1)
                .contains("echo: fake terminal")
        );
    }

    #[test]
    fn scrolls_and_resizes() {
        let mut t = FakeTerminal::new(&spec());
        for _ in 0..20 {
            t.write(b"x\r");
        }
        assert_eq!(t.cursor().row, 7);
        t.resize(20, 3);
        assert_eq!((t.grid().cols, t.grid().rows), (20, 3));
        assert_eq!(t.cursor().row, 2);
        assert!(t.grid().row_text(2).starts_with("user@wt-linux"));
    }

    #[test]
    fn backspace_unwraps_to_the_previous_row() {
        let mut t = FakeTerminal::new(&spec());
        let row = t.cursor().row;
        let fill = 40 - t.cursor().col as usize;
        t.write("a".repeat(fill).as_bytes());
        t.write(b"b");
        assert_eq!((t.cursor().row, t.cursor().col), (row + 1, 1));
        t.write(b"\x7f\x7f");
        assert_eq!((t.cursor().row, t.cursor().col), (row, 39));
        assert_eq!(t.grid().row_text(row).trim_end().chars().count(), 39);
        assert!(t.grid().row_text(row + 1).trim().is_empty());
        assert_eq!(t.pending_line(), "a".repeat(fill - 1));
    }
}
