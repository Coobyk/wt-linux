//! Every user-invokable command. Keybindings, the command palette and the tab bar all
//! dispatch an `Action`; `State::dispatch` is the single place that performs them.
use std::fmt;
use std::str::FromStr;

/// A command. Config names are `snake_case`; parameterised actions use `name:N`
/// (`switch_to_tab:3`, `new_tab_profile:1`, both 1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    NewTab,
    /// New tab with the profile at this 0-based index.
    NewTabProfile(usize),
    CloseTab,
    RenameTab,
    NextTab,
    PrevTab,
    /// Activate the tab at this 0-based index.
    SwitchToTab(usize),
    /// New pane below the focused one (`HSplit`: horizontal divider).
    SplitHorizontal,
    /// New pane right of the focused one (`VSplit`: vertical divider).
    SplitVertical,
    ClosePane,
    FocusNextPane,
    FocusPrevPane,
    OpenPalette,
    OpenSettings,
    FontSizeIncrease,
    FontSizeDecrease,
    FontSizeReset,
    Quit,
}

/// Unparameterised actions, in palette order.
pub const SIMPLE: &[Action] = &[
    Action::NewTab,
    Action::CloseTab,
    Action::RenameTab,
    Action::NextTab,
    Action::PrevTab,
    Action::SplitHorizontal,
    Action::SplitVertical,
    Action::ClosePane,
    Action::FocusNextPane,
    Action::FocusPrevPane,
    Action::OpenPalette,
    Action::OpenSettings,
    Action::FontSizeIncrease,
    Action::FontSizeDecrease,
    Action::FontSizeReset,
    Action::Quit,
];

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::NewTab => "new_tab",
            Action::NewTabProfile(_) => "new_tab_profile",
            Action::CloseTab => "close_tab",
            Action::RenameTab => "rename_tab",
            Action::NextTab => "next_tab",
            Action::PrevTab => "prev_tab",
            Action::SwitchToTab(_) => "switch_to_tab",
            Action::SplitHorizontal => "split_horizontal",
            Action::SplitVertical => "split_vertical",
            Action::ClosePane => "close_pane",
            Action::FocusNextPane => "focus_next_pane",
            Action::FocusPrevPane => "focus_prev_pane",
            Action::OpenPalette => "open_palette",
            Action::OpenSettings => "open_settings",
            Action::FontSizeIncrease => "font_size_increase",
            Action::FontSizeDecrease => "font_size_decrease",
            Action::FontSizeReset => "font_size_reset",
            Action::Quit => "quit",
        }
    }

    /// Human title shown in the palette and settings.
    pub fn title(self) -> String {
        match self {
            Action::NewTab => "New tab".into(),
            Action::NewTabProfile(i) => format!("New tab with profile {}", i + 1),
            Action::CloseTab => "Close tab".into(),
            Action::RenameTab => "Rename tab".into(),
            Action::NextTab => "Next tab".into(),
            Action::PrevTab => "Previous tab".into(),
            Action::SwitchToTab(i) => format!("Switch to tab {}", i + 1),
            Action::SplitHorizontal => "Split pane horizontally".into(),
            Action::SplitVertical => "Split pane vertically".into(),
            Action::ClosePane => "Close pane".into(),
            Action::FocusNextPane => "Focus next pane".into(),
            Action::FocusPrevPane => "Focus previous pane".into(),
            Action::OpenPalette => "Command palette".into(),
            Action::OpenSettings => "Settings".into(),
            Action::FontSizeIncrease => "Increase font size".into(),
            Action::FontSizeDecrease => "Decrease font size".into(),
            Action::FontSizeReset => "Reset font size".into(),
            Action::Quit => "Quit".into(),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::NewTabProfile(i) | Action::SwitchToTab(i) => {
                write!(f, "{}:{}", self.name(), i + 1)
            }
            _ => f.write_str(self.name()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAction(pub String);

impl fmt::Display for UnknownAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown action `{}`", self.0)
    }
}

impl std::error::Error for UnknownAction {}

impl FromStr for Action {
    type Err = UnknownAction;
    fn from_str(s: &str) -> Result<Action, UnknownAction> {
        let err = || UnknownAction(s.to_string());
        let s = s.trim();
        if let Some((name, n)) = s.split_once(':') {
            let n: usize = n.trim().parse().map_err(|_| err())?;
            let i = n.checked_sub(1).ok_or_else(err)?;
            return match name.trim() {
                "switch_to_tab" => Ok(Action::SwitchToTab(i)),
                "new_tab_profile" => Ok(Action::NewTabProfile(i)),
                _ => Err(err()),
            };
        }
        SIMPLE
            .iter()
            .copied()
            .find(|a| a.name() == s)
            .ok_or_else(err)
    }
}

impl serde::Serialize for Action {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Action {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Action, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        let mut all: Vec<Action> = SIMPLE.to_vec();
        all.extend([Action::SwitchToTab(2), Action::NewTabProfile(0)]);
        for a in all {
            assert_eq!(a.to_string().parse::<Action>(), Ok(a));
        }
        assert_eq!("switch_to_tab:3".parse(), Ok(Action::SwitchToTab(2)));
        assert!("switch_to_tab:0".parse::<Action>().is_err());
        assert!("nope".parse::<Action>().is_err());
    }
}
