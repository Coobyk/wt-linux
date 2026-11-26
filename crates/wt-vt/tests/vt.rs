//! Headless behaviour tests: feed real escape sequences, assert grid state.

use wt_vt::*;

fn term(cols: u16, rows: u16) -> Terminal {
    Terminal::new(cols, rows, 1000)
}

fn screen(t: &Terminal) -> String {
    t.grid().screen_text()
}

fn row(t: &Terminal, r: i32) -> String {
    t.grid().line(r).text().trim_end().to_owned()
}

#[test]
fn terminal_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Terminal>();
}

#[test]
fn plain_text_and_newlines() {
    let mut t = term(10, 3);
    t.feed(b"hello\r\nworld");
    assert_eq!(screen(&t), "hello\nworld\n\n");
    let c = t.cursor();
    assert_eq!((c.row, c.col), (1, 5));
    assert!(c.visible);
}

#[test]
fn sgr_palette_colors() {
    let mut t = term(20, 2);
    // printf 'a\033[31mB\033[42mC\033[0mD'
    t.feed(b"a\x1b[31mB\x1b[42mC\x1b[0mD");
    let g = t.grid();
    assert_eq!(g.cell(0, 0).fg, Color::Default);
    assert_eq!(g.cell(0, 1).ch, 'B');
    assert_eq!(g.cell(0, 1).fg, Color::Indexed(1));
    assert_eq!(g.cell(0, 2).fg, Color::Indexed(1));
    assert_eq!(g.cell(0, 2).bg, Color::Indexed(2));
    assert_eq!(g.cell(0, 3).fg, Color::Default);
    assert_eq!(g.cell(0, 3).bg, Color::Default);
}

#[test]
fn sgr_bright_256_and_truecolor() {
    let mut t = term(20, 2);
    t.feed(
        b"\x1b[91ma\x1b[38;5;200mb\x1b[48;5;17mc\x1b[38;2;1;2;3md\x1b[48:2::10:20:30me\x1b[39;49mf",
    );
    let g = t.grid();
    assert_eq!(g.cell(0, 0).fg, Color::Indexed(9));
    assert_eq!(g.cell(0, 1).fg, Color::Indexed(200));
    assert_eq!(g.cell(0, 2).bg, Color::Indexed(17));
    assert_eq!(g.cell(0, 3).fg, Color::Rgb(Rgb::new(1, 2, 3)));
    assert_eq!(g.cell(0, 4).bg, Color::Rgb(Rgb::new(10, 20, 30)));
    assert_eq!(g.cell(0, 5).fg, Color::Default);
    assert_eq!(g.cell(0, 5).bg, Color::Default);
}

#[test]
fn sgr_attributes() {
    let mut t = term(20, 2);
    t.feed(b"\x1b[1ma\x1b[22;2mb\x1b[22;3mc\x1b[23;4md\x1b[24;9me\x1b[29;7mf\x1b[27;5mg\x1b[25mh");
    t.feed(b"\x1b[0m\x1b[4:3mi\x1b[4:2mj\x1b[0;8mk\x1b[0;1;3;5;7mz\x1b[0mn");
    let g = t.grid();
    let a = |c| g.cell(0, c).attrs;
    assert_eq!(a(0), Attrs::BOLD);
    assert_eq!(a(1), Attrs::DIM);
    assert_eq!(a(2), Attrs::ITALIC);
    assert_eq!(a(3), Attrs::UNDERLINE);
    assert_eq!(a(4), Attrs::STRIKETHROUGH);
    assert_eq!(a(5), Attrs::INVERSE);
    assert_eq!(a(6), Attrs::BLINK);
    assert_eq!(a(7), Attrs::empty());
    assert_eq!(a(8), Attrs::UNDERLINE | Attrs::UNDERCURL);
    assert_eq!(a(9), Attrs::UNDERLINE | Attrs::DOUBLE_UNDERLINE);
    assert_eq!(a(10), Attrs::HIDDEN);
    assert_eq!(
        a(11),
        Attrs::BOLD | Attrs::ITALIC | Attrs::BLINK | Attrs::INVERSE
    );
    assert_eq!(a(12), Attrs::empty());
}

#[test]
fn blink_survives_other_sgr_but_not_reset() {
    let mut t = term(10, 1);
    t.feed(b"\x1b[5ma\x1b[1mb\x1b[22mc\x1b[0md");
    let g = t.grid();
    assert_eq!(g.cell(0, 0).attrs, Attrs::BLINK);
    assert_eq!(g.cell(0, 1).attrs, Attrs::BLINK | Attrs::BOLD);
    assert_eq!(g.cell(0, 2).attrs, Attrs::BLINK);
    assert_eq!(g.cell(0, 3).attrs, Attrs::empty());
}

#[test]
fn underline_color() {
    let mut t = term(10, 1);
    t.feed(b"\x1b[4;58;5;3mu\x1b[59mv");
    let g = t.grid();
    assert_eq!(g.cell(0, 0).underline_color, Some(Color::Indexed(3)));
    assert_eq!(g.cell(0, 1).underline_color, None);
}

#[test]
fn cursor_movement() {
    let mut t = term(10, 5);
    t.feed(b"\x1b[3;4HX"); // CUP row 3 col 4 (1-based)
    assert_eq!(t.grid().cell(2, 3).ch, 'X');
    t.feed(b"\x1b[2AY"); // CUU 2
    assert_eq!(t.grid().cell(0, 4).ch, 'Y');
    t.feed(b"\x1b[3BZ"); // CUD 3
    assert_eq!(t.grid().cell(3, 5).ch, 'Z');
    t.feed(b"\x1b[4DW"); // CUB 4
    assert_eq!(t.grid().cell(3, 2).ch, 'W');
    t.feed(b"\x1b[2CV"); // CUF 2
    assert_eq!(t.grid().cell(3, 5).ch, 'V');
    t.feed(b"\x1b[1GU"); // CHA
    assert_eq!(t.grid().cell(3, 0).ch, 'U');
    t.feed(b"\x1b[5dT"); // VPA
    assert_eq!(t.grid().cell(4, 1).ch, 'T');
    t.feed(b"\x1b[99;99H");
    assert_eq!((t.cursor().row, t.cursor().col), (4, 9));
    t.feed(b"\x1b[H");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 0));
    // Save/restore cursor (DECSC / DECRC)
    t.feed(b"\x1b[2;2H\x1b7\x1b[5;5H\x1b8S");
    assert_eq!(t.grid().cell(1, 1).ch, 'S');
    // Backspace and tab
    t.feed(b"\r\x1b[3;1Hab\x08c\tT");
    // HT renders as blanks; existing cells it skips over are kept.
    assert_eq!(row(&t, 2), "ac X    T");
}

#[test]
fn erase_in_line_and_display() {
    let mut t = term(10, 3);
    t.feed(b"0123456789\r\nabcdefghij\r\nABCDEFGHIJ");
    t.feed(b"\x1b[1;5H\x1b[K"); // EL 0
    assert_eq!(row(&t, 0), "0123");
    t.feed(b"\x1b[2;5H\x1b[1K"); // EL 1 (inclusive)
    assert_eq!(row(&t, 1), "     fghij");
    t.feed(b"\x1b[3;1H\x1b[2K"); // EL 2
    assert_eq!(row(&t, 2), "");
    t.feed(b"\x1b[3;1HABCDEFGHIJ\x1b[2;3H\x1b[J"); // ED 0
    assert_eq!(screen(&t), "0123\n\n\n");
    t.feed(b"\x1b[1;1Hxxxx\x1b[2;1Hyyyy\x1b[1;2H\x1b[1J"); // ED 1
    assert_eq!(screen(&t), "  xx\nyyyy\n\n"); // ED 1 is inclusive of the cursor
    t.feed(b"\x1b[2J"); // ED 2
    assert_eq!(screen(&t), "\n\n\n");
    // ECH / DCH / ICH
    t.feed(b"\x1b[Habcdef\x1b[1;2H\x1b[2X");
    assert_eq!(row(&t, 0), "a  def");
    t.feed(b"\x1b[1;2H\x1b[2P");
    assert_eq!(row(&t, 0), "adef");
    t.feed(b"\x1b[1;2H\x1b[3@");
    assert_eq!(row(&t, 0), "a   def");
}

#[test]
fn erase_uses_current_background() {
    let mut t = term(5, 1);
    t.feed(b"\x1b[44m\x1b[2K");
    assert_eq!(t.grid().cell(0, 4).bg, Color::Indexed(4));
}

#[test]
fn autowrap_decawm() {
    let mut t = term(5, 3);
    t.feed(b"abcdefg");
    assert_eq!(screen(&t), "abcde\nfg\n\n");
    assert!(t.grid().line(0).is_wrapped());
    assert!(!t.grid().line(1).is_wrapped());
    // Pending-wrap: writing exactly `cols` chars leaves the cursor on the last column.
    let mut t = term(5, 3);
    t.feed(b"abcde");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 4));
    t.feed(b"\r\n");
    assert_eq!(t.cursor().row, 1);
    // DECAWM off: overwrite last column.
    let mut t = term(5, 3);
    t.feed(b"\x1b[?7labcdefg");
    assert_eq!(screen(&t), "abcdg\n\n\n");
    assert!(!t.modes().autowrap);
}

#[test]
fn scrollback_accumulates_and_is_capped() {
    let mut t = Terminal::new(10, 3, 5);
    for i in 0..10 {
        t.feed(format!("line{i}\r\n").as_bytes());
    }
    let g = t.grid();
    assert_eq!(g.history_len(), 5);
    assert_eq!(g.line(-1).text().trim_end(), "line7");
    assert_eq!(g.line(-5).text().trim_end(), "line3");
    assert_eq!(screen(&t), "line8\nline9\n\n");
    // Viewport scrolled 2 lines into history.
    let rows: Vec<String> = g
        .viewport(2)
        .map(|l| l.text().trim_end().to_owned())
        .collect();
    assert_eq!(rows, ["line6", "line7", "line8"]);
    // Scroll is clamped to history length.
    assert_eq!(g.viewport_line(100, 0), -5);
    t.clear_scrollback();
    assert_eq!(t.grid().history_len(), 0);
}

#[test]
fn scroll_region_decstbm() {
    let mut t = term(5, 5);
    t.feed(b"1\r\n2\r\n3\r\n4\r\n5");
    t.feed(b"\x1b[2;4r"); // region rows 2-4
    t.feed(b"\x1b[4;1H\nX"); // LF at bottom of region scrolls region only
    assert_eq!(screen(&t), "1\n3\n4\nX\n5\n");
    // Lines scrolled out of a partial region must not enter scrollback.
    assert_eq!(t.grid().history_len(), 0);
    t.feed(b"\x1b[2;1H\x1bM"); // RI at top of region scrolls down
    assert_eq!(screen(&t), "1\n\n3\n4\n5\n");
    t.feed(b"\x1b[2;1H\x1b[L"); // IL
    assert_eq!(screen(&t), "1\n\n\n3\n5\n");
    t.feed(b"\x1b[2;1H\x1b[2M"); // DL
    assert_eq!(screen(&t), "1\n3\n\n\n5\n");
    t.feed(b"\x1b[r\x1b[H\x1b[2S"); // reset region, SU 2
    assert_eq!(screen(&t), "\n\n5\n\n\n");
}

#[test]
fn scroll_up_su_full_screen() {
    let mut t = term(5, 4);
    t.feed(b"a\r\nb\r\nc\r\nd\x1b[2S");
    assert_eq!(screen(&t), "c\nd\n\n\n");
    t.feed(b"\x1b[1T");
    assert_eq!(screen(&t), "\nc\nd\n\n");
}

#[test]
fn alt_screen_1049() {
    let mut t = term(10, 3);
    t.feed(b"shell$ ");
    t.feed(b"\x1b[?1049h");
    assert!(t.modes().alt_screen && t.grid().is_alt_screen());
    assert_eq!(screen(&t), "\n\n\n");
    t.feed(b"\x1b[Hvim");
    assert_eq!(row(&t, 0), "vim");
    t.feed(b"\x1b[?1049l");
    assert!(!t.modes().alt_screen);
    assert_eq!(row(&t, 0), "shell$");
    assert_eq!((t.cursor().row, t.cursor().col), (0, 7));
}

#[test]
fn alt_screen_has_no_scrollback() {
    let mut t = term(10, 2);
    t.feed(b"\x1b[?1049h");
    for i in 0..5 {
        t.feed(format!("{i}\r\n").as_bytes());
    }
    assert_eq!(t.grid().history_len(), 0);
}

#[test]
fn wide_chars() {
    let mut t = term(6, 2);
    t.feed("a中b".as_bytes());
    let g = t.grid();
    assert_eq!(g.cell(0, 1).ch, '中');
    assert!(g.cell(0, 1).is_wide());
    assert_eq!(g.cell(0, 2).width, CellWidth::WideSpacer);
    assert_eq!(g.cell(0, 3).ch, 'b');
    assert_eq!(g.line(0).text().trim_end(), "a中b");
    // Wide char that doesn't fit wraps, leaving a leading spacer.
    let mut t = term(4, 2);
    t.feed("abc中".as_bytes());
    assert_eq!(t.grid().cell(0, 3).width, CellWidth::LeadingSpacer);
    assert!(t.grid().cell(1, 0).is_wide());
    // Cursor reports wide when on a wide char.
    let mut t = term(6, 2);
    t.feed("中\x1b[1G".as_bytes());
    assert!(t.cursor().wide);
}

#[test]
fn combining_characters() {
    let mut t = term(5, 1);
    t.feed("e\u{301}x".as_bytes());
    let c = t.grid().cell(0, 0);
    assert_eq!(c.ch, 'e');
    assert_eq!(c.combining, &['\u{301}']);
    assert_eq!(c.text(), "e\u{301}");
    assert_eq!(t.grid().cell(0, 1).ch, 'x');
}

#[test]
fn split_utf8_and_escape_across_feeds() {
    let mut t = term(10, 1);
    let bytes = "\x1b[31mé中".as_bytes();
    for b in bytes {
        t.feed(std::slice::from_ref(b));
    }
    assert_eq!(t.grid().cell(0, 0).ch, 'é');
    assert_eq!(t.grid().cell(0, 0).fg, Color::Indexed(1));
    assert_eq!(t.grid().cell(0, 1).ch, '中');
}

#[test]
fn cursor_shape_and_visibility() {
    let mut t = term(5, 1);
    t.feed(b"\x1b[?25l");
    assert!(!t.cursor().visible);
    t.feed(b"\x1b[?25h\x1b[6 q");
    let c = t.cursor();
    assert!(c.visible);
    assert_eq!(c.shape, CursorShape::Beam);
    assert!(!c.blinking);
    t.feed(b"\x1b[3 q");
    assert_eq!(t.cursor().shape, CursorShape::Underline);
    assert!(t.cursor().blinking);
    t.feed(b"\x1b[0 q");
    assert_eq!(t.cursor().shape, CursorShape::Block);
}

#[test]
fn replies_dsr_cpr_and_da() {
    let mut t = term(10, 5);
    t.feed(b"\x1b[3;4H\x1b[6n");
    assert_eq!(t.take_replies(), b"\x1b[3;4R");
    t.feed(b"\x1b[5n");
    assert_eq!(t.take_replies(), b"\x1b[0n");
    t.feed(b"\x1b[c");
    assert!(t.take_replies().starts_with(b"\x1b[?"));
    assert!(t.take_replies().is_empty());
}

#[test]
fn osc_color_query_uses_palette() {
    let mut t = term(10, 1);
    t.feed(b"\x1b]11;?\x07");
    let r = String::from_utf8(t.take_replies()).unwrap();
    assert_eq!(r, "\x1b]11;rgb:0c0c/0c0c/0c0c\x07");
    t.feed(b"\x1b]4;1;rgb:ff/00/80\x07");
    assert_eq!(t.palette().colors[1], Rgb::new(0xff, 0, 0x80));
    t.feed(b"\x1b]4;1;?\x07");
    let r = String::from_utf8(t.take_replies()).unwrap();
    assert!(r.contains("4;1;rgb:ffff/0000/8080"), "{r:?}");
    t.feed(b"\x1b]104;1\x07");
    assert_eq!(t.palette().colors[1], Rgb::hex(0xC50F1F));
}

#[test]
fn title_bell_and_clipboard_events() {
    let mut t = term(10, 1);
    t.feed(b"\x1b]0;my title\x07\x07\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(t.title(), Some("my title"));
    let ev = t.take_events();
    assert!(matches!(&ev[0], TermEvent::Title(s) if s == "my title"));
    assert!(ev.iter().any(|e| matches!(e, TermEvent::Bell)));
    assert!(ev.iter().any(
        |e| matches!(e, TermEvent::ClipboardStore { primary: false, text } if text == "hello")
    ));
    assert!(t.take_events().is_empty());
}

#[test]
fn modes() {
    let mut t = term(10, 1);
    let m = t.modes();
    assert!(!m.app_cursor && !m.bracketed_paste && m.mouse == MouseMode::Off);
    t.feed(b"\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h");
    let m = t.modes();
    assert!(m.app_cursor && m.app_keypad && m.bracketed_paste && m.focus_events && m.sgr_mouse);
    assert_eq!(m.mouse, MouseMode::Drag);
    t.set_focused(true);
    assert_eq!(t.take_replies(), b"\x1b[I");
}

#[test]
fn hyperlinks() {
    let mut t = term(10, 1);
    t.feed(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\x");
    let g = t.grid();
    assert_eq!(
        g.hyperlink(Point::new(0, 0)).as_deref(),
        Some("https://example.com")
    );
    assert_eq!(g.hyperlink(Point::new(0, 4)), None);
}

#[test]
fn text_range_selection() {
    let mut t = term(5, 3);
    t.feed(b"abcdefg\r\nxy");
    let g = t.grid();
    // Soft-wrapped line joins without a newline.
    assert_eq!(g.text_range(Point::new(0, 0), Point::new(1, 4)), "abcdefg");
    assert_eq!(g.text_range(Point::new(1, 0), Point::new(2, 1)), "fg\nxy");
    // Reversed endpoints are normalised.
    assert_eq!(g.text_range(Point::new(0, 3), Point::new(0, 1)), "bcd");
}

#[test]
fn damage_tracking() {
    let mut t = term(10, 4);
    assert_eq!(t.take_damage(), Damage::Full);
    t.feed(b"\x1b[3;1Hhi");
    match t.take_damage() {
        Damage::Rows(rows) => {
            assert!(
                rows.iter()
                    .any(|r| r.row == 2 && r.left == 0 && r.right >= 1),
                "{rows:?}"
            );
            assert!(rows.iter().all(|r| r.row == 0 || r.row == 2), "{rows:?}");
        }
        Damage::Full => panic!("expected partial damage"),
    }
    match t.take_damage() {
        Damage::Rows(rows) => assert!(rows.is_empty() || rows.iter().all(|r| r.row == 2)),
        Damage::Full => panic!("expected partial damage"),
    }
    t.resize(20, 4);
    assert_eq!(t.take_damage(), Damage::Full);
}

#[test]
fn resize_reflows_primary_screen() {
    let mut t = term(10, 3);
    t.feed(b"abcdefghij12\r\nxy");
    assert_eq!(screen(&t), "abcdefghij\n12\nxy\n");
    // Narrower: wrapped line re-splits, top lines move to scrollback.
    t.resize(5, 3);
    assert_eq!(screen(&t), "fghij\n12\nxy\n");
    assert_eq!(t.grid().line(-1).text(), "abcde");
    // Wider again: re-joins, pulling history back.
    t.resize(12, 3);
    assert_eq!(screen(&t), "abcdefghij12\nxy\n\n");
    assert_eq!(t.grid().history_len(), 0);
    assert_eq!((t.cols(), t.rows()), (12, 3));
    // Cursor follows its content.
    assert_eq!((t.cursor().row, t.cursor().col), (1, 2));
}

#[test]
fn resize_rows_moves_lines_into_scrollback() {
    let mut t = term(10, 4);
    t.feed(b"1\r\n2\r\n3\r\n4");
    t.resize(10, 2);
    assert_eq!(screen(&t), "3\n4\n");
    assert_eq!(t.grid().history_len(), 2);
    t.resize(10, 4);
    assert_eq!(screen(&t), "1\n2\n3\n4\n");
}

#[test]
fn resize_alt_screen_truncates() {
    let mut t = term(10, 2);
    t.feed(b"\x1b[?1049habcdefghij");
    t.resize(5, 2);
    assert_eq!(screen(&t), "abcde\n\n");
}

#[test]
fn zero_size_is_clamped() {
    let mut t = Terminal::new(0, 0, 0);
    t.feed(b"abc\r\n");
    t.resize(0, 0);
    assert_eq!((t.cols(), t.rows()), (1, 1));
}

#[test]
fn ris_resets_state() {
    let mut t = term(10, 2);
    t.feed(b"\x1b[31mhello\x1b[?25l\x1bc");
    assert_eq!(screen(&t), "\n\n");
    assert!(t.cursor().visible);
    t.feed(b"x");
    assert_eq!(t.grid().cell(0, 0).fg, Color::Default);
}

#[test]
fn palette_resolution() {
    let p = Palette::default();
    assert_eq!(
        p.resolve(Color::Indexed(1), ColorRole::Foreground),
        Rgb::hex(0xC50F1F)
    );
    assert_eq!(
        p.resolve(Color::Indexed(16), ColorRole::Foreground),
        Rgb::new(0, 0, 0)
    );
    assert_eq!(
        p.resolve(Color::Indexed(231), ColorRole::Foreground),
        Rgb::new(255, 255, 255)
    );
    assert_eq!(
        p.resolve(Color::Indexed(232), ColorRole::Foreground),
        Rgb::new(8, 8, 8)
    );
    assert_eq!(
        p.resolve(Color::Default, ColorRole::Background),
        p.background
    );
    assert_eq!(
        p.resolve(Color::Rgb(Rgb::new(1, 2, 3)), ColorRole::Background),
        Rgb::new(1, 2, 3)
    );
}

#[test]
fn real_world_ls_color_output() {
    // `ls --color` style output with a trailing reset and CRLF.
    let mut t = term(40, 3);
    t.feed(b"\x1b[0m\x1b[01;34msrc\x1b[0m  \x1b[01;32mrun.sh\x1b[0m\r\n");
    let g = t.grid();
    assert_eq!(row(&t, 0), "src  run.sh");
    assert_eq!(g.cell(0, 0).fg, Color::Indexed(4));
    assert!(g.cell(0, 0).attrs.contains(Attrs::BOLD));
    assert_eq!(g.cell(0, 3).fg, Color::Default);
    assert_eq!(g.cell(0, 5).fg, Color::Indexed(2));
}

#[test]
fn random_bytes_never_panic() {
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut t = Terminal::new(17, 7, 50);
    for round in 0..200 {
        let len = (next() % 512) as usize;
        let mut buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Bias towards escape sequences.
        for b in buf.iter_mut().step_by(7) {
            *b = 0x1b;
        }
        t.feed(&buf);
        if round % 37 == 0 {
            t.resize((next() % 30 + 1) as u16, (next() % 10 + 1) as u16);
        }
        let g = t.grid();
        let c = t.cursor();
        assert!(c.row < g.rows() && c.col < g.cols());
        for l in g.line_range() {
            let _ = g.line(l).text();
        }
        let _ = t.take_damage();
        let _ = t.take_replies();
        let _ = t.take_events();
    }
}

#[test]
fn output_of_real_printf() {
    // Feed what the real `printf` binary emits, not a hand-written literal.
    let out = std::process::Command::new("printf")
        .arg(r"a\033[31mB\033[1;4mC\033[0m\r\n\033[7mrev\033[27m")
        .output();
    let Ok(out) = out else { return }; // printf missing: nothing to check
    let mut t = term(10, 2);
    t.feed(&out.stdout);
    let g = t.grid();
    assert_eq!(screen(&t), "aBC\nrev\n");
    assert_eq!(g.cell(0, 1).fg, Color::Indexed(1));
    assert_eq!(g.cell(0, 2).attrs, Attrs::BOLD | Attrs::UNDERLINE);
    assert_eq!(g.cell(1, 0).attrs, Attrs::INVERSE);
}
