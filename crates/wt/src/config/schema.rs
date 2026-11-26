//! The TOML config schema.
//!
//! docs/ARCHITECTURE.md assigns the profile model and TOML schema to `wt-app`. wt-app has no
//! schema yet, so wt defines it here, shaped after Windows Terminal's settings.json
//! (`defaultProfile`, `profiles.defaults`, `profiles.list`, `schemes`, `actions`) in snake_case
//! TOML. When wt-app publishes its own types this module should become a re-export of them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Profile `name` the first tab and new tabs open with.
    pub default_profile: String,
    /// Lines of scrollback each terminal keeps.
    pub scrollback: usize,
    pub window: WindowConfig,
    /// Settings every profile inherits unless it sets its own (WT's `profiles.defaults`).
    pub defaults: ProfileSettings,
    pub profiles: Vec<Profile>,
    pub color_schemes: Vec<ColorScheme>,
    /// Action name -> chord, e.g. `new_tab = "ctrl+shift+t"`. An empty chord unbinds.
    pub keybindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    pub title: String,
    /// Initial grid size of the first tab, in cells.
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(flatten)]
    pub settings: ProfileSettings,
}

/// Every field optional: unset fields fall through profile -> `defaults` -> built-in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSettings {
    /// Program to run. `$SHELL`-style variables expand; unset means the user's login shell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Starting directory; a leading `~` expands to `$HOME`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Extra environment. Merged key by key across layers, not replaced.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_shape: Option<CursorShape>,
    /// Hidden profiles stay launchable by name but are left out of menus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    /// The 16 ANSI colours: black, red, green, yellow, blue, purple, cyan, white, then brights.
    pub palette: Vec<String>,
}

/// A profile with every layer folded in and its command resolved: what one tab launches.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProfile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub env: Vec<(String, String)>,
    pub font_family: String,
    pub font_size: f32,
    pub color_scheme: ColorScheme,
    pub cursor_shape: CursorShape,
}

impl ProfileSettings {
    /// Lay `top` over `self`: every field `top` sets wins, `env` merges by key.
    pub fn overlay(&mut self, top: &ProfileSettings) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(
            command,
            args,
            cwd,
            font_family,
            font_size,
            color_scheme,
            cursor_shape,
            hidden
        );
        for (k, v) in &top.env {
            self.env.insert(k.clone(), v.clone());
        }
    }
}

impl ColorScheme {
    fn new(name: &str, fg: &str, bg: &str, cursor: &str, palette: [&str; 16]) -> Self {
        Self {
            name: name.into(),
            foreground: fg.into(),
            background: bg.into(),
            cursor: cursor.into(),
            selection: None,
            palette: palette.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    /// Windows Terminal's default scheme.
    pub fn campbell() -> Self {
        Self::new(
            "Campbell",
            "#CCCCCC",
            "#0C0C0C",
            "#FFFFFF",
            [
                "#0C0C0C", "#C50F1F", "#13A10E", "#C19C00", "#0037DA", "#881798", "#3A96DD",
                "#CCCCCC", "#767676", "#E74856", "#16C60C", "#F9F1A5", "#3B78FF", "#B4009E",
                "#61D6D6", "#F2F2F2",
            ],
        )
    }

    pub fn one_half_dark() -> Self {
        Self::new(
            "One Half Dark",
            "#DCDFE4",
            "#282C34",
            "#FFFFFF",
            [
                "#282C34", "#E06C75", "#98C379", "#E5C07B", "#61AFEF", "#C678DD", "#56B6C2",
                "#DCDFE4", "#5A6374", "#E06C75", "#98C379", "#E5C07B", "#61AFEF", "#C678DD",
                "#56B6C2", "#DCDFE4",
            ],
        )
    }
}

/// Parse `#RRGGBB` (or `RRGGBB`).
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Actions the keymap knows. wt-app owns the real actions enum; unknown names only warn.
pub const KNOWN_ACTIONS: &[&str] = &[
    "new_tab",
    "close_tab",
    "next_tab",
    "prev_tab",
    "copy",
    "paste",
    "find",
    "command_palette",
    "open_settings",
    "split_vertical",
    "split_horizontal",
    "close_pane",
    "scroll_up",
    "scroll_down",
    "scroll_page_up",
    "scroll_page_down",
    "increase_font_size",
    "decrease_font_size",
    "reset_font_size",
    "toggle_fullscreen",
    "quit",
];

impl Default for Config {
    fn default() -> Self {
        let keybindings = [
            ("new_tab", "ctrl+shift+t"),
            ("close_tab", "ctrl+shift+w"),
            ("next_tab", "ctrl+tab"),
            ("prev_tab", "ctrl+shift+tab"),
            ("copy", "ctrl+shift+c"),
            ("paste", "ctrl+shift+v"),
            ("find", "ctrl+shift+f"),
            ("command_palette", "ctrl+shift+p"),
            ("open_settings", "ctrl+comma"),
            ("split_vertical", "alt+shift+plus"),
            ("split_horizontal", "alt+shift+minus"),
            ("scroll_up", "ctrl+shift+up"),
            ("scroll_down", "ctrl+shift+down"),
            ("scroll_page_up", "ctrl+shift+pageup"),
            ("scroll_page_down", "ctrl+shift+pagedown"),
            ("increase_font_size", "ctrl+plus"),
            ("decrease_font_size", "ctrl+minus"),
            ("reset_font_size", "ctrl+0"),
            ("toggle_fullscreen", "f11"),
        ];
        Self {
            default_profile: "Shell".into(),
            scrollback: 10_000,
            window: WindowConfig {
                title: "wt-linux".into(),
                cols: 120,
                rows: 30,
            },
            defaults: ProfileSettings {
                font_family: Some("monospace".into()),
                font_size: Some(12.0),
                color_scheme: Some("Campbell".into()),
                cursor_shape: Some(CursorShape::Bar),
                ..ProfileSettings::default()
            },
            profiles: vec![Profile {
                name: "Shell".into(),
                settings: ProfileSettings::default(),
            }],
            color_schemes: vec![ColorScheme::campbell(), ColorScheme::one_half_dark()],
            keybindings: keybindings
                .iter()
                .map(|(a, c)| ((*a).to_owned(), (*c).to_owned()))
                .collect(),
        }
    }
}
