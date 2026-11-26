use super::*;

fn launch(cmd: &[&str]) -> Launch {
    let mut first = Config::default().resolve_profile(None, &|_| None).unwrap();
    crate::config::command_override(
        &mut first,
        &cmd.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    Launch {
        config: Config::default(),
        source: crate::config::Source::Builtin,
        warnings: vec![],
        first,
        rune_args: vec![],
    }
}

#[test]
fn cell_fit_resizes_every_tab() {
    let mut s = State::new(launch(&["true"]));
    s.open_tab(s.tabs.sessions[0].profile.clone());
    let (cw, ch) = s.cell();
    s.fit([0.0, 0.0, 80.0 * cw + 2.0 * PAD, 24.0 * ch + 2.0 * PAD]);
    assert_eq!(s.grid, (80, 24));
    for t in &s.tabs.sessions {
        assert_eq!(t.term.grid().cols(), 80);
    }
}

#[test]
fn headless_frame_shows_child_output() {
    let mut s = State::new(launch(&["printf 'hi from wt'"]));
    s.settle(Duration::from_secs(5), Duration::from_millis(200));
    let state = std::cell::RefCell::new(Some(s));
    let app = App::new(AppSpec::new("wt-test", (640.0, 360.0), fallback_init, view))
        .init(move || state.borrow_mut().take().unwrap());
    let mut rt = app.headless_runtime((640.0, 360.0), rune::Env::default());
    rt.frame(0.016);
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(texts.iter().any(|t| t.contains("hi from wt")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Shell (exited: 0)"), "{texts:?}");
    assert!(rt.find("tab-0").is_some() && rt.find("new-tab").is_some());
}
