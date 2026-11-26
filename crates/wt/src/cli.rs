//! Hand-rolled command line: `wt [--config <path>] [--profile <name>] [--command <cmd...>]`.

use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: wt [OPTIONS] [--rune <RUNE-ARGS...> --] [--command <CMD> [ARGS...]]

Options:
  -c, --config <PATH>     Read this config file instead of $XDG_CONFIG_HOME/wt-linux/config.toml
  -p, --profile <NAME>    Open the first tab with this profile instead of `default_profile`
  -e, --command <CMD...>  Run CMD (and every argument after it) in the first tab.
                          A single argument containing spaces runs through `/bin/sh -c`.
      --print-config      Print the effective config as TOML and exit
      --rune <ARGS...>    Pass the following arguments, up to `--`, to Rune's tooling
                          (e.g. --rune --snap out.png -- --command htop)
  -V, --version           Print version and exit
  -h, --help              Print this help and exit";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cli {
    pub config: Option<PathBuf>,
    pub profile: Option<String>,
    /// Program and arguments, verbatim. Empty means "use the profile's command".
    pub command: Vec<String>,
    pub print_config: bool,
    /// Arguments forwarded to `rune::App::args`, so Rune never reads wt's own flags.
    pub rune_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Run(Cli),
    Version,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError(pub String);

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n\n{USAGE}", self.0)
    }
}

impl std::error::Error for CliError {}

pub fn version_string() -> String {
    format!("wt-linux {}", env!("CARGO_PKG_VERSION"))
}

/// Parse arguments without the program name.
pub fn parse<I, T>(args: I) -> Result<Parsed, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args = args.into_iter().map(|a| {
        a.into()
            .into_string()
            .map_err(|a| CliError(format!("argument is not UTF-8: {a:?}")))
    });
    let mut cli = Cli::default();
    while let Some(arg) = args.next() {
        let arg = arg?;
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, CliError> {
            match inline.clone() {
                Some(v) => Ok(v),
                None => match args.next() {
                    Some(v) => v,
                    None => Err(CliError(format!("{name} needs a value"))),
                },
            }
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-c" | "--config" => cli.config = Some(PathBuf::from(value("--config")?)),
            "-p" | "--profile" => cli.profile = Some(value("--profile")?),
            "--print-config" => cli.print_config = true,
            "-e" | "--command" => {
                cli.command.extend(inline);
                for a in args.by_ref() {
                    cli.command.push(a?);
                }
                if cli.command.is_empty() || cli.command[0].is_empty() {
                    return Err(CliError("--command needs a program".into()));
                }
            }
            "--rune" => {
                cli.rune_args.extend(inline);
                for a in args.by_ref() {
                    let a = a?;
                    if a == "--" {
                        break;
                    }
                    cli.rune_args.push(a);
                }
            }
            other if other.starts_with('-') => {
                return Err(CliError(format!("unknown option: {other}")));
            }
            other => {
                return Err(CliError(format!(
                    "unexpected argument: {other} (use --command to run a program)"
                )));
            }
        }
    }
    Ok(Parsed::Run(cli))
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
