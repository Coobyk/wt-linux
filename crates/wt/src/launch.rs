//! Turns the parsed command line into what the window opens with: the merged config and the
//! first tab's profile. Precedence: built-in defaults < config file < `--profile`/`--command`.

use std::ffi::OsString;

use crate::cli::Cli;
use crate::config::{self, Config, ResolvedProfile, Source};

pub struct Launch {
    pub config: Config,
    pub source: Source,
    pub warnings: Vec<String>,
    /// The first tab, with `--profile` and `--command` applied.
    pub first: ResolvedProfile,
    pub rune_args: Vec<String>,
}

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn prepare(cli: &Cli, env: Env) -> anyhow::Result<Launch> {
    let default = config::default_path(
        env("XDG_CONFIG_HOME").map(OsString::from),
        env("HOME").map(OsString::from),
    );
    let loaded = config::load(cli.config.as_deref(), default.as_deref())?;
    let mut first = loaded.config.resolve_profile(cli.profile.as_deref(), env)?;
    config::command_override(&mut first, &cli.command);
    Ok(Launch {
        config: loaded.config,
        source: loaded.source,
        warnings: loaded.warnings,
        first,
        rune_args: cli.rune_args.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cli_overrides_file_overrides_defaults() {
        let dir = std::env::temp_dir().join(format!("wt-launch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.toml");
        std::fs::write(
            &path,
            r#"
            default_profile = "a"
            [[profiles]]
            name = "a"
            command = "from-file-a"
            [[profiles]]
            name = "b"
            command = "from-file-b"
        "#,
        )
        .unwrap();
        let env = |_: &str| None;
        let cli = |profile: Option<&str>, command: &[&str]| Cli {
            config: Some(path.clone()),
            profile: profile.map(Into::into),
            command: command.iter().map(|s| s.to_string()).collect(),
            ..Cli::default()
        };
        assert_eq!(
            prepare(&cli(None, &[]), &env).unwrap().first.program,
            "from-file-a"
        );
        assert_eq!(
            prepare(&cli(Some("b"), &[]), &env).unwrap().first.program,
            "from-file-b"
        );
        let l = prepare(&cli(Some("b"), &["htop"]), &env).unwrap();
        assert_eq!(
            (l.first.name.as_str(), l.first.program.as_str()),
            ("b", "htop"),
            "--command keeps --profile's settings"
        );
        assert_eq!(l.source, Source::File(path.clone()));
        assert!(prepare(&cli(Some("zzz"), &[]), &env).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn xdg_config_home_is_used_and_seeded() {
        let dir = std::env::temp_dir().join(format!("wt-launch-xdg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let xdg = dir.to_str().unwrap().to_owned();
        let env = move |k: &str| (k == "XDG_CONFIG_HOME").then(|| xdg.clone());
        let l = prepare(&Cli::default(), &env).unwrap();
        assert_eq!(
            l.source,
            Source::Created(PathBuf::from(&dir).join("wt-linux/config.toml"))
        );
        assert_eq!(l.first.program, "/bin/sh");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
