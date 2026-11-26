//! Headless checks through Rune: element tree for tabs + splits + fake terminal content, key
//! dispatch through the runtime, and a PNG render of the split scene.
use wt_app::rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer, write_png};
use wt_app::rune::{App, Env, Key, KeyEvent, Mods, Runtime};
use wt_app::{Action, SCENES, State, spec};

const SIZE: (f32, f32) = (1100.0, 680.0);

fn runtime(scene: &str) -> Runtime<State> {
    let mut rt = App::new(spec()).headless_runtime(SIZE, Env::new(true, 1.0));
    let f = SCENES.iter().find(|(n, _)| *n == scene).unwrap().1;
    rt.update(|s, _| f(s));
    rt.frame(0.0);
    rt.frame(0.0);
    rt
}

fn keys(rt: &Runtime<State>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.key).collect()
}

fn texts(rt: &Runtime<State>) -> String {
    rt.elements()
        .into_iter()
        .filter_map(|e| e.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn state<T>(rt: &mut Runtime<State>, f: impl FnOnce(&State) -> T) -> T {
    let mut out = None;
    rt.update(|s, _| out = Some(f(s)));
    out.unwrap()
}

fn chord(key: Key, shift: bool) -> KeyEvent {
    KeyEvent {
        key,
        mods: Mods {
            ctrl: true,
            cmd: true,
            shift,
            ..Mods::default()
        },
    }
}

#[test]
fn tabs_and_split_with_fake_terminal_render() {
    let mut rt = runtime("split");
    let k = keys(&rt);
    let (tabs, panes) = state(&mut rt, |s| {
        (
            s.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            s.focused_pane(),
        )
    });
    assert_eq!(tabs.len(), 3);
    for id in &tabs {
        assert!(
            k.contains(&format!("tab-{id}")),
            "missing tab-{id} in {k:?}"
        );
        assert!(k.contains(&format!("tab-{id}-close")));
    }
    assert!(k.contains(&"new-tab".to_string()));
    let pane_keys: Vec<_> = k.iter().filter(|k| k.starts_with("pane-")).collect();
    assert_eq!(pane_keys.len(), 3, "{pane_keys:?}");
    assert_eq!(k.iter().filter(|k| k.starts_with("divider-")).count(), 2);
    assert!(panes.is_some());
    let t = texts(&rt);
    assert!(t.contains("wt-linux fake terminal (Bash)"), "{t}");
    assert!(t.contains("cargo build -p wt-app"));
    assert!(t.contains("build"), "renamed tab title");

    // Panes share the content area side by side, then stacked on the right.
    let rect = |key: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap()
            .rect
    };
    let [a, b, c] = [pane_keys[0], pane_keys[1], pane_keys[2]].map(|k| rect(k));
    assert!(
        a[0] < b[0] && (b[0] - c[0]).abs() < 1.0 && b[1] < c[1],
        "{a:?} {b:?} {c:?}"
    );
}

#[test]
fn keymap_drives_the_runtime() {
    let mut rt = runtime("tabs");
    assert_eq!(state(&mut rt, |s| s.tabs.len()), 3);
    rt.key(chord(Key::Command('t'), true));
    rt.frame(0.0);
    assert_eq!(state(&mut rt, |s| (s.tabs.len(), s.active)), (4, 3));
    rt.key(chord(Key::Tab, false));
    rt.frame(0.0);
    assert_eq!(state(&mut rt, |s| s.active), 0);
    rt.key(KeyEvent {
        key: Key::Text("=".into()),
        mods: Mods {
            alt: true,
            shift: true,
            ..Mods::default()
        },
    });
    rt.update(|s, _| {
        s.dispatch(Action::SplitVertical);
    });
    rt.frame(0.0);
    assert!(keys(&rt).iter().any(|k| k.starts_with("divider-")));
    // Typing goes to the focused fake terminal.
    for c in "ls".chars() {
        rt.key(KeyEvent {
            key: Key::Text(c.to_string()),
            mods: Mods::default(),
        });
    }
    rt.frame(0.0);
    assert!(texts(&rt).contains("$ ls\n") || texts(&rt).lines().any(|l| l.ends_with("$ ls")));
    let before = state(&mut rt, |s| s.font_size(0));
    rt.key(chord(Key::Command('='), false));
    rt.frame(0.0);
    assert_eq!(state(&mut rt, |s| s.font_size(0)), before + 1.0);
}

#[test]
fn palette_filters_and_runs() {
    let mut rt = runtime("palette");
    let k = keys(&rt);
    assert!(k.contains(&"palette".to_string()) && k.contains(&"palette-input".to_string()));
    let t = texts(&rt);
    assert!(t.contains("Split pane vertically") && !t.contains("Close tab\n"));
    let panes = state(&mut rt, |s| s.panes.len());
    rt.update(|s, tx| wt_app::app::apply(s.palette_run(Some(0)), tx));
    rt.frame(0.0);
    assert_eq!(state(&mut rt, |s| s.panes.len()), panes + 1);
    assert!(!keys(&rt).contains(&"palette".to_string()));
}

#[test]
fn settings_tab_renders() {
    let rt = runtime("settings");
    let k = keys(&rt);
    assert!(k.contains(&"settings".to_string()));
    assert!(k.iter().any(|k| k.starts_with("profile-card-")));
    assert!(texts(&rt).contains("Background opacity"));
}

#[test]
fn split_scene_renders_to_png() {
    let Some(gpu) = Gpu::test_headless() else {
        return;
    };
    let mut rt = runtime("split");
    let (w, h) = (SIZE.0 as u32, SIZE.1 as u32);
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    let target = Offscreen::new(&gpu.device, w, h);
    renderer.render_runtime_to_offscreen(&gpu.device, &gpu.queue, &target, &mut rt);
    let rgba = target.read(&gpu.device);
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    // Green prompt ink ("user@wt-linux") must be present.
    let green = rgba
        .chunks(4)
        .filter(|p| p[1] > 120 && p[0] < 80 && p[2] < 80)
        .count();
    assert!(green > 50, "expected prompt ink, found {green}");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("wt-app-split.png");
    write_png(&out, w, h, &rgba).unwrap();
}
