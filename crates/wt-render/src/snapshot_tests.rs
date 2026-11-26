//! Renders a fake grid through Rune's real GPU pipeline (offscreen, no display) to PNG.

use std::path::PathBuf;

use rune::Runtime;
use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer, write_png};

use crate::tests::{Screen, runtime, sample};
use crate::*;

const W: u32 = 320;
const H: u32 = 144;

/// One renderer across frames, as a window keeps: it holds the glyph atlas pages reused rows
/// point into.
fn render(gpu: &Gpu, renderer: &mut Renderer, rt: &mut Runtime<Screen>) -> Vec<u8> {
    let target = Offscreen::new(&gpu.device, W, H);
    renderer.render_runtime_to_offscreen(&gpu.device, &gpu.queue, &target, rt);
    target.read(&gpu.device)
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/wt-render-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Pixels that differ inside the cell box (`col`, `row`), elsewhere on its row (antialiased glyph
/// edges may cross into a neighbour) and on other rows, for 8×18 cells.
fn diff_in_cell(a: &[u8], b: &[u8], col: u32, row: u32) -> (usize, usize, usize) {
    let (mut inside, mut beside, mut outside) = (0, 0, 0);
    for (i, (pa, pb)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
        if pa == pb {
            continue;
        }
        let (x, y) = (i as u32 % W, i as u32 / W);
        if x / 8 == col && y / 18 == row {
            inside += 1
        } else if y / 18 == row && x / 8 + 1 >= col && x / 8 <= col + 1 {
            beside += 1
        } else {
            outside += 1
        }
    }
    (inside, beside, outside)
}

fn distinct_colors(px: &[u8]) -> usize {
    let mut set: Vec<[u8; 4]> = px.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
    set.sort_unstable();
    set.dedup();
    set.len()
}

#[test]
fn renders_to_png_and_a_changed_cell_changes_only_its_pixels() {
    let Some(gpu) = Gpu::test_headless() else {
        return;
    };
    let mut rt = runtime(sample());
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    rt.frame(0.0);
    let before = render(&gpu, &mut renderer, &mut rt);
    assert_eq!(before.len(), (W * H * 4) as usize);
    assert!(
        distinct_colors(&before) > 50,
        "text, fills and the cursor make a busy image"
    );
    let first = &before[..4];
    assert!(
        before.chunks(4).filter(|p| *p != first).count() > 2000,
        "more than a blank screen"
    );

    // Cell (3, 1) is the 'o' in "error": make it a green-on-red 'X'.
    rt.update(|s, _| {
        s.term.set(
            3,
            1,
            Cell::new('X')
                .fg(VtColor::Indexed(10))
                .bg(VtColor::Indexed(1)),
        )
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.view.last_damage().rebuilt, vec![1]);
    let after = render(&gpu, &mut renderer, &mut rt);
    let (inside, beside, outside) = diff_in_cell(&before, &after, 3, 1);
    assert!(inside > 50, "the cell's pixels changed ({inside})");
    assert!(
        beside < inside / 4,
        "at most glyph edges bleed into neighbours ({beside})"
    );
    assert_eq!(
        outside, 0,
        "nothing beyond the cell and its neighbours changed"
    );

    let dir = out_dir();
    write_png(&dir.join("terminal.png"), W, H, &before).unwrap();
    write_png(&dir.join("terminal-changed.png"), W, H, &after).unwrap();
    let tree: String = rt
        .elements()
        .iter()
        .map(|e| format!("{:<12} {:<10} {:?} {:?}\n", e.path, e.role, e.rect, e.text))
        .collect();
    std::fs::write(dir.join("terminal.elements.txt"), &tree).unwrap();
    std::fs::write(
        dir.join("terminal.a11y.json"),
        rt.accessibility_json().to_string(),
    )
    .unwrap();
    if std::env::var_os("WT_UPDATE_SNAPSHOTS").is_some() {
        let keep = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("snapshots");
        std::fs::create_dir_all(&keep).unwrap();
        write_png(&keep.join("terminal.png"), W, H, &before).unwrap();
        std::fs::write(keep.join("terminal.elements.txt"), tree).unwrap();
    }
}

#[test]
fn cursor_shapes_selection_and_scroll_draw_differently() {
    let Some(gpu) = Gpu::test_headless() else {
        return;
    };
    let mut rt = runtime(sample());
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    let mut shots = vec![render(&gpu, &mut renderer, &mut rt)];
    let edits: [fn(&mut Screen); 5] = [
        |s| s.term.cursor.shape = CursorShape::Line,
        |s| s.term.cursor.shape = CursorShape::Underline,
        |s| s.term.cursor.visible = false,
        |s| {
            s.sel = Selection::Range {
                start: Point::new(0, 0),
                end: Point::new(1, 10),
            }
        },
        |s| s.scroll = 3,
    ];
    for edit in edits {
        rt.update(|s, _| edit(s));
        rt.frame(1.0 / 60.0);
        let shot = render(&gpu, &mut renderer, &mut rt);
        assert_ne!(&shot, shots.last().unwrap());
        shots.push(shot);
    }
    let dir = out_dir();
    for (i, s) in shots.iter().enumerate() {
        write_png(&dir.join(format!("state-{i}.png")), W, H, s).unwrap();
    }
}
