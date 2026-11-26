//! Chord strings such as `ctrl+shift+t`, as written in `[keybindings]`.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
    pub key: ChordKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A printable key, lowercased (`t`, `0`, `,`, `+`, `-`).
    Char(char),
    Named(&'static str),
    Function(u8),
}

const NAMED: &[&str] = &[
    "tab",
    "enter",
    "escape",
    "space",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
];

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let s = s.trim().to_ascii_lowercase();
        if s.is_empty() {
            return Err("empty chord".into());
        }
        let parts: Vec<&str> = s.split('+').collect();
        // "ctrl++" means ctrl and the plus key.
        let (mods, key) = if s.ends_with("++") {
            (&parts[..parts.len() - 2], "+")
        } else {
            (&parts[..parts.len() - 1], parts[parts.len() - 1])
        };
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
            key: ChordKey::Char(' '),
        };
        for m in mods {
            match *m {
                "ctrl" | "control" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                "super" | "win" | "logo" | "meta" => chord.logo = true,
                other => return Err(format!("unknown modifier {other:?} in {s:?}")),
            }
        }
        chord.key = match key {
            "plus" => ChordKey::Char('+'),
            "minus" => ChordKey::Char('-'),
            "comma" => ChordKey::Char(','),
            "period" => ChordKey::Char('.'),
            "space" => ChordKey::Named("space"),
            "esc" => ChordKey::Named("escape"),
            "return" => ChordKey::Named("enter"),
            "pgup" => ChordKey::Named("pageup"),
            "pgdn" => ChordKey::Named("pagedown"),
            k if NAMED.contains(&k) => ChordKey::Named(NAMED.iter().find(|n| **n == k).unwrap()),
            k if k.len() > 1
                && k.starts_with('f')
                && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
            {
                ChordKey::Function(k[1..].parse().unwrap())
            }
            k if k.chars().count() == 1 => ChordKey::Char(k.chars().next().unwrap()),
            "" => return Err(format!("chord {s:?} has no key")),
            k => return Err(format!("unknown key {k:?} in {s:?}")),
        };
        Ok(chord)
    }
}
