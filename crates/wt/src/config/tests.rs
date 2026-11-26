use super::*;

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| (*v).to_owned())
    }
}

const ENV: &[(&str, &str)] = &[
    ("HOME", "/home/u"),
    ("SHELL", "/usr/bin/zsh"),
    ("EDITOR", "nvim"),
];

#[test]
fn empty_file_is_builtin_defaults() {
    assert_eq!(Config::parse("").unwrap(), Config::default());
}

#[test]
fn default_file_round_trips_and_validates() {
    let text = default_file_text();
    assert!(text.starts_with("# wt-linux"));
    assert_eq!(Config::parse(&text).unwrap(), Config::default());
    assert!(Config::default().validate().unwrap().is_empty());
}

#[test]
fn file_scalars_override_defaults_and_unset_ones_keep_them() {
    let c = Config::parse("scrollback = 500\n[window]\ncols = 80\n").unwrap();
    assert_eq!(c.scrollback, 500);
    assert_eq!(c.window.cols, 80);
    assert_eq!(c.window.rows, Config::default().window.rows);
    assert_eq!(c.window.title, "wt-linux");
}

#[test]
fn profile_precedence_profile_over_defaults_over_builtin() {
    let c = Config::parse(
        r##"
        default_profile = "dev"
        [defaults]
        font_size = 14.0
        color_scheme = "One Half Dark"
        env = { A = "defaults", B = "defaults" }
        [[profiles]]
        name = "dev"
        command = "$EDITOR"
        args = ["$HOME/notes"]
        cwd = "~/src"
        font_size = 16.0
        env = { B = "profile" }
    "##,
    )
    .unwrap();
    assert!(c.validate().unwrap().is_empty());
    let p = c.resolve_profile(None, &env_of(ENV)).unwrap();
    assert_eq!(p.name, "dev");
    assert_eq!(p.program, "nvim");
    assert_eq!(p.args, ["/home/u/notes"]);
    assert_eq!(p.cwd, Some(PathBuf::from("/home/u/src")));
    assert_eq!(p.font_size, 16.0, "profile beats defaults");
    assert_eq!(p.font_family, "monospace", "built-in default survives");
    assert_eq!(
        p.color_scheme.name, "One Half Dark",
        "file defaults beat built-in defaults"
    );
    let get = |k: &str| p.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("A"), Some("defaults"));
    assert_eq!(get("B"), Some("profile"));
    assert_eq!(get("TERM"), Some("xterm-256color"));
    assert_eq!(get("WT_PROFILE"), Some("dev"));
}

#[test]
fn same_named_profile_merges_into_builtin() {
    let c = Config::parse("[[profiles]]\nname = \"Shell\"\nargs = [\"-l\"]\n").unwrap();
    assert_eq!(c.profiles.len(), 1);
    let p = c.resolve_profile(Some("shell"), &env_of(ENV)).unwrap();
    assert_eq!(p.program, "/usr/bin/zsh", "no command: login shell");
    assert_eq!(p.args, ["-l"]);
}

#[test]
fn shell_falls_back_to_bin_sh() {
    let p = Config::default().resolve_profile(None, &|_| None).unwrap();
    assert_eq!(p.program, "/bin/sh");
    assert_eq!(p.cwd, None);
}

#[test]
fn color_schemes_replace_by_name_and_append() {
    let pal = (0..16).map(|_| "\"#000000\"").collect::<Vec<_>>().join(",");
    let c = Config::parse(&format!(
        r##"
        [[color_schemes]]
        name = "Campbell"
        foreground = "#111111"
        background = "#222222"
        cursor = "#333333"
        palette = [{pal}]
        [[color_schemes]]
        name = "Mine"
        foreground = "#111111"
        background = "#222222"
        cursor = "#333333"
        palette = [{pal}]
    "##
    ))
    .unwrap();
    assert_eq!(c.color_schemes.len(), 3);
    assert_eq!(c.color_scheme("Campbell").unwrap().foreground, "#111111");
    assert!(c.color_scheme("Mine").is_some());
    assert!(c.validate().unwrap().is_empty());
}

#[test]
fn keybindings_override_per_action() {
    let c = Config::parse(
        "[keybindings]\nnew_tab = \"ctrl+t\"\ncopy = \"\"\nfrobnicate = \"ctrl+q\"\n",
    )
    .unwrap();
    assert_eq!(c.keybindings["new_tab"], "ctrl+t");
    assert_eq!(
        c.keybindings["close_tab"], "ctrl+shift+w",
        "untouched actions keep defaults"
    );
    let map = c.keymap();
    assert!(!map.iter().any(|(a, _)| a == "copy"), "empty chord unbinds");
    let w = c.validate().unwrap();
    assert!(w.iter().any(|w| w.contains("frobnicate")), "{w:?}");
}

#[test]
fn validation_errors() {
    assert!(
        Config::parse("default_profile = \"nope\"")
            .unwrap()
            .validate()
            .is_err()
    );
    assert!(
        Config::parse("[window]\ncols = 0")
            .unwrap()
            .validate()
            .is_err()
    );
    assert!(
        Config::parse("[defaults]\nfont_size = -1.0")
            .unwrap()
            .validate()
            .is_err()
    );
    let bad = "[[color_schemes]]\nname=\"x\"\nforeground=\"red\"\nbackground=\"#000000\"\ncursor=\"#000000\"\npalette=[]";
    assert!(Config::parse(bad).unwrap().validate().is_err());
    assert!(
        Config::parse("scrollback = \"lots\"").is_err(),
        "type errors are parse errors"
    );
    let w = Config::parse("[defaults]\ncolor_scheme = \"Nope\"")
        .unwrap()
        .validate()
        .unwrap();
    assert!(w.iter().any(|w| w.contains("Nope")));
}

#[test]
fn unknown_profile_is_an_error() {
    let e = Config::default()
        .resolve_profile(Some("fish"), &|_| None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("fish") && e.contains("Shell"), "{e}");
}

#[test]
fn command_override_beats_profile() {
    let mut p = Config::default()
        .resolve_profile(None, &env_of(ENV))
        .unwrap();
    command_override(&mut p, &["htop".into(), "-d".into(), "5".into()]);
    assert_eq!(
        (p.program.as_str(), p.args.clone()),
        ("htop", vec!["-d".to_owned(), "5".to_owned()])
    );
    command_override(&mut p, &["ls -la | head".into()]);
    assert_eq!(p.program, "/bin/sh");
    assert_eq!(p.args, ["-c", "ls -la | head"]);
    command_override(&mut p, &[]);
    assert_eq!(p.program, "/bin/sh", "empty override changes nothing");
}

#[test]
fn var_expansion() {
    let env = env_of(ENV);
    assert_eq!(expand_vars("$HOME/x", &env), "/home/u/x");
    assert_eq!(expand_vars("${EDITOR}rc", &env), "nvimrc");
    assert_eq!(expand_vars("$NOPE-$$-$", &env), "-$-$");
}

#[test]
fn xdg_path() {
    let p = |x: Option<&str>, h: Option<&str>| default_path(x.map(Into::into), h.map(Into::into));
    assert_eq!(
        p(Some("/x"), Some("/h")),
        Some(PathBuf::from("/x/wt-linux/config.toml"))
    );
    assert_eq!(
        p(Some("rel"), Some("/h")),
        Some(PathBuf::from("/h/.config/wt-linux/config.toml"))
    );
    assert_eq!(
        p(None, Some("/h")),
        Some(PathBuf::from("/h/.config/wt-linux/config.toml"))
    );
    assert_eq!(p(None, None), None);
}

#[test]
fn chords() {
    let c = Chord::parse("Ctrl+Shift+T").unwrap();
    assert!(c.ctrl && c.shift && !c.alt && c.key == ChordKey::Char('t'));
    assert_eq!(
        Chord::parse("alt+shift+plus").unwrap().key,
        ChordKey::Char('+')
    );
    assert_eq!(Chord::parse("ctrl++").unwrap().key, ChordKey::Char('+'));
    assert_eq!(Chord::parse("f11").unwrap().key, ChordKey::Function(11));
    assert_eq!(
        Chord::parse("ctrl+tab").unwrap().key,
        ChordKey::Named("tab")
    );
    assert!(Chord::parse("hyper+x").is_err());
    assert!(Chord::parse("ctrl+banana").is_err());
    for (a, c) in &Config::default().keybindings {
        assert!(Chord::parse(c).is_ok(), "default {a} = {c}");
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wt-config-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn first_run_writes_default_file_then_reads_it() {
    let dir = temp_dir("first-run");
    let path = dir.join("wt-linux/config.toml");
    let l = load(None, Some(&path)).unwrap();
    assert_eq!(l.source, Source::Created(path.clone()));
    assert_eq!(l.config, Config::default());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), default_file_text());

    std::fs::write(&path, "scrollback = 7\n").unwrap();
    let l = load(None, Some(&path)).unwrap();
    assert_eq!(l.source, Source::File(path.clone()));
    assert_eq!(l.config.scrollback, 7);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn explicit_path_must_exist_and_wins_over_default() {
    let dir = temp_dir("explicit");
    std::fs::create_dir_all(&dir).unwrap();
    let default = dir.join("default.toml");
    let explicit = dir.join("explicit.toml");
    std::fs::write(&default, "scrollback = 1\n").unwrap();
    std::fs::write(&explicit, "scrollback = 2\n").unwrap();
    assert_eq!(
        load(Some(&explicit), Some(&default))
            .unwrap()
            .config
            .scrollback,
        2
    );
    assert!(load(Some(&dir.join("missing.toml")), Some(&default)).is_err());
    std::fs::write(&explicit, "default_profile = \"ghost\"\n").unwrap();
    let e = format!("{:#}", load(Some(&explicit), None).unwrap_err());
    assert!(e.contains("ghost") && e.contains("explicit.toml"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_home_means_builtin() {
    let l = load(None, None).unwrap();
    assert_eq!(l.source, Source::Builtin);
}
