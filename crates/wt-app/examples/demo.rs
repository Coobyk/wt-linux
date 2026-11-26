//! The shell over fake terminals. Headless: `--scene split --snap split.png` or `--tree`.
fn main() {
    wt_app::rune::run(wt_app::spec());
}
