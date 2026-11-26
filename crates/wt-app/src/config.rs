//! The TOML config at `$XDG_CONFIG_HOME/wt-linux/config.toml` (schema: `CONFIG.md`).
//! Loading happens once at startup or from a settings "Reload" press, never per frame.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::scheme::{ColorScheme, builtin_schemes};

pub const SAMPLE_CONFIG: &str = include_str!("../config.sample.toml");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Name of the profile new tabs use. Falls back to the first profile.
    pub default_profile: Option<String>,
    /// `"dark"` or `"light"` chrome.
    pub theme: Theme,
    pub window: WindowConfig,
    /// Defaults every profile inherits when it leaves `font` unset.
    pub font: FontConfig,
    pub profiles: Vec<Profile>,
    /// Extra colour schemes; built-ins (`Campbell`, `One Half Dark`, `One Half Light`) are always present.
    pub schemes: Vec<ColorScheme>,
    /// `action -> key` or `action -> [keys]`. An empty string or list unbinds the action.
    pub keybindings: BTreeMap<String, KeySpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    pub width: f32,
    pub height: f32,
}

impl Default for WindowConfig {
    fn default() -> WindowConfig {
        WindowConfig {
            width: 1100.0,
            height: 680.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Family name; `"mono"` is Rune's bundled monospace face.
    pub family: String,
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> FontConfig {
        FontConfig {
            family: "mono".into(),
            size: 13.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// Executable; empty means `$SHELL`, then `/bin/sh`.
    pub program: String,
    pub args: Vec<String>,
    /// Working directory; `~` expands to `$HOME`. Unset inherits the app's.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub color_scheme: String,
    pub font: Option<FontConfig>,
    /// Background opacity, 0.0..=1.0.
    pub opacity: f32,
    pub hidden: bool,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            name: "Shell".into(),
            program: String::new(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            color_scheme: "Campbell".into(),
            font: None,
            opacity: 1.0,
            hidden: false,
        }
    }
}

impl Profile {
    pub fn resolved_program(&self) -> String {
        if !self.program.is_empty() {
            return self.program.clone();
        }
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into())
    }

    pub fn resolved_cwd(&self) -> Option<PathBuf> {
        let cwd = self.cwd.as_deref()?;
        match cwd.strip_prefix('~') {
            Some(rest) => std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(rest.trim_start_matches('/'))),
            None => Some(PathBuf::from(cwd)),
        }
    }

    /// Program basename, used as the initial tab title.
    pub fn short_program(&self) -> String {
        let p = self.resolved_program();
        p.rsplit('/').next().unwrap_or(&p).to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    One(String),
    Many(Vec<String>),
}

impl KeySpec {
    pub fn keys(&self) -> Vec<&str> {
        match self {
            KeySpec::One(s) if s.trim().is_empty() => Vec::new(),
            KeySpec::One(s) => vec![s.as_str()],
            KeySpec::Many(v) => v
                .iter()
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
                .collect(),
        }
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            default_profile: None,
            theme: Theme::Dark,
            window: WindowConfig::default(),
            font: FontConfig::default(),
            profiles: vec![Profile::default()],
            schemes: Vec::new(),
            keybindings: BTreeMap::new(),
        }
    }
}

/// `$XDG_CONFIG_HOME/wt-linux/config.toml`, else `$HOME/.config/wt-linux/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    config_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn config_path_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = match xdg.filter(|x| !x.is_empty() && Path::new(x).is_absolute()) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(home.filter(|h| !h.is_empty())?).join(".config"),
    };
    Some(base.join("wt-linux").join("config.toml"))
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Config> {
        let mut c: Config = toml::from_str(s).context("parsing wt-linux config")?;
        c.normalize();
        Ok(c)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Reads `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(s) => Config::from_toml(&s).with_context(|| format!("in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// `load(default_config_path())`, or defaults when no path can be formed.
    pub fn load_default() -> Result<Config> {
        match default_config_path() {
            Some(p) => Config::load(&p),
            None => Ok(Config::default()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }

    fn normalize(&mut self) {
        if self.profiles.is_empty() {
            self.profiles.push(Profile::default());
        }
        for p in &mut self.profiles {
            p.opacity = p.opacity.clamp(0.0, 1.0);
        }
        self.font.size = self.font.size.clamp(4.0, 96.0);
    }

    pub fn default_profile_index(&self) -> usize {
        self.default_profile
            .as_deref()
            .and_then(|n| self.profiles.iter().position(|p| p.name == n))
            .unwrap_or(0)
    }

    /// The profile's font, else the global default.
    pub fn font_for(&self, profile: &Profile) -> FontConfig {
        profile.font.clone().unwrap_or_else(|| self.font.clone())
    }

    /// User schemes override built-ins of the same name. Unknown names fall back to Campbell.
    pub fn scheme(&self, name: &str) -> ColorScheme {
        self.all_schemes()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| builtin_schemes().remove(0))
    }

    pub fn all_schemes(&self) -> Vec<ColorScheme> {
        let mut out = self.schemes.clone();
        for b in builtin_schemes() {
            if !out.iter().any(|s| s.name == b.name) {
                out.push(b);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_config_parses() {
        let c = Config::from_toml(SAMPLE_CONFIG).unwrap();
        assert!(c.profiles.len() >= 2);
        assert_eq!(
            c.profiles[c.default_profile_index()].name,
            c.default_profile.clone().unwrap()
        );
        assert!(c.keybindings.contains_key("new_tab"));
        for name in c.keybindings.keys() {
            name.parse::<crate::Action>().unwrap();
        }
        for p in &c.profiles {
            assert_eq!(c.scheme(&p.color_scheme).name, p.color_scheme);
        }
    }

    #[test]
    fn empty_config_is_default_and_round_trips() {
        let c = Config::from_toml("").unwrap();
        assert_eq!(c, Config::default());
        let s = Config::from_toml(SAMPLE_CONFIG).unwrap();
        assert_eq!(Config::from_toml(&s.to_toml().unwrap()).unwrap(), s);
    }

    #[test]
    fn unknown_fields_are_errors() {
        assert!(Config::from_toml("[[profiles]]\nname = \"x\"\nprogramm = \"zsh\"\n").is_err());
    }

    #[test]
    fn xdg_path() {
        let p = config_path_from(Some("/x/cfg".into()), Some("/home/u".into())).unwrap();
        assert_eq!(p, PathBuf::from("/x/cfg/wt-linux/config.toml"));
        let p = config_path_from(Some("relative".into()), Some("/home/u".into())).unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/wt-linux/config.toml"));
        assert_eq!(config_path_from(None, None), None);
    }

    #[test]
    fn load_missing_is_default() {
        let c = Config::load(Path::new("/nonexistent/wt-linux/config.toml")).unwrap();
        assert_eq!(c, Config::default());
    }
}
