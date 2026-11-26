use std::process::ExitCode;

use wt::cli::{self, Parsed, USAGE};
use wt::config::Source;
use wt::launch::{self, process_env};

fn main() -> ExitCode {
    let cli = match cli::parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Run(cli)) => cli,
        Ok(Parsed::Version) => {
            println!("{}", cli::version_string());
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Help) => {
            println!("{}\n\n{USAGE}", cli::version_string());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("wt: {}\n\n{USAGE}", e.0);
            return ExitCode::from(2);
        }
    };
    let launch = match launch::prepare(&cli, &process_env) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("wt: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    for w in &launch.warnings {
        eprintln!("wt: warning: {w}");
    }
    if let Source::Created(p) = &launch.source {
        eprintln!("wt: wrote default config to {}", p.display());
    }
    if cli.print_config {
        print!("{}", launch.config.to_toml());
        return ExitCode::SUCCESS;
    }
    run(launch)
}

#[cfg(feature = "real-app")]
fn run(launch: launch::Launch) -> ExitCode {
    // TODO(wt-app): proposed entry point; wt-app owns the window, tabs and keymap.
    wt_app::run(launch.config, launch.first, launch.rune_args);
    ExitCode::SUCCESS
}

#[cfg(all(feature = "ui", not(feature = "real-app")))]
fn run(launch: launch::Launch) -> ExitCode {
    wt::ui::run(launch);
    ExitCode::SUCCESS
}

/// No window: run the first tab to completion and print its final screen.
#[cfg(not(any(feature = "ui", feature = "real-app")))]
fn run(launch: launch::Launch) -> ExitCode {
    use std::sync::Arc;
    let w = &launch.config.window;
    let mut s = match wt::session::Session::spawn(
        launch.first,
        w.cols,
        w.rows,
        launch.config.scrollback,
        Arc::new(|| {}),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("wt: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let mut input = s.input();
    std::thread::spawn(move || {
        use std::io::Write;
        if std::io::copy(&mut std::io::stdin().lock(), &mut input).is_ok() {
            let _ = input.write_all(b"\x04").and_then(|()| input.flush());
        }
    });
    while !(s.is_finished() && s.exit_status().is_some()) {
        s.drain(wt::session::DRAIN_BUDGET);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    #[cfg(not(feature = "real-vt"))]
    {
        let g = s.term.grid();
        for r in 0..g.rows() {
            println!("{}", g.line_text(r));
        }
    }
    ExitCode::from(s.exit_status().and_then(|e| e.code()).unwrap_or(1) as u8)
}
