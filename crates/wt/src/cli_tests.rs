use super::*;

fn run(args: &[&str]) -> Cli {
    match parse(args.iter().copied()) {
        Ok(Parsed::Run(cli)) => cli,
        other => panic!("expected Run for {args:?}, got {other:?}"),
    }
}

#[test]
fn no_args_is_defaults() {
    assert_eq!(run(&[]), Cli::default());
}

#[test]
fn version_and_help() {
    assert_eq!(parse(["--version"]), Ok(Parsed::Version));
    assert_eq!(parse(["-V"]), Ok(Parsed::Version));
    assert_eq!(parse(["-h"]), Ok(Parsed::Help));
    assert_eq!(parse(["--profile", "x", "--help"]), Ok(Parsed::Help));
    assert!(version_string().starts_with("wt-linux "));
}

#[test]
fn config_and_profile_in_both_spellings() {
    let a = run(&["--config", "/tmp/a.toml", "--profile", "zsh"]);
    let b = run(&["--config=/tmp/a.toml", "-p", "zsh"]);
    let c = run(&["-c", "/tmp/a.toml", "--profile=zsh"]);
    for cli in [a, b, c] {
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/a.toml")));
        assert_eq!(cli.profile.as_deref(), Some("zsh"));
    }
}

#[test]
fn later_flags_win() {
    assert_eq!(run(&["-p", "a", "-p", "b"]).profile.as_deref(), Some("b"));
}

#[test]
fn command_takes_everything_after_it() {
    let cli = run(&[
        "-p",
        "bash",
        "--command",
        "htop",
        "-d",
        "5",
        "--profile",
        "nope",
    ]);
    assert_eq!(cli.profile.as_deref(), Some("bash"));
    assert_eq!(cli.command, ["htop", "-d", "5", "--profile", "nope"]);
    assert_eq!(run(&["-e", "vim"]).command, ["vim"]);
    assert_eq!(run(&["--command=ls", "-la"]).command, ["ls", "-la"]);
}

#[test]
fn rune_args_pass_through() {
    let cli = run(&["--rune", "--snap", "out.png", "--scene", "x"]);
    assert_eq!(cli.rune_args, ["--snap", "out.png", "--scene", "x"]);
    assert!(cli.command.is_empty());
    let cli = run(&[
        "--rune",
        "--tree",
        "--",
        "-p",
        "a",
        "--command",
        "echo",
        "--",
    ]);
    assert_eq!(cli.rune_args, ["--tree"]);
    assert_eq!(cli.profile.as_deref(), Some("a"));
    assert_eq!(
        cli.command,
        ["echo", "--"],
        "`--` after --command belongs to the command"
    );
}

#[test]
fn errors() {
    assert!(parse(["--config"]).is_err());
    assert!(parse(["--command"]).is_err());
    assert!(parse(["--bogus"]).is_err());
    assert!(parse(["stray"]).is_err());
    let msg = parse(["--bogus"]).unwrap_err().to_string();
    assert!(msg.contains("--bogus") && msg.contains("Usage"));
}
