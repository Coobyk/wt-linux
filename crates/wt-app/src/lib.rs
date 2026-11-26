//! wt-app — the Rune app shell for wt-linux (see docs/ARCHITECTURE.md): tab bar, split-pane
//! layout tree, command palette, profile/config model, settings tab and a rebindable keymap.
//!
//! Headless check: `cargo run -p wt-app --example demo -- --scene split --snap split.png`.
pub mod action;
pub mod app;
pub mod chrome;
pub mod config;
pub mod grid_view;
pub mod input;
pub mod keymap;
pub mod layout;
pub mod palette;
pub mod scheme;
pub mod settings;
pub mod terminal;
pub mod view;

pub use action::Action;
pub use app::State;
pub use config::Config;
pub use keymap::Keymap;
pub use layout::Layout;
pub use rune;
pub use terminal::FakeTerminal;
pub use terminal::PaneTerminal;
pub use terminal::TerminalFactory;

use rune::{App, AppSpec};

/// Demo scenes for `--scene NAME` (headless snapshots and the playground).
pub const SCENES: &[(&str, fn(&mut State))] = &[
    ("tabs", scene_tabs),
    ("split", scene_split),
    ("palette", scene_palette),
    ("settings", scene_settings),
];

fn scene_tabs(s: &mut State) {
    s.dispatch(Action::NewTabProfile(1.min(s.config.profiles.len() - 1)));
    s.dispatch(Action::NewTab);
    s.tabs[2].custom_title = Some("build".into());
    s.dispatch(Action::SwitchToTab(0));
}

fn scene_split(s: &mut State) {
    scene_tabs(s);
    s.dispatch(Action::SplitVertical);
    s.dispatch(Action::SplitHorizontal);
    if let Some(p) = s.focused_pane().and_then(|p| s.panes.get_mut(&p)) {
        p.term.write(b"cargo build -p wt-app");
    }
}

fn scene_palette(s: &mut State) {
    scene_split(s);
    s.dispatch(Action::OpenPalette);
    if let app::Mode::Palette(p) = &mut s.mode {
        p.query = rune::TextEdit::new("split");
    }
}

fn scene_settings(s: &mut State) {
    s.dispatch(Action::OpenSettings);
    s.settings_section = app::SettingsSection::Profiles;
}

/// The window's spec. `init` builds a default-config state; use [`app`] to supply one.
pub fn spec() -> AppSpec<State> {
    AppSpec {
        scenes: SCENES,
        on_key: Some(view::on_key),
        poll: Some(view::poll),
        min_size: Some((480.0, 320.0)),
        ..AppSpec::new("wt-linux", (1100.0, 680.0), demo_state, view::view)
    }
}

/// A demo state over `FakeTerminal`s with the sample config's profiles.
pub fn demo_state() -> State {
    State::new(Config::from_toml(config::SAMPLE_CONFIG).expect("sample config parses"))
}

/// The app with a given config, config path and terminal factory (the `wt` binary passes a
/// factory over wt-backend + wt-vt; `terminal::fake_factory()` otherwise).
pub fn app(
    config: Config,
    config_path: Option<std::path::PathBuf>,
    factory: TerminalFactory,
) -> App<State> {
    let mut spec = spec();
    spec.size = (config.window.width, config.window.height);
    App::new(spec).init(move || State::with_factory(config, config_path, factory))
}

/// Loads `$XDG_CONFIG_HOME/wt-linux/config.toml` and runs the window over fake terminals.
pub fn run() -> anyhow::Result<()> {
    let path = config::default_config_path();
    let config = match &path {
        Some(p) => Config::load(p)?,
        None => Config::default(),
    };
    app(config, path, terminal::fake_factory()).run();
    Ok(())
}
