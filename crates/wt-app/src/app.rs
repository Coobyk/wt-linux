//! App state: tabs, panes, modes, and `dispatch`, the one place actions take effect.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rune::{Family, KeyEvent, TextEdit, TextStyle, Tx, Waker};

use crate::action::{Action, SIMPLE};
use crate::config::{Config, Profile, Theme};
use crate::input::encode_key;
use crate::keymap::{Keymap, pretty};
use crate::layout::{Layout, PaneId, SplitDir};
use crate::palette::{Palette, PaletteItem, filter};
use crate::terminal::{Notify, PaneTerminal, SpawnSpec, TerminalFactory, fake_factory};

pub const INITIAL_COLS: u16 = 80;
/// Palette rows shown; matches past this are neither drawn nor selectable.
pub const PALETTE_ROWS: usize = 12;
pub const INITIAL_ROWS: u16 = 24;

pub struct Pane {
    /// Index into `Config::profiles`.
    pub profile: usize,
    pub term: Box<dyn PaneTerminal>,
    /// The content box the pane was last fitted to, for refitting when the font changes.
    pub fit: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TabKind {
    Terminal { layout: Layout, focused: PaneId },
    Settings,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: u32,
    /// Set by renaming; overrides the child's title.
    pub custom_title: Option<String>,
    pub kind: TabKind,
}

/// The one modal thing the shell is doing.
#[derive(Clone, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Palette(Palette),
    Rename {
        tab: u32,
        edit: TextEdit,
    },
    ProfileMenu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Startup,
    Appearance,
    Profiles,
    ColorSchemes,
    Actions,
}

pub const SETTINGS_SECTIONS: &[(SettingsSection, &str)] = &[
    (SettingsSection::Startup, "Startup"),
    (SettingsSection::Appearance, "Appearance"),
    (SettingsSection::Profiles, "Profiles"),
    (SettingsSection::ColorSchemes, "Color schemes"),
    (SettingsSection::Actions, "Actions"),
];

/// What an action asks of the host after mutating state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Focus(&'static str),
    Blur,
    Quit,
}

pub struct State {
    pub config: Config,
    pub config_path: Option<PathBuf>,
    pub config_errors: Vec<String>,
    pub keymap: Keymap,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub panes: BTreeMap<PaneId, Pane>,
    pub mode: Mode,
    pub settings_section: SettingsSection,
    /// Added to every profile's font size by the font size actions.
    pub font_delta: f32,
    /// A split's ratio when its divider drag began: `(split id, ratio)`.
    pub drag: Option<(u32, f32)>,
    /// Last save result, shown in settings.
    pub status: Option<String>,
    next_id: u32,
    factory: TerminalFactory,
    /// Rune's waker, installed by `spec.setup`; terminals reach it through `notify`.
    waker: Arc<Mutex<Option<Waker>>>,
    /// Set when a pane wakes before `setup` ran; `setup` then wakes once itself.
    pending_wake: Arc<AtomicBool>,
    notify: Notify,
    scratch: TextEdit,
}

impl State {
    /// A shell over `FakeTerminal`s with one tab of the default profile.
    pub fn new(config: Config) -> State {
        State::with_factory(config, None, fake_factory())
    }

    pub fn with_factory(
        config: Config,
        config_path: Option<PathBuf>,
        factory: TerminalFactory,
    ) -> State {
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let pending_wake = Arc::new(AtomicBool::new(false));
        let (slot, pending) = (waker.clone(), pending_wake.clone());
        let notify: Notify = Arc::new(move || {
            if let Some(w) = slot.lock().unwrap().as_ref() {
                w();
            } else {
                pending.store(true, Ordering::SeqCst);
            }
        });
        let (keymap, config_errors) = Keymap::from_config(&config.keybindings);
        let mut s = State {
            config,
            config_path,
            config_errors,
            keymap,
            tabs: Vec::new(),
            active: 0,
            panes: BTreeMap::new(),
            mode: Mode::Normal,
            settings_section: SettingsSection::default(),
            font_delta: 0.0,
            drag: None,
            status: None,
            next_id: 1,
            factory,
            waker,
            pending_wake,
            notify,
            scratch: TextEdit::default(),
        };
        let p = s.config.default_profile_index();
        s.new_tab(p);
        s
    }

    /// `spec.setup` hands Rune's waker to every terminal spawned after this. A wake that
    /// arrived earlier (the first tab spawns before setup) fired `pending_wake`; flush it.
    pub fn set_waker(&mut self, waker: Waker) {
        *self.waker.lock().unwrap() = Some(waker.clone());
        if self.pending_wake.swap(false, Ordering::SeqCst) {
            waker();
        }
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// The focused pane of the active tab, if it's a terminal tab.
    pub fn focused_pane(&self) -> Option<PaneId> {
        match self.active_tab()?.kind {
            TabKind::Terminal { focused, .. } => Some(focused),
            TabKind::Settings => None,
        }
    }

    pub fn profile(&self, i: usize) -> &Profile {
        &self.config.profiles[i.min(self.config.profiles.len() - 1)]
    }

    pub fn tab_title(&self, i: usize) -> String {
        let tab = &self.tabs[i];
        if let Some(t) = &tab.custom_title {
            return t.clone();
        }
        match &tab.kind {
            TabKind::Settings => "Settings".into(),
            TabKind::Terminal { focused, .. } => match self.panes.get(focused) {
                Some(p) => p
                    .term
                    .title()
                    .unwrap_or_else(|| self.profile(p.profile).name.clone()),
                None => "Terminal".into(),
            },
        }
    }

    pub fn font_size(&self, profile: usize) -> f32 {
        (self.config.font_for(self.profile(profile)).size + self.font_delta).clamp(6.0, 72.0)
    }

    /// The monospace text style a pane with this profile draws in.
    pub fn text_style(&self, profile: usize) -> TextStyle {
        let font = self.config.font_for(self.profile(profile));
        let family = match font.family.as_str() {
            "" | "mono" => Family::Mono,
            "system-mono" => Family::SystemMono,
            other => Family::Named(intern(other)),
        };
        TextStyle::new(self.font_size(profile), 400.0).family(family)
    }

    pub fn dark(&self) -> bool {
        self.config.theme == Theme::Dark
    }

    fn spawn_pane(&mut self, profile: usize) -> PaneId {
        let p = self.profile(profile);
        let spec = SpawnSpec {
            program: p.resolved_program(),
            args: p.args.clone(),
            cwd: p.resolved_cwd(),
            env: p.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            cols: INITIAL_COLS,
            rows: INITIAL_ROWS,
            profile: p.name.clone(),
        };
        let term = (self.factory)(&spec, self.notify.clone());
        let id = self.id();
        self.panes.insert(
            id,
            Pane {
                profile,
                term,
                fit: None,
            },
        );
        id
    }

    pub fn new_tab(&mut self, profile: usize) {
        let pane = self.spawn_pane(profile);
        let id = self.id();
        self.tabs.push(Tab {
            id,
            custom_title: None,
            kind: TabKind::Terminal {
                layout: Layout::Pane(pane),
                focused: pane,
            },
        });
        self.active = self.tabs.len() - 1;
    }

    /// Closes the tab at `i` and its panes. Returns `Effect::Quit` when it was the last.
    pub fn close_tab(&mut self, i: usize) -> Effect {
        if i >= self.tabs.len() {
            return Effect::None;
        }
        let tab = self.tabs.remove(i);
        if let TabKind::Terminal { layout, .. } = tab.kind {
            for p in layout.panes() {
                self.panes.remove(&p);
            }
        }
        if matches!(&self.mode, Mode::Rename { tab: t, .. } if *t == tab.id) {
            self.mode = Mode::Normal;
        }
        if self.tabs.is_empty() {
            self.active = 0;
            return Effect::Quit;
        }
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        Effect::None
    }

    pub fn close_tab_id(&mut self, id: u32) -> Effect {
        match self.tabs.iter().position(|t| t.id == id) {
            Some(i) => self.close_tab(i),
            None => Effect::None,
        }
    }

    fn split(&mut self, dir: SplitDir) {
        let Some(target) = self.focused_pane() else {
            return;
        };
        let profile = self.panes.get(&target).map_or(0, |p| p.profile);
        let new = self.spawn_pane(profile);
        let split_id = self.id();
        if let Some(Tab {
            kind: TabKind::Terminal { layout, focused },
            ..
        }) = self.tabs.get_mut(self.active)
        {
            layout.split(target, dir, new, split_id);
            *focused = new;
        }
    }

    fn close_pane(&mut self) -> Effect {
        let Some(Tab { kind, .. }) = self.tabs.get_mut(self.active) else {
            return Effect::None;
        };
        let TabKind::Terminal { layout, focused } = kind else {
            return self.close_tab(self.active);
        };
        let target = *focused;
        if layout.panes().len() == 1 {
            return self.close_tab(self.active);
        }
        let next = layout.cycle(target, -1);
        layout.remove(target);
        *focused = if layout.contains(next) {
            next
        } else {
            layout.panes()[0]
        };
        self.panes.remove(&target);
        Effect::None
    }

    pub fn focus_pane(&mut self, pane: PaneId) {
        if let Some(Tab {
            kind: TabKind::Terminal { layout, focused },
            ..
        }) = self.tabs.get_mut(self.active)
            && layout.contains(pane)
        {
            *focused = pane;
        }
    }

    fn cycle_pane(&mut self, step: isize) {
        if let Some(Tab {
            kind: TabKind::Terminal { layout, focused },
            ..
        }) = self.tabs.get_mut(self.active)
        {
            *focused = layout.cycle(*focused, step);
        }
    }

    pub fn activate(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
        }
    }

    fn open_settings(&mut self) {
        match self.tabs.iter().position(|t| t.kind == TabKind::Settings) {
            Some(i) => self.active = i,
            None => {
                let id = self.id();
                self.tabs.push(Tab {
                    id,
                    custom_title: None,
                    kind: TabKind::Settings,
                });
                self.active = self.tabs.len() - 1;
            }
        }
    }

    pub fn start_rename(&mut self, i: usize) -> Effect {
        if i >= self.tabs.len() {
            return Effect::None;
        }
        self.active = i;
        let title = self.tab_title(i);
        let mut edit = TextEdit::new(title);
        edit.anchor = 0;
        edit.focus = edit.text.len();
        self.mode = Mode::Rename {
            tab: self.tabs[i].id,
            edit,
        };
        Effect::Focus("rename-input")
    }

    pub fn commit_rename(&mut self) -> Effect {
        if let Mode::Rename { tab, edit } = std::mem::take(&mut self.mode)
            && let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab)
        {
            let name = edit.text.trim();
            t.custom_title = (!name.is_empty()).then(|| name.to_string());
        }
        Effect::Blur
    }

    /// The text field the current mode edits (rename or palette query).
    pub fn mode_edit(&mut self) -> &mut TextEdit {
        match &mut self.mode {
            Mode::Palette(p) => &mut p.query,
            Mode::Rename { edit, .. } => edit,
            _ => &mut self.scratch,
        }
    }

    /// Everything the palette can run: simple actions, one per profile, one per tab.
    pub fn palette_items(&self) -> Vec<PaletteItem> {
        let key = |a: Action| self.keymap.keys_for(a).first().map(|k| pretty(k));
        let mut items: Vec<PaletteItem> = SIMPLE
            .iter()
            .map(|&a| PaletteItem {
                label: a.title(),
                action: a,
                key: key(a),
            })
            .collect();
        for (i, p) in self
            .config
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.hidden)
        {
            let a = Action::NewTabProfile(i);
            items.push(PaletteItem {
                label: format!("New tab: {}", p.name),
                action: a,
                key: key(a),
            });
        }
        for i in 0..self.tabs.len() {
            let a = Action::SwitchToTab(i);
            items.push(PaletteItem {
                label: format!("Switch to tab: {}", self.tab_title(i)),
                action: a,
                key: key(a),
            });
        }
        items
    }

    pub fn palette_matches(&self) -> Vec<PaletteItem> {
        match &self.mode {
            Mode::Palette(p) => {
                let mut m = filter(&self.palette_items(), &p.query.text);
                m.truncate(PALETTE_ROWS);
                m
            }
            _ => Vec::new(),
        }
    }

    pub fn palette_move(&mut self, step: isize) {
        let n = self.palette_matches().len();
        if let Mode::Palette(p) = &mut self.mode
            && n > 0
        {
            p.selected = (p.selected.min(n - 1) as isize + step).rem_euclid(n as isize) as usize;
        }
    }

    /// Runs the selected palette entry (or entry `index`) and closes the palette.
    pub fn palette_run(&mut self, index: Option<usize>) -> Effect {
        let matches = self.palette_matches();
        let Mode::Palette(p) = &self.mode else {
            return Effect::None;
        };
        let i = index.unwrap_or(p.selected);
        self.mode = Mode::Normal;
        match matches.get(i.min(matches.len().saturating_sub(1))) {
            Some(item) => match self.dispatch(item.action) {
                Effect::None => Effect::Blur,
                e => e,
            },
            None => Effect::Blur,
        }
    }

    pub fn dispatch(&mut self, action: Action) -> Effect {
        if !matches!(action, Action::OpenPalette)
            && matches!(self.mode, Mode::Palette(_) | Mode::ProfileMenu)
        {
            self.mode = Mode::Normal;
        }
        match action {
            Action::NewTab => self.new_tab(self.config.default_profile_index()),
            Action::NewTabProfile(i) if i < self.config.profiles.len() => self.new_tab(i),
            Action::NewTabProfile(_) => {}
            Action::CloseTab => return self.close_tab(self.active),
            Action::RenameTab => return self.start_rename(self.active),
            Action::NextTab | Action::PrevTab if !self.tabs.is_empty() => {
                let step = if action == Action::NextTab { 1 } else { -1 };
                self.active =
                    (self.active as isize + step).rem_euclid(self.tabs.len() as isize) as usize;
            }
            Action::NextTab | Action::PrevTab => {}
            Action::SwitchToTab(i) => self.activate(i),
            Action::SplitHorizontal => self.split(SplitDir::Horizontal),
            Action::SplitVertical => self.split(SplitDir::Vertical),
            Action::ClosePane => return self.close_pane(),
            Action::FocusNextPane => self.cycle_pane(1),
            Action::FocusPrevPane => self.cycle_pane(-1),
            Action::OpenPalette => {
                if matches!(self.mode, Mode::Palette(_)) {
                    self.mode = Mode::Normal;
                    return Effect::Blur;
                }
                self.mode = Mode::Palette(Palette::default());
                return Effect::Focus("palette-input");
            }
            Action::OpenSettings => self.open_settings(),
            Action::FontSizeIncrease => {
                self.font_delta = (self.font_delta + 1.0).min(48.0);
                self.refit();
            }
            Action::FontSizeDecrease => {
                self.font_delta = (self.font_delta - 1.0).max(-8.0);
                self.refit();
            }
            Action::FontSizeReset => {
                self.font_delta = 0.0;
                self.refit();
            }
            Action::Quit => return Effect::Quit,
        }
        Effect::None
    }

    /// Keymap first; otherwise, in normal mode, the key goes to the focused pane.
    pub fn handle_key(&mut self, ev: &KeyEvent, tx: Option<&Tx>) -> (bool, Effect) {
        if let Some(a) = self.keymap.action_for(ev, tx) {
            return (true, self.dispatch(a));
        }
        if !matches!(self.mode, Mode::Normal) {
            return (false, Effect::None);
        }
        let (Some(pane), Some(bytes)) = (self.focused_pane(), encode_key(ev)) else {
            return (false, Effect::None);
        };
        if let Some(p) = self.panes.get_mut(&pane) {
            p.term.write(&bytes);
        }
        (true, Effect::None)
    }

    /// Resizes a pane's terminal to fit `w` x `h` points of its content box.
    pub fn fit_pane(&mut self, pane: PaneId, w: f32, h: f32) {
        let Some(p) = self.panes.get(&pane) else {
            return;
        };
        let (cw, lh) = cell_size(self.font_size(p.profile));
        let cols = ((w / cw).floor() as u16).max(2);
        let rows = ((h / lh).floor() as u16).max(1);
        if let Some(p) = self.panes.get_mut(&pane) {
            p.fit = Some((w, h));
            p.term.resize(cols, rows);
        }
    }

    /// Refits every laid-out pane, after the font size changes.
    pub fn refit(&mut self) {
        let fits: Vec<_> = self
            .panes
            .iter()
            .filter_map(|(id, p)| p.fit.map(|(w, h)| (*id, w, h)))
            .collect();
        for (id, w, h) in fits {
            self.fit_pane(id, w, h);
        }
    }

    pub fn poll_terminals(&mut self) -> bool {
        let mut changed = false;
        for p in self.panes.values_mut() {
            changed |= p.term.poll();
        }
        changed
    }
}

/// Monospace cell `(width, line height)` for a font size. The bundled mono face advances 0.6 em.
pub fn cell_size(font_size: f32) -> (f32, f32) {
    (font_size * 0.6, (font_size * 1.3).ceil())
}

/// Applies an `Effect` through Rune.
pub fn apply(effect: Effect, tx: &mut Tx) {
    match effect {
        Effect::None => {}
        Effect::Focus(key) => tx.focus_when_shown(key, false),
        Effect::Blur => tx.blur(),
        Effect::Quit => tx.quit(),
    }
}

pub fn run_action(s: &mut State, a: Action, tx: &mut Tx) {
    let e = s.dispatch(a);
    apply(e, tx);
}

/// Font family names live for the process; `Family::Named` needs `&'static str`.
fn intern(name: &str) -> &'static str {
    use std::sync::Mutex;
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap();
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(n);
    n
}
