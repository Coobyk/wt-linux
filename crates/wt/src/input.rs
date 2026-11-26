//! Keyboard: config chords -> actions, everything else -> bytes for the pty (xterm encoding).

use rune::{Key, KeyEvent, Mods};

use crate::config::{Chord, ChordKey};

pub fn matches(c: &Chord, ev: &KeyEvent) -> bool {
    let m = ev.mods;
    if c.ctrl != m.ctrl || c.alt != m.alt || c.logo != m.logo {
        return false;
    }
    match (&c.key, &ev.key) {
        // Shifted punctuation (`+` is Shift+= on most layouts) carries Shift implicitly.
        (ChordKey::Char(ch), Key::Command(k)) => {
            ch == k && (c.shift == m.shift || (!c.shift && !ch.is_alphanumeric()))
        }
        (ChordKey::Char(ch), Key::Text(t)) => {
            let mut it = t.chars().flat_map(char::to_lowercase);
            let one = matches!((it.next(), it.next()), (Some(k), None) if k == *ch);
            // Shifted punctuation (`+` is Shift+= on most layouts) carries Shift implicitly.
            one && (c.shift == m.shift || (!c.shift && !ch.is_alphanumeric()))
        }
        (ChordKey::Function(n), Key::Function(k)) => n == k && c.shift == m.shift,
        (ChordKey::Named(n), k) => named(k) == Some(*n) && c.shift == m.shift,
        _ => false,
    }
}

fn named(k: &Key) -> Option<&'static str> {
    Some(match k {
        Key::Tab => "tab",
        Key::Enter => "enter",
        Key::Escape => "escape",
        Key::Text(t) if t == " " => "space",
        Key::Command(' ') => "space",
        Key::Backspace => "backspace",
        Key::Delete => "delete",
        Key::Insert => "insert",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Up => "up",
        Key::Down => "down",
        Key::Left => "left",
        Key::Right => "right",
        _ => return None,
    })
}

/// Every action whose chord matches, in keymap order.
pub fn actions<'a>(
    keymap: &'a [(String, Chord)],
    ev: &'a KeyEvent,
) -> impl Iterator<Item = &'a str> {
    keymap
        .iter()
        .filter(|(_, c)| matches(c, ev))
        .map(|(a, _)| a.as_str())
}

/// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
fn modifier(m: Mods) -> u8 {
    1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8
}

fn csi(final_: char, m: Mods) -> Vec<u8> {
    match modifier(m) {
        1 => format!("\x1b[{final_}"),
        n => format!("\x1b[1;{n}{final_}"),
    }
    .into_bytes()
}

fn tilde(code: u8, m: Mods) -> Vec<u8> {
    match modifier(m) {
        1 => format!("\x1b[{code}~"),
        n => format!("\x1b[{code};{n}~"),
    }
    .into_bytes()
}

fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

/// Bytes a key sends to the child, or `None` if it sends nothing.
pub fn encode(ev: &KeyEvent) -> Option<Vec<u8>> {
    let m = ev.mods;
    let alt = |mut b: Vec<u8>| {
        if m.alt {
            b.insert(0, 0x1b);
        }
        b
    };
    Some(match &ev.key {
        Key::Text(t) if m.ctrl && !m.alt && t.chars().count() == 1 => {
            let c = t.chars().next()?.to_ascii_lowercase();
            // Ctrl+Space and friends; anything else typed with Ctrl (AltGr) inserts as-is.
            ctrl_byte(c)
                .map(|b| vec![b])
                .unwrap_or_else(|| t.as_bytes().to_vec())
        }
        Key::Text(t) => alt(t.as_bytes().to_vec()),
        Key::Command(c) if m.ctrl => alt(vec![ctrl_byte(*c)?]),
        Key::Command(_) => return None,
        Key::Enter => alt(b"\r".to_vec()),
        Key::Backspace => alt(vec![if m.ctrl { 0x08 } else { 0x7f }]),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt(b"\t".to_vec()),
        Key::Escape => alt(b"\x1b".to_vec()),
        Key::Up => csi('A', m),
        Key::Down => csi('B', m),
        Key::Right => csi('C', m),
        Key::Left => csi('D', m),
        Key::Home => csi('H', m),
        Key::End => csi('F', m),
        Key::Insert => tilde(2, m),
        Key::Delete => tilde(3, m),
        Key::PageUp => tilde(5, m),
        Key::PageDown => tilde(6, m),
        Key::Function(n @ 1..=4) if modifier(m) == 1 => {
            format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes()
        }
        Key::Function(n @ 1..=4) => csi((b'P' + n - 1) as char, m),
        Key::Function(n) => tilde(
            match n {
                5 => 15,
                6..=10 => 11 + n,
                11..=14 => 12 + n,
                15..=16 => 13 + n,
                17..=20 => 14 + n,
                _ => return None,
            },
            m,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: Key, f: impl FnOnce(&mut Mods)) -> KeyEvent {
        let mut mods = Mods::default();
        f(&mut mods);
        KeyEvent { key, mods }
    }
    fn ctrl(m: &mut Mods) {
        m.ctrl = true;
        m.cmd = true;
    }

    #[test]
    fn chords_match_rune_key_events() {
        let c = Chord::parse("ctrl+shift+t").unwrap();
        assert!(matches(
            &c,
            &ev(Key::Command('t'), |m| {
                ctrl(m);
                m.shift = true;
            })
        ));
        assert!(!matches(&c, &ev(Key::Command('t'), ctrl)));
        assert!(matches(
            &Chord::parse("ctrl+tab").unwrap(),
            &ev(Key::Tab, ctrl)
        ));
        assert!(matches(
            &Chord::parse("f11").unwrap(),
            &ev(Key::Function(11), |_| {})
        ));
        assert!(matches(
            &Chord::parse("ctrl+plus").unwrap(),
            &ev(Key::Text("+".into()), |m| {
                ctrl(m);
                m.shift = true;
            })
        ));
        // Implicit Shift only goes one way: a chord that asks for Shift still needs it.
        assert!(!matches(
            &Chord::parse("ctrl+shift+minus").unwrap(),
            &ev(Key::Command('-'), ctrl)
        ));
        let keymap = vec![("new_tab".to_owned(), c)];
        assert_eq!(
            actions(
                &keymap,
                &ev(Key::Command('t'), |m| {
                    ctrl(m);
                    m.shift = true;
                })
            )
            .next(),
            Some("new_tab")
        );
        assert_eq!(
            actions(&keymap, &ev(Key::Text("t".into()), |_| {})).next(),
            None
        );
    }

    #[test]
    fn encodes_xterm_sequences() {
        fn e(k: Key, f: fn(&mut Mods)) -> Vec<u8> {
            encode(&ev(k, f)).unwrap()
        }
        assert_eq!(e(Key::Text("é".into()), |_| {}), "é".as_bytes());
        assert_eq!(e(Key::Text("x".into()), |m| m.alt = true), b"\x1bx");
        assert_eq!(e(Key::Command('c'), ctrl), [3]);
        assert_eq!(e(Key::Text(" ".into()), ctrl), [0]);
        assert_eq!(e(Key::Enter, |_| {}), b"\r");
        assert_eq!(e(Key::Backspace, |_| {}), [0x7f]);
        assert_eq!(e(Key::Up, |_| {}), b"\x1b[A");
        assert_eq!(e(Key::Left, ctrl), b"\x1b[1;5D");
        assert_eq!(e(Key::Tab, |m| m.shift = true), b"\x1b[Z");
        assert_eq!(e(Key::PageDown, |_| {}), b"\x1b[6~");
        assert_eq!(e(Key::Function(1), |_| {}), b"\x1bOP");
        assert_eq!(e(Key::Function(5), |_| {}), b"\x1b[15~");
        assert_eq!(e(Key::Function(12), |_| {}), b"\x1b[24~");
        assert_eq!(encode(&ev(Key::Command('v'), |m| m.logo = true)), None);
    }
}
