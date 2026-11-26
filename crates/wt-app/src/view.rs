//! The window: tab bar, the active tab's content, and the palette / profile-menu overlays.
use rune::tok::type_;
use rune::{Align, Cursor, Cx, El, Key, KeyEvent, Phase, Side, Tx, col, el, row, text, text_input};

use crate::action::Action;
use crate::app::{Mode, State, TabKind, apply, run_action};
use crate::chrome::Chrome;
use crate::grid_view::{GridLook, grid_view};
use crate::layout::{Layout, PaneId, Split};
use crate::settings::settings_view;

pub const TAB_BAR_H: f32 = 40.0;

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let th = Chrome::of(s.dark());
    let content = match s.active_tab().map(|t| &t.kind) {
        Some(TabKind::Terminal { layout, focused }) => col()
            .key("content")
            .grow(1.0)
            .w_full()
            .min_h(0.0)
            .child(layout_view(s, cx, &th, layout, *focused)),
        Some(TabKind::Settings) => col()
            .key("content")
            .grow(1.0)
            .w_full()
            .min_h(0.0)
            .child(settings_view(s, &th)),
        None => col().key("content").grow(1.0),
    };
    let mut root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(th.window)
        .on_key_preview(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| keymap_only(s, ev, tx))
        .child(tab_bar(s, &th))
        .child(content);
    match &s.mode {
        Mode::Palette(_) => root = root.child(palette_view(s, cx, &th)),
        Mode::ProfileMenu => root = root.child(profile_menu(s, &th)),
        _ => {}
    }
    root
}

/// Keymap bindings, even while a text field has focus.
fn keymap_only(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match s.keymap.action_for(ev, Some(tx)) {
        Some(a) => {
            run_action(s, a, tx);
            true
        }
        None => false,
    }
}

/// `AppSpec::on_key`: keymap, then typing into the focused pane.
pub fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let (handled, effect) = s.handle_key(ev, Some(tx));
    apply(effect, tx);
    handled
}

/// `AppSpec::poll`.
pub fn poll(s: &mut State, _: f64, _: &mut Tx) -> bool {
    s.poll_terminals()
}

fn icon_button(key: impl Into<String>, label: &str, th: &Chrome) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .hover_fill(th.tab_hover)
        .child(text(label).font(type_::BODY).color(th.text))
}

fn tab_bar(s: &State, th: &Chrome) -> El {
    let mut bar = row()
        .key("tab-bar")
        .w_full()
        .h(TAB_BAR_H)
        .fill(th.tab_bar)
        .px(8.0)
        .pt(6.0)
        .gap(2.0)
        .items_end();
    for (i, tab) in s.tabs.iter().enumerate() {
        let id = tab.id;
        let active = i == s.active;
        let title: El = match &s.mode {
            Mode::Rename { tab: t, edit } if *t == id => text_input(edit, type_::BODY)
                .key("rename-input")
                .label("Tab name")
                .color(th.text)
                .w(140.0)
                .h(24.0)
                .px(6.0)
                .radius(4.0)
                .fill(th.input)
                .border(1.0, th.accent)
                .on_edit(|s: &mut State| s.mode_edit())
                .on_submit(|s: &mut State, tx: &mut Tx| apply(s.commit_rename(), tx))
                .on_input_key(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
                    if ev.key == Key::Escape {
                        s.mode = Mode::Normal;
                        tx.blur();
                        return true;
                    }
                    false
                }),
            _ => text(s.tab_title(i))
                .font(type_::BODY)
                .color(if active { th.text } else { th.text_dim })
                .nowrap()
                .truncate()
                .max_w(180.0),
        };
        let close = el()
            .key(format!("tab-{id}-close"))
            .role("button")
            .label("Close tab")
            .size(20.0, 20.0)
            .center()
            .radius(4.0)
            .hover_fill(th.tab_hover)
            .on_press(move |s: &mut State| {
                s.close_tab_id(id);
            })
            .child(text("×").font(type_::BODY).color(th.text_dim));
        let mut t = row()
            .key(format!("tab-{id}"))
            .role("tab")
            .label(s.tab_title(i))
            .h(TAB_BAR_H - 6.0)
            .min_w(120.0)
            .pl(12.0)
            .pr(6.0)
            .gap(8.0)
            .center_y()
            .justify_between()
            .corners([8.0, 8.0, 0.0, 0.0])
            .on_press(move |s: &mut State| s.activate(i))
            .on_double_press(move |s: &mut State| {
                s.start_rename(i);
            })
            .child(title)
            .child(close);
        t = if active {
            t.fill(th.tab_active)
        } else {
            t.hover_fill(th.tab_hover)
        };
        bar = bar.child(t);
    }
    bar.child(
        row()
            .h(TAB_BAR_H - 6.0)
            .center_y()
            .gap(2.0)
            .pl(4.0)
            .child(
                icon_button("new-tab", "+", th)
                    .on_press_tx(|s: &mut State, tx: &mut Tx| run_action(s, Action::NewTab, tx)),
            )
            .child(
                icon_button("profiles-button", "⌄", th).on_press(|s: &mut State| {
                    s.mode = if matches!(s.mode, Mode::ProfileMenu) {
                        Mode::Normal
                    } else {
                        Mode::ProfileMenu
                    };
                }),
            ),
    )
}

fn profile_menu(s: &State, th: &Chrome) -> El {
    let item = |key: String, label: String, hint: Option<String>, action: Action| {
        row()
            .key(key)
            .role("menuitem")
            .label(label.clone())
            .w_full()
            .h(30.0)
            .px(10.0)
            .gap(16.0)
            .center_y()
            .justify_between()
            .radius(4.0)
            .focusable(true)
            .hover_fill(th.selected)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| run_action(s, action, tx))
            .child(text(label).font(type_::BODY).color(th.text))
            .child(
                text(hint.unwrap_or_default())
                    .font(type_::LABEL)
                    .color(th.text_dim),
            )
    };
    let mut menu = col()
        .key("profile-menu")
        .role("menu")
        .label("New tab")
        .w(260.0)
        .padding(6.0)
        .gap(2.0)
        .radius(8.0)
        .fill(th.card)
        .border(1.0, th.border)
        .shadow(6.0, 18.0, rune::Color::BLACK.alpha(0.35))
        .anchored_fit("profiles-button", Side::Bottom, Align::Start, 4.0)
        .modal(true, |s: &mut State| s.mode = Mode::Normal);
    for (i, p) in s
        .config
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.hidden)
    {
        let a = Action::NewTabProfile(i);
        menu = menu.child(item(
            format!("profile-{i}"),
            p.name.clone(),
            s.keymap
                .keys_for(a)
                .first()
                .map(|k| crate::keymap::pretty(k)),
            a,
        ));
    }
    menu.child(el().w_full().h(1.0).fill(th.border).my(4.0))
        .child(item(
            "menu-settings".into(),
            "Settings".into(),
            s.keymap
                .keys_for(Action::OpenSettings)
                .first()
                .map(|k| crate::keymap::pretty(k)),
            Action::OpenSettings,
        ))
        .child(item(
            "menu-palette".into(),
            "Command palette".into(),
            s.keymap
                .keys_for(Action::OpenPalette)
                .first()
                .map(|k| crate::keymap::pretty(k)),
            Action::OpenPalette,
        ))
}

fn layout_view(s: &State, cx: &mut Cx<State>, th: &Chrome, layout: &Layout, focused: PaneId) -> El {
    match layout {
        Layout::Pane(id) => pane_view(s, th, *id, *id == focused),
        Layout::VSplit(sp) => split_view(s, cx, th, sp, focused, true),
        Layout::HSplit(sp) => split_view(s, cx, th, sp, focused, false),
    }
}

fn split_view(
    s: &State,
    cx: &mut Cx<State>,
    th: &Chrome,
    sp: &Split,
    focused: PaneId,
    vertical: bool,
) -> El {
    let key = format!("split-{}", sp.id);
    let total = cx
        .layout_size(&key)
        .map(|[w, h]| if vertical { w } else { h })
        .unwrap_or(0.0);
    let (split_id, ratio) = (sp.id, sp.ratio);
    let a = layout_view(s, cx, th, &sp.a, focused);
    let b = layout_view(s, cx, th, &sp.b, focused);
    let wrap = |c: El, grow: f32| {
        let w = el().grow(grow).min_w(0.0).min_h(0.0);
        let w = if vertical { w.h_full() } else { w.w_full() };
        w.child(c)
    };
    let mut divider = el()
        .key(format!("divider-{split_id}"))
        .role("separator")
        .label("Resize panes")
        .fill(th.divider)
        .cursor(if vertical {
            Cursor::ResizeColumn
        } else {
            Cursor::ResizeRow
        })
        .on_drag(
            move |s: &mut State, d: &mut rune::Drag, _: &mut Tx| match d.phase {
                Phase::Down => s.drag = Some((split_id, ratio)),
                Phase::Move | Phase::Up => {
                    if let Some((id, base)) = s.drag
                        && id == split_id
                        && total > 0.0
                    {
                        let travel = if vertical { d.dx } else { d.dy };
                        if let Some(TabKind::Terminal { layout, .. }) =
                            s.tabs.get_mut(s.active).map(|t| &mut t.kind)
                        {
                            layout.set_ratio(split_id, base + travel / total);
                        }
                    }
                    if d.phase == Phase::Up {
                        s.drag = None;
                    }
                }
            },
        );
    divider = if vertical {
        divider.w(4.0).h_full()
    } else {
        divider.h(4.0).w_full()
    };
    let container = if vertical { row() } else { col() };
    container
        .key(key)
        .grow(1.0)
        .w_full()
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .child(wrap(a, ratio))
        .child(divider)
        .child(wrap(b, 1.0 - ratio))
}

fn pane_view(s: &State, th: &Chrome, id: PaneId, focused: bool) -> El {
    let Some(pane) = s.panes.get(&id) else {
        return el().key(format!("pane-{id}")).grow(1.0);
    };
    let profile = s.profile(pane.profile);
    let scheme = s.config.scheme(&profile.color_scheme);
    let look = GridLook {
        scheme: &scheme,
        style: s.text_style(pane.profile),
        focused,
    };
    let multi = matches!(
        s.active_tab().map(|t| &t.kind),
        Some(TabKind::Terminal {
            layout: Layout::VSplit(_) | Layout::HSplit(_),
            ..
        })
    );
    let border = if focused && multi {
        th.accent
    } else {
        scheme.bg().alpha(profile.opacity)
    };
    col()
        .key(format!("pane-{id}"))
        .role("group")
        .label(format!("Terminal: {}", profile.name))
        .w_full()
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .padding(4.0)
        .fill(scheme.bg().alpha(profile.opacity))
        .border(1.0, border)
        .clip_rounded(0.0)
        .cursor(Cursor::Text)
        .on_press(move |s: &mut State| s.focus_pane(id))
        .on_layout(move |s: &mut State, r: [f32; 4]| s.fit_pane(id, r[2] - 8.0, r[3] - 8.0))
        .child(grid_view(pane.term.grid(), pane.term.cursor(), &look))
}

fn palette_view(s: &State, cx: &mut Cx<State>, th: &Chrome) -> El {
    let Mode::Palette(p) = &s.mode else {
        return el();
    };
    let width = 560.0;
    let win_w = cx.layout_size("root").map_or(1100.0, |[w, _]| w);
    let matches = s.palette_matches();
    let selected = p.selected.min(matches.len().saturating_sub(1));
    let input = text_input(&p.query, type_::BODY)
        .key("palette-input")
        .label("Command palette")
        .placeholder("Type a command name", th.text_dim)
        .color(th.text)
        .w_full()
        .h(36.0)
        .px(10.0)
        .radius(6.0)
        .fill(th.input)
        .border(1.0, th.accent)
        .on_edit(|s: &mut State| s.mode_edit())
        .on_submit(|s: &mut State, tx: &mut Tx| apply(s.palette_run(None), tx))
        .on_input_key(|s: &mut State, ev: &KeyEvent, _: &mut Tx| match ev.key {
            Key::Up => {
                s.palette_move(-1);
                true
            }
            Key::Down => {
                s.palette_move(1);
                true
            }
            _ => false,
        });
    let mut list = col()
        .key("palette-list")
        .role("listbox")
        .label("Commands")
        .w_full()
        .gap(1.0);
    for (i, item) in matches.iter().take(12).enumerate() {
        let r = row()
            .key(format!("palette-item-{i}"))
            .role("option")
            .label(item.label.clone())
            .w_full()
            .h(30.0)
            .px(10.0)
            .center_y()
            .justify_between()
            .radius(4.0)
            .hover_fill(th.selected)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| apply(s.palette_run(Some(i)), tx))
            .child(
                text(item.label.clone())
                    .font(type_::BODY)
                    .color(th.text)
                    .nowrap(),
            )
            .child(
                text(item.key.clone().unwrap_or_default())
                    .font(type_::LABEL)
                    .color(th.text_dim)
                    .nowrap(),
            );
        list = list.child(if i == selected {
            r.fill(th.selected)
        } else {
            r
        });
    }
    if matches.is_empty() {
        list = list.child(
            text("No matching commands")
                .font(type_::BODY)
                .color(th.text_dim)
                .px(10.0)
                .py(6.0),
        );
    }
    col()
        .key("palette")
        .role("dialog")
        .label("Command palette")
        .w(width)
        .padding(8.0)
        .gap(6.0)
        .radius(8.0)
        .fill(th.card)
        .border(1.0, th.border)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.4))
        .absolute(((win_w - width) / 2.0).max(0.0), TAB_BAR_H + 8.0)
        .modal(true, |s: &mut State| s.mode = Mode::Normal)
        .child(input)
        .child(list)
}
