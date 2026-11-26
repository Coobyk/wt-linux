//! Table-driven keymap: `DEFAULT_BINDINGS` (action, chord) pairs, overridden per action by
//! the config's `[keybindings]` table. Chords use Rune's `Chord` syntax ("ctrl+shift+t").
use std::collections::BTreeMap;

use rune::{Chord, KeyEvent, Tx};

use crate::action::Action;
use crate::config::KeySpec;

/// Windows Terminal's defaults where it has one.
pub const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("new_tab", "ctrl+shift+t"),
    ("close_tab", "ctrl+shift+f4"),
    ("rename_tab", "ctrl+shift+r"),
    ("next_tab", "ctrl+tab"),
    ("prev_tab", "ctrl+shift+tab"),
    ("split_vertical", "alt+shift+="),
    ("split_horizontal", "alt+shift+-"),
    ("close_pane", "ctrl+shift+w"),
    ("focus_next_pane", "alt+right"),
    ("focus_prev_pane", "alt+left"),
    ("open_palette", "ctrl+shift+p"),
    ("open_settings", "ctrl+comma"),
    ("font_size_increase", "ctrl+="),
    ("font_size_decrease", "ctrl+-"),
    ("font_size_reset", "ctrl+0"),
    ("switch_to_tab:1", "ctrl+alt+1"),
    ("switch_to_tab:2", "ctrl+alt+2"),
    ("switch_to_tab:3", "ctrl+alt+3"),
    ("switch_to_tab:4", "ctrl+alt+4"),
    ("switch_to_tab:5", "ctrl+alt+5"),
    ("new_tab_profile:1", "ctrl+shift+1"),
    ("new_tab_profile:2", "ctrl+shift+2"),
    ("new_tab_profile:3", "ctrl+shift+3"),
];

#[derive(Clone, Debug)]
pub struct Binding {
    pub chord: Chord,
    /// The chord as written, for display.
    pub spec: String,
    pub action: Action,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Keymap {
    pub fn defaults() -> Keymap {
        let mut k = Keymap::default();
        for (action, key) in DEFAULT_BINDINGS {
            let action = action.parse().expect("default action name");
            k.bind(action, key).expect("default chord");
        }
        k
    }

    /// Defaults, then each configured action's keys replace its defaults. A chord claimed by
    /// a configured action is removed from whatever it was bound to. Returns readable errors
    /// for bad entries, which are skipped.
    pub fn from_config(map: &BTreeMap<String, KeySpec>) -> (Keymap, Vec<String>) {
        let mut k = Keymap::defaults();
        let mut errors = Vec::new();
        for (name, spec) in map {
            let action: Action = match name.parse() {
                Ok(a) => a,
                Err(e) => {
                    errors.push(format!("keybindings: {e}"));
                    continue;
                }
            };
            k.bindings.retain(|b| b.action != action);
            for key in spec.keys() {
                if let Err(e) = k.bind(action, key) {
                    errors.push(format!("keybindings.{name}: {e}"));
                }
            }
        }
        (k, errors)
    }

    /// Binds `key` to `action`, taking the chord from any other action.
    pub fn bind(&mut self, action: Action, key: &str) -> Result<(), rune::ChordError> {
        let chord = Chord::parse(&canonical(key))?;
        self.bindings.retain(|b| b.chord != chord);
        self.bindings.push(Binding {
            chord,
            spec: key.trim().to_string(),
            action,
        });
        Ok(())
    }

    /// The action a key press triggers. With `tx`, matching follows the active layout.
    pub fn action_for(&self, ev: &KeyEvent, tx: Option<&Tx>) -> Option<Action> {
        self.bindings
            .iter()
            .find(|b| match tx {
                Some(tx) => b.chord.matches_tx(ev, tx),
                None => b.chord.matches(ev),
            })
            .map(|b| b.action)
    }

    pub fn keys_for(&self, action: Action) -> Vec<&str> {
        self.bindings
            .iter()
            .filter(|b| b.action == action)
            .map(|b| b.spec.as_str())
            .collect()
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
}

/// `mod` spelled as the key it is on this platform, so `mod+t` and `ctrl+t` are one chord.
fn canonical(spec: &str) -> String {
    if cfg!(target_os = "macos") {
        return spec.to_string();
    }
    spec.split('+')
        .map(|t| {
            if t.trim().eq_ignore_ascii_case("mod") {
                "ctrl"
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// "ctrl+shift+t" -> "Ctrl+Shift+T".
pub fn pretty(spec: &str) -> String {
    let spec = spec.trim();
    let (mods, key) = match spec.strip_suffix("++") {
        Some(m) => (m, "+"),
        None => spec.rsplit_once('+').unwrap_or(("", spec)),
    };
    let cap = |p: &str| {
        let p = p.trim();
        let mut c = p.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    };
    mods.split('+')
        .filter(|m| !m.trim().is_empty())
        .map(cap)
        .chain(std::iter::once(cap(key)))
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Key, Mods};

    fn press(key: Key, ctrl: bool, shift: bool, alt: bool) -> KeyEvent {
        KeyEvent {
            key,
            mods: Mods {
                ctrl,
                cmd: ctrl,
                shift,
                alt,
                ..Mods::default()
            },
        }
    }

    #[test]
    fn defaults_cover_required_actions() {
        let k = Keymap::defaults();
        for a in [
            Action::NewTab,
            Action::CloseTab,
            Action::SplitHorizontal,
            Action::SplitVertical,
            Action::OpenPalette,
            Action::NextTab,
            Action::PrevTab,
            Action::FontSizeIncrease,
            Action::FontSizeDecrease,
        ] {
            assert!(!k.keys_for(a).is_empty(), "{a:?} unbound");
        }
    }

    #[test]
    fn matches_key_events() {
        let k = Keymap::defaults();
        assert_eq!(
            k.action_for(&press(Key::Command('t'), true, true, false), None),
            Some(Action::NewTab)
        );
        assert_eq!(
            k.action_for(&press(Key::Command('p'), true, true, false), None),
            Some(Action::OpenPalette)
        );
        assert_eq!(
            k.action_for(&press(Key::Tab, true, false, false), None),
            Some(Action::NextTab)
        );
        assert_eq!(
            k.action_for(&press(Key::Tab, true, true, false), None),
            Some(Action::PrevTab)
        );
        assert_eq!(
            k.action_for(&press(Key::Text("t".into()), false, false, false), None),
            None
        );
    }

    #[test]
    fn config_rebinds_and_unbinds() {
        let mut map = BTreeMap::new();
        map.insert(
            "new_tab".into(),
            KeySpec::Many(vec!["ctrl+n".into(), "ctrl+shift+n".into()]),
        );
        map.insert("open_palette".into(), KeySpec::One(String::new()));
        map.insert("close_pane".into(), KeySpec::One("ctrl+shift+t".into()));
        map.insert("bogus".into(), KeySpec::One("ctrl+b".into()));
        map.insert("quit".into(), KeySpec::One("ctrl+nope+x".into()));
        let (k, errors) = Keymap::from_config(&map);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(k.keys_for(Action::NewTab), vec!["ctrl+n", "ctrl+shift+n"]);
        assert!(k.keys_for(Action::OpenPalette).is_empty());
        assert_eq!(
            k.action_for(&press(Key::Command('t'), true, true, false), None),
            Some(Action::ClosePane)
        );
    }

    #[test]
    fn pretty_labels() {
        assert_eq!(pretty("ctrl+shift+t"), "Ctrl+Shift+T");
        assert_eq!(pretty("ctrl++"), "Ctrl++");
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn mod_alias_takes_the_ctrl_chord() {
        let mut k = Keymap::defaults();
        k.bind(Action::Quit, "mod+shift+t").unwrap();
        assert!(k.keys_for(Action::NewTab).is_empty());
        let ev = KeyEvent {
            key: Key::Command('t'),
            mods: Mods {
                ctrl: true,
                cmd: true,
                shift: true,
                ..Mods::default()
            },
        };
        assert_eq!(k.action_for(&ev, None), Some(Action::Quit));
    }
}
