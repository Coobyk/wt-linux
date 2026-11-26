//! Key events a terminal pane sends to its child, as xterm-style bytes.
use rune::{Key, KeyEvent};

pub fn encode_key(ev: &KeyEvent) -> Option<Vec<u8>> {
    let m = ev.mods;
    let alt = |mut b: Vec<u8>| {
        if m.alt {
            b.insert(0, 0x1b);
        }
        b
    };
    Some(match &ev.key {
        Key::Text(s) => alt(s.as_bytes().to_vec()),
        Key::Command(c) if m.ctrl || m.cmd => {
            let c = c.to_ascii_lowercase();
            match c {
                'a'..='z' => alt(vec![c as u8 - b'a' + 1]),
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                ' ' | '@' | '2' => vec![0],
                _ => return None,
            }
        }
        Key::Command(c) => alt(c.to_string().into_bytes()),
        Key::Enter => alt(b"\r".to_vec()),
        Key::Backspace => alt(vec![0x7f]),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => vec![0x1b],
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Function(n @ 1..=4) => format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes(),
        Key::Function(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][(*n - 5) as usize];
            format!("\x1b[{code}~").into_bytes()
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    #[test]
    fn encodes() {
        let k = |key, mods| encode_key(&KeyEvent { key, mods });
        assert_eq!(
            k(Key::Text("a".into()), Mods::default()),
            Some(b"a".to_vec())
        );
        assert_eq!(
            k(
                Key::Command('c'),
                Mods {
                    ctrl: true,
                    ..Mods::default()
                }
            ),
            Some(vec![3])
        );
        assert_eq!(
            k(
                Key::Text("b".into()),
                Mods {
                    alt: true,
                    ..Mods::default()
                }
            ),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(k(Key::Up, Mods::default()), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            k(Key::Function(5), Mods::default()),
            Some(b"\x1b[15~".to_vec())
        );
    }
}
