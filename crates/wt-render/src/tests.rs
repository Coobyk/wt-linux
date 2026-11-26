use std::rc::Rc;

use rune::{Cx, El, Env, Runtime};

use crate::mock::MockTerminal;
use crate::*;

pub(crate) struct Screen {
    pub term: MockTerminal,
    pub view: Rc<TerminalView>,
    pub sel: Selection,
    pub scroll: usize,
}

pub(crate) fn screen_view(s: &Screen, _: &mut Cx<Screen>) -> El {
    s.view.view(&s.term, s.sel, s.scroll)
}

pub(crate) fn sample() -> MockTerminal {
    let mut t = MockTerminal::new(40, 8);
    let red = Cell::default().fg(VtColor::Indexed(1));
    let on_blue = Cell::default()
        .fg(VtColor::Indexed(15))
        .bg(VtColor::Indexed(4));
    t.write(
        0,
        0,
        "user@wt-linux:~$ ls -la",
        &Cell::default().fg(VtColor::Indexed(10)),
    );
    t.write(0, 1, "error: something went wrong", &red);
    t.write(0, 2, " INVERSE ", &Cell::default().attrs(Attrs::INVERSE));
    t.write(
        10,
        2,
        " bold ",
        &Cell::default().attrs(Attrs::BOLD | Attrs::UNDERLINE),
    );
    t.write(0, 3, "status bar", &on_blue);
    t.write(0, 4, "wide: 日本語 ─│┼ é", &Cell::default());
    t.write(
        0,
        5,
        "truecolor",
        &Cell::default()
            .fg(VtColor::Rgb(255, 128, 0))
            .attrs(Attrs::ITALIC | Attrs::STRIKE),
    );
    t.cursor = CursorState {
        col: 17,
        row: 0,
        shape: CursorShape::Block,
        visible: true,
    };
    for k in 0..5 {
        t.push_history(&format!("history line {k}"));
    }
    t
}

pub(crate) fn runtime(term: MockTerminal) -> Runtime<Screen> {
    let view = Rc::new(TerminalView::default().cell_size(8.0, 18.0));
    let s = Screen {
        term,
        view,
        sel: Selection::None,
        scroll: 0,
    };
    let mut rt = Runtime::new(s, screen_view, (320.0, 144.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}

fn damage(rt: &Runtime<Screen>) -> Vec<u16> {
    rt.state.view.last_damage().rebuilt
}

fn step(rt: &mut Runtime<Screen>, f: impl FnOnce(&mut Screen)) -> Vec<u16> {
    rt.update(|s, _| f(s));
    rt.frame(1.0 / 60.0);
    damage(rt)
}

#[test]
fn first_frame_builds_every_row() {
    let rt = runtime(sample());
    assert_eq!(damage(&rt), (0..8).collect::<Vec<u16>>());
}

#[test]
fn unchanged_frame_rebuilds_nothing_and_draws_the_same() {
    let mut rt = runtime(sample());
    let first = format!("{:?}", rt.instances);
    rt.invalidate();
    rt.frame(1.0 / 60.0);
    assert_eq!(damage(&rt), Vec::<u16>::new());
    assert_eq!(format!("{:?}", rt.instances), first);
}

#[test]
fn one_changed_cell_rebuilds_only_its_row() {
    let mut rt = runtime(sample());
    assert_eq!(step(&mut rt, |s| s.term.set(3, 5, Cell::new('X'))), vec![5]);
}

#[test]
fn cursor_move_rebuilds_old_and_new_rows() {
    let mut rt = runtime(sample());
    assert_eq!(step(&mut rt, |s| s.term.cursor.row = 2), vec![0, 2]);
    assert_eq!(
        step(&mut rt, |s| s.term.cursor.shape = CursorShape::Line),
        vec![2]
    );
    assert_eq!(step(&mut rt, |s| s.term.cursor.visible = false), vec![2]);
}

#[test]
fn selection_rebuilds_the_rows_it_covers() {
    let mut rt = runtime(sample());
    let sel = Selection::Range {
        start: Point::new(3, 4),
        end: Point::new(1, 2),
    };
    assert_eq!(step(&mut rt, |s| s.sel = sel), vec![1, 2, 3]);
    assert_eq!(step(&mut rt, |s| s.sel = Selection::None), vec![1, 2, 3]);
}

#[test]
fn scrolling_shows_history_above_the_screen() {
    let mut rt = runtime(sample());
    assert_eq!(step(&mut rt, |s| s.scroll = 2).len(), 8);
    let mut cells = Vec::new();
    rt.state.term.line(-2, &mut cells);
    assert_eq!(
        cells.iter().map(|c| c.ch).collect::<String>().trim_end(),
        "history line 3"
    );
    // Scrolled past the history clamps to it.
    let clamped = step(&mut rt, |s| s.scroll = 99);
    assert_eq!(clamped.len(), 8);
    let at_top = format!("{:?}", rt.instances);
    step(&mut rt, |s| s.scroll = 5);
    assert_eq!(format!("{:?}", rt.instances), at_top);
}

#[test]
fn palette_change_rebuilds_everything() {
    let mut rt = runtime(sample());
    let mut view = TerminalView::default().cell_size(8.0, 18.0);
    view.set_palette(Palette {
        bg: [0, 0, 0x40],
        ..Palette::default()
    });
    assert_eq!(step(&mut rt, |s| s.view = Rc::new(view)).len(), 8);
}

#[test]
fn selection_columns() {
    let r = Selection::Range {
        start: Point::new(2, 5),
        end: Point::new(0, 3),
    };
    assert_eq!(r.cols_on(0, 10), Some((3, 10)));
    assert_eq!(r.cols_on(1, 10), Some((0, 10)));
    assert_eq!(r.cols_on(2, 10), Some((0, 6)));
    assert_eq!(r.cols_on(3, 10), None);
    let b = Selection::Block {
        start: Point::new(-1, 7),
        end: Point::new(1, 2),
    };
    assert_eq!(b.cols_on(0, 10), Some((2, 8)));
    assert_eq!(b.cols_on(-2, 10), None);
}

#[test]
fn palette_indexes_xterm_256() {
    let p = Palette::default();
    assert_eq!(p.indexed(16), [0, 0, 0]);
    assert_eq!(p.indexed(231), [255, 255, 255]);
    assert_eq!(p.indexed(232), [8, 8, 8]);
    assert_eq!(p.indexed(196), [255, 0, 0]);
}

#[test]
fn grid_size_fits_cells() {
    let v = TerminalView::default().cell_size(8.0, 18.0);
    assert_eq!(v.grid_size_for((800.0, 600.0)), (100, 33));
    assert_eq!(v.grid_size_for((1.0, 1.0)), (1, 1));
}

#[test]
fn measured_cells_are_positive() {
    let m = TerminalView::default().metrics();
    assert!(m.cell_w > 4.0 && m.cell_w < 14.0, "{m:?}");
    assert_eq!(m.cell_h, 18.0);
}

#[test]
fn element_tree_has_the_terminal() {
    let rt = runtime(sample());
    let els = rt.elements();
    let dump: String = els
        .iter()
        .map(|e| format!("{} {} {:?} {:?}\n", e.path, e.role, e.rect, e.text))
        .collect();
    assert!(dump.contains("terminal"), "{dump}");
    let a11y = rt.accessibility_json().to_string();
    assert!(a11y.contains(r#""role":"terminal""#), "{a11y}");
}
