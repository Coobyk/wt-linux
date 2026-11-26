//! Config loading. Precedence, lowest to highest: built-in defaults, the TOML file, then
//! command-line overrides (`--profile`, `--command`).

mod keymap;
mod schema;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use serde::Deserialize;

pub use keymap::{Chord, ChordKey};
pub use schema::*;

/// The file on disk: every key optional, laid over [`Config::default`] by [`Config::merge`].
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub default_profile: Option<String>,
    pub scrollback: Option<usize>,
    pub window: WindowFile,
    pub defaults: ProfileSettings,
    pub profiles: Vec<Profile>,
    pub color_schemes: Vec<ColorScheme>,
    pub keybindings: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct WindowFile {
    pub title: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

/// Where the effective config came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    /// No file existed; built-in defaults were used and written to this path.
    Created(PathBuf),
    /// No file existed and none could be written; built-in defaults only.
    Builtin,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    pub source: Source,
    pub warnings: Vec<String>,
}

impl Config {
    pub fn parse(toml_text: &str) -> anyhow::Result<Config> {
        let file: ConfigFile = toml::from_str(toml_text)?;
        let mut config = Config::default();
        config.merge(file);
        Ok(config)
    }

    /// Lay a config file over `self`. Scalars replace; `defaults` and same-named profiles merge
    /// field by field; new profiles append in file order; colour schemes replace by name;
    /// keybindings replace per action.
    pub fn merge(&mut self, file: ConfigFile) {
        if let Some(v) = file.default_profile {
            self.default_profile = v;
        }
        if let Some(v) = file.scrollback {
            self.scrollback = v;
        }
        if let Some(v) = file.window.title {
            self.window.title = v;
        }
        if let Some(v) = file.window.cols {
            self.window.cols = v;
        }
        if let Some(v) = file.window.rows {
            self.window.rows = v;
        }
        self.defaults.overlay(&file.defaults);
        for p in file.profiles {
            match self.profiles.iter_mut().find(|q| q.name == p.name) {
                Some(q) => q.settings.overlay(&p.settings),
                None => self.profiles.push(p),
            }
        }
        for s in file.color_schemes {
            match self.color_schemes.iter_mut().find(|q| q.name == s.name) {
                Some(q) => *q = s,
                None => self.color_schemes.push(s),
            }
        }
        self.keybindings.extend(file.keybindings);
    }

    /// Hard errors make the config unusable; the returned strings are warnings.
    pub fn validate(&self) -> anyhow::Result<Vec<String>> {
        let mut warnings = Vec::new();
        if self.window.cols == 0 || self.window.rows == 0 {
            bail!("window.cols and window.rows must be at least 1");
        }
        if self.profile(&self.default_profile).is_none() {
            bail!(
                "default_profile {:?} names no profile (have: {})",
                self.default_profile,
                self.profile_names().join(", ")
            );
        }
        for s in &self.color_schemes {
            if s.palette.len() != 16 {
                bail!(
                    "color scheme {:?}: palette needs 16 colours, has {}",
                    s.name,
                    s.palette.len()
                );
            }
            for c in [&s.foreground, &s.background, &s.cursor]
                .into_iter()
                .chain(s.selection.as_ref())
                .chain(&s.palette)
            {
                if parse_hex_color(c).is_none() {
                    bail!("color scheme {:?}: {c:?} is not #RRGGBB", s.name);
                }
            }
        }
        let mut layers = vec![("defaults", &self.defaults)];
        layers.extend(self.profiles.iter().map(|p| (p.name.as_str(), &p.settings)));
        for (name, s) in layers {
            if let Some(scheme) = &s.color_scheme
                && self.color_scheme(scheme).is_none()
            {
                warnings.push(format!(
                    "{name}: unknown color_scheme {scheme:?}, using Campbell"
                ));
            }
            if let Some(size) = s.font_size
                && !(size.is_finite() && size > 0.0)
            {
                bail!("{name}: font_size must be positive, got {size}");
            }
        }
        for (action, chord) in &self.keybindings {
            if !KNOWN_ACTIONS.contains(&action.as_str()) {
                warnings.push(format!("keybindings: unknown action {action:?}"));
            }
            if !chord.is_empty()
                && let Err(e) = Chord::parse(chord)
            {
                warnings.push(format!("keybindings.{action}: {e}"));
            }
        }
        Ok(warnings)
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name).or_else(|| {
            self.profiles
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
        })
    }

    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }

    pub fn color_scheme(&self, name: &str) -> Option<&ColorScheme> {
        self.color_schemes.iter().find(|s| s.name == name)
    }

    /// Bound chords by action, skipping unbound and unparsable entries.
    pub fn keymap(&self) -> Vec<(String, Chord)> {
        self.keybindings
            .iter()
            .filter_map(|(a, c)| Chord::parse(c).ok().map(|c| (a.clone(), c)))
            .collect()
    }

    /// Fold built-ins, `defaults` and the named profile (or `default_profile`) into what a tab
    /// launches. `env` supplies `$VAR` lookups and `HOME`/`SHELL`.
    pub fn resolve_profile(
        &self,
        name: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> anyhow::Result<ResolvedProfile> {
        let name = name.unwrap_or(&self.default_profile);
        let profile = self.profile(name).ok_or_else(|| {
            anyhow!(
                "no profile named {name:?} (have: {})",
                self.profile_names().join(", ")
            )
        })?;
        let mut s = self.defaults.clone();
        s.overlay(&profile.settings);

        let program = match s.command.as_deref().map(str::trim) {
            Some(c) if !c.is_empty() => expand_vars(c, env),
            _ => String::new(),
        };
        let program = if program.is_empty() {
            env("SHELL")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/sh".into())
        } else {
            program
        };
        let args = s
            .args
            .unwrap_or_default()
            .iter()
            .map(|a| expand_vars(a, env))
            .collect();
        let cwd = s
            .cwd
            .as_deref()
            .map(|c| PathBuf::from(expand_home(&expand_vars(c, env), env)));

        let mut vars: BTreeMap<String, String> = [
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            ("TERM_PROGRAM", "wt-linux"),
            ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        vars.insert("WT_PROFILE".into(), profile.name.clone());
        for (k, v) in &s.env {
            vars.insert(k.clone(), expand_vars(v, env));
        }

        let scheme = s
            .color_scheme
            .as_deref()
            .and_then(|n| self.color_scheme(n))
            .cloned()
            .unwrap_or_else(ColorScheme::campbell);
        Ok(ResolvedProfile {
            name: profile.name.clone(),
            program,
            args,
            cwd,
            env: vars.into_iter().collect(),
            font_family: s.font_family.unwrap_or_else(|| "monospace".into()),
            font_size: s.font_size.unwrap_or(12.0),
            color_scheme: scheme,
            cursor_shape: s.cursor_shape.unwrap_or(CursorShape::Bar),
        })
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("Config serializes")
    }
}

/// `--command` as the user typed it: several words run as program + args; one word with
/// whitespace in it runs through `/bin/sh -c` so quoting and pipes work.
pub fn command_override(profile: &mut ResolvedProfile, command: &[String]) {
    let Some((first, rest)) = command.split_first() else {
        return;
    };
    if rest.is_empty() && first.trim().contains(char::is_whitespace) {
        profile.program = "/bin/sh".into();
        profile.args = vec!["-c".into(), first.clone()];
    } else {
        profile.program = first.clone();
        profile.args = rest.to_vec();
    }
}

/// `$VAR` and `${VAR}`; unknown variables expand to nothing, `$$` is a literal `$`.
pub fn expand_vars(s: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let name: String = match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
                continue;
            }
            Some('{') => {
                chars.next();
                let n: String = chars.by_ref().take_while(|&c| c != '}').collect();
                n
            }
            _ => {
                let mut n = String::new();
                while let Some(&c) = chars.peek()
                    && (c.is_ascii_alphanumeric() || c == '_')
                {
                    n.push(c);
                    chars.next();
                }
                if n.is_empty() {
                    out.push('$');
                    continue;
                }
                n
            }
        };
        out.push_str(&env(&name).unwrap_or_default());
    }
    out
}

fn expand_home(s: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    match (s.strip_prefix('~'), env("HOME")) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{home}{rest}")
        }
        _ => s.to_owned(),
    }
}

/// `$XDG_CONFIG_HOME/wt-linux/config.toml`, else `$HOME/.config/wt-linux/config.toml`.
/// A relative `XDG_CONFIG_HOME` is ignored, as the XDG spec says.
pub fn default_path(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("wt-linux").join("config.toml"))
}

pub const DEFAULT_HEADER: &str = "\
# wt-linux configuration. Every key is optional: anything left out uses the built-in default.
# Precedence: built-in defaults < this file < command-line flags (--profile, --command).
# `defaults` applies to every profile; a [[profiles]] entry with an existing name merges into it.

";

/// The file written on first run.
pub fn default_file_text() -> String {
    format!("{DEFAULT_HEADER}{}", Config::default().to_toml())
}

/// Load `explicit` (which must exist) or the default path (created with defaults when missing).
pub fn load(explicit: Option<&Path>, default: Option<&Path>) -> anyhow::Result<Loaded> {
    let (config, source) = match (explicit, default) {
        (Some(path), _) => (read(path)?, Source::File(path.to_owned())),
        (None, Some(path)) if path.exists() => (read(path)?, Source::File(path.to_owned())),
        (None, Some(path)) => match write_default(path) {
            Ok(()) => (Config::default(), Source::Created(path.to_owned())),
            Err(e) => {
                let mut l = finish(Config::default(), Source::Builtin)?;
                l.warnings.push(format!(
                    "could not write default config to {}: {e:#}",
                    path.display()
                ));
                return Ok(l);
            }
        },
        (None, None) => (Config::default(), Source::Builtin),
    };
    finish(config, source)
}

fn finish(config: Config, source: Source) -> anyhow::Result<Loaded> {
    let warnings = config.validate().with_context(|| match &source {
        Source::File(p) => format!("invalid config {}", p.display()),
        _ => "invalid built-in config".to_owned(),
    })?;
    Ok(Loaded {
        config,
        source,
        warnings,
    })
}

fn read(path: &Path) -> anyhow::Result<Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Config::parse(&text).with_context(|| format!("parsing {}", path.display()))
}

fn write_default(path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // create_new: never clobber a file that appeared since the exists() check.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(default_file_text().as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests;
