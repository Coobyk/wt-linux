//! Feed stdin through a `Terminal` and dump the resulting screen + attributes.
//!
//! `printf 'a\033[31mB\033[0m\n' | cargo run -p wt-vt --example vtdump -- 40 5`

use std::io::Read;

fn main() {
    let mut args = std::env::args().skip(1);
    let cols = args.next().and_then(|a| a.parse().ok()).unwrap_or(80);
    let rows = args.next().and_then(|a| a.parse().ok()).unwrap_or(24);
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("read stdin");

    let mut term = wt_vt::Terminal::new(cols, rows, 10_000);
    // Treat bare LF as CRLF like a tty's onlcr would.
    let mut bytes = Vec::with_capacity(input.len());
    for b in input {
        if b == b'\n' {
            bytes.push(b'\r');
        }
        bytes.push(b);
    }
    term.feed(&bytes);
    term.flush_sync_update();

    let grid = term.grid();
    println!("{term:?}");
    for line in grid.line_range() {
        let l = grid.line(line);
        println!(
            "{line:>4} |{}|{}",
            l.text(),
            if l.is_wrapped() { " (wrapped)" } else { "" }
        );
    }
    println!("styled cells:");
    for line in grid.line_range() {
        for (col, c) in grid.line(line).cells().enumerate() {
            if c.fg != wt_vt::Color::Default || c.bg != wt_vt::Color::Default || !c.attrs.is_empty()
            {
                println!(
                    "  ({line},{col}) {:?} fg={:?} bg={:?} attrs={:?} {:?}",
                    c.text(),
                    c.fg,
                    c.bg,
                    c.attrs,
                    c.width
                );
            }
        }
    }
}
