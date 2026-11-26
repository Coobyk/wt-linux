//! Command palette: fuzzy search over actions, profiles and tabs.
use rune::TextEdit;

use crate::action::Action;

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub label: String,
    pub action: Action,
    /// Display form of the first bound key, if any.
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub query: TextEdit,
    pub selected: usize,
}

/// Case-insensitive subsequence match. Higher is better; `None` if `query` isn't a
/// subsequence of `label`. Rewards consecutive runs and word starts, penalises late starts.
pub fn fuzzy_score(query: &str, label: &str) -> Option<i32> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let l: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut qi = 0;
    let mut prev: Option<usize> = None;
    let mut first = None;
    for (i, &c) in l.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            score += 1;
            if prev == Some(i.wrapping_sub(1)) {
                score += 5;
            }
            if i == 0 || !l[i - 1].is_alphanumeric() {
                score += 8;
            }
            first.get_or_insert(i);
            prev = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| score - first.unwrap_or(0) as i32 / 2 - (l.len() as i32 / 16))
}

/// Items matching `query`, best first; ties keep input order.
pub fn filter(items: &[PaletteItem], query: &str) -> Vec<PaletteItem> {
    let mut scored: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| fuzzy_score(query, &it.label).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| items[i].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> PaletteItem {
        PaletteItem {
            label: label.into(),
            action: Action::NewTab,
            key: None,
        }
    }

    #[test]
    fn fuzzy_matches_subsequences() {
        assert!(fuzzy_score("spv", "Split pane vertically").is_some());
        assert!(fuzzy_score("xyz", "Split pane vertically").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("NEW", "new tab").is_some());
    }

    #[test]
    fn ranks_prefix_and_word_starts_first() {
        let items = [
            item("Close pane"),
            item("Split pane horizontally"),
            item("Split pane vertically"),
            item("Settings"),
        ];
        let got = filter(&items, "split v");
        assert_eq!(got[0].label, "Split pane vertically");
        let got = filter(&items, "set");
        assert_eq!(got[0].label, "Settings");
        assert_eq!(filter(&items, "").len(), 4);
        assert!(filter(&items, "qqq").is_empty());
    }
}
