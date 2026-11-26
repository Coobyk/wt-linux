//! The settings tab: startup, appearance, profiles, colour schemes and the keymap.
use rune::tok::type_;
use rune::{El, Tx, col, el, row, text};

use crate::action::Action;
use crate::app::{SETTINGS_SECTIONS, SettingsSection, State};
use crate::chrome::Chrome;
use crate::config::Theme;
use crate::keymap::pretty;
use crate::scheme::{ColorScheme, hex_color};

fn button(key: impl Into<String>, label: impl Into<String>, on: bool, th: &Chrome) -> El {
    let label = label.into();
    let b = row()
        .key(key)
        .role("button")
        .label(label.clone())
        .h(28.0)
        .px(12.0)
        .center_y()
        .radius(6.0)
        .border(1.0, th.border)
        .hover_fill(th.tab_hover)
        .child(text(label).font(type_::BODY).color(th.text).nowrap());
    if on {
        b.fill(th.selected).border(1.0, th.accent)
    } else {
        b.fill(th.input)
    }
}

fn field(label: &str, value: El, th: &Chrome) -> El {
    row()
        .w_full()
        .min_h(36.0)
        .gap(16.0)
        .center_y()
        .child(
            text(label.to_string())
                .font(type_::BODY)
                .color(th.text_dim)
                .w(160.0),
        )
        .child(value)
}

fn value(s: impl Into<String>, th: &Chrome) -> El {
    text(s.into()).font(type_::BODY).color(th.text)
}

fn heading(s: &str, th: &Chrome) -> El {
    text(s.to_string())
        .font(type_::TITLE)
        .color(th.text)
        .pb(8.0)
}

pub fn settings_view(s: &State, th: &Chrome) -> El {
    let mut nav = col()
        .key("settings-nav")
        .role("tablist")
        .label("Settings sections")
        .w(200.0)
        .h_full()
        .padding(12.0)
        .gap(4.0)
        .fill(th.tab_bar);
    for &(sec, name) in SETTINGS_SECTIONS {
        let on = s.settings_section == sec;
        let mut item = row()
            .key(format!("settings-nav-{name}"))
            .role("tab")
            .label(name)
            .w_full()
            .h(32.0)
            .px(12.0)
            .center_y()
            .radius(6.0)
            .hover_fill(th.tab_hover)
            .on_press(move |s: &mut State| s.settings_section = sec)
            .child(
                text(name)
                    .font(type_::BODY)
                    .color(if on { th.text } else { th.text_dim }),
            );
        if on {
            item = item.fill(th.selected);
        }
        nav = nav.child(item);
    }
    let body = match s.settings_section {
        SettingsSection::Startup => startup(s, th),
        SettingsSection::Appearance => appearance(s, th),
        SettingsSection::Profiles => profiles(s, th),
        SettingsSection::ColorSchemes => schemes(s, th),
        SettingsSection::Actions => actions(s, th),
    };
    row()
        .key("settings")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(nav)
        .child(
            col()
                .key("settings-body")
                .grow(1.0)
                .h_full()
                .min_w(0.0)
                .padding(24.0)
                .gap(6.0)
                .clip_rounded(0.0)
                .child(body),
        )
}

fn startup(s: &State, th: &Chrome) -> El {
    let default = s.config.default_profile_index();
    let mut picks = row().gap(6.0).flex_wrap();
    for (i, p) in s.config.profiles.iter().enumerate() {
        let name = p.name.clone();
        picks = picks.child(
            button(
                format!("default-profile-{i}"),
                p.name.clone(),
                i == default,
                th,
            )
            .on_press(move |s: &mut State| s.config.default_profile = Some(name.clone())),
        );
    }
    let path = s
        .config_path
        .as_ref()
        .map_or("(not set)".to_string(), |p| p.display().to_string());
    let mut c = col()
        .w_full()
        .gap(4.0)
        .child(heading("Startup", th))
        .child(field("Default profile", picks, th))
        .child(field("Config file", value(path, th), th))
        .child(field(
            "Save",
            button("settings-save", "Save to config file", false, th).on_press(save),
            th,
        ));
    if let Some(st) = &s.status {
        c = c.child(text(st.clone()).font(type_::LABEL).color(th.text_dim));
    }
    for e in &s.config_errors {
        c = c.child(
            text(e.clone())
                .font(type_::LABEL)
                .color(hex_color("#e74856")),
        );
    }
    c
}

/// Writes the config off the UI thread; the frame loop never touches the file system.
fn save(s: &mut State) {
    let Some(path) = s.config_path.clone() else {
        s.status = Some("No config path; set $XDG_CONFIG_HOME or $HOME".into());
        return;
    };
    let cfg = s.config.clone();
    s.status = Some(format!("Saving to {}", path.display()));
    std::thread::spawn(move || {
        if let Err(e) = cfg.save(&path) {
            eprintln!("wt-linux: saving {}: {e:#}", path.display());
        }
    });
}

fn appearance(s: &State, th: &Chrome) -> El {
    let theme = row()
        .gap(6.0)
        .child(
            button("theme-dark", "Dark", s.config.theme == Theme::Dark, th)
                .on_press(|s: &mut State| s.config.theme = Theme::Dark),
        )
        .child(
            button("theme-light", "Light", s.config.theme == Theme::Light, th)
                .on_press(|s: &mut State| s.config.theme = Theme::Light),
        );
    let size = row()
        .gap(6.0)
        .center_y()
        .child(
            button("font-size-down", "−", false, th)
                .on_press(|s: &mut State| s.config.font.size = (s.config.font.size - 1.0).max(6.0)),
        )
        .child(
            value(format!("{} pt", s.config.font.size), th)
                .w(56.0)
                .align(rune::Align::Center),
        )
        .child(
            button("font-size-up", "+", false, th).on_press(|s: &mut State| {
                s.config.font.size = (s.config.font.size + 1.0).min(72.0)
            }),
        );
    let zoom = row()
        .gap(6.0)
        .center_y()
        .child(value(format!("{:+} pt", s.font_delta), th))
        .child(button("font-size-reset", "Reset", false, th).on_press_tx(
            |s: &mut State, tx: &mut Tx| crate::app::run_action(s, Action::FontSizeReset, tx),
        ));
    col()
        .w_full()
        .gap(4.0)
        .child(heading("Appearance", th))
        .child(field("Theme", theme, th))
        .child(field(
            "Font family",
            value(s.config.font.family.clone(), th),
            th,
        ))
        .child(field("Font size", size, th))
        .child(field("Zoom", zoom, th))
}

fn swatches(sc: &ColorScheme) -> El {
    let mut r = row().gap(2.0);
    for i in 0..16 {
        r = r.child(
            el().size(16.0, 16.0)
                .radius(3.0)
                .fill(hex_color(sc.ansi_hex(i))),
        );
    }
    r
}

fn profiles(s: &State, th: &Chrome) -> El {
    let schemes = s.config.all_schemes();
    let mut c = col().w_full().gap(12.0).child(heading("Profiles", th));
    for (i, p) in s.config.profiles.iter().enumerate() {
        let font = s.config.font_for(p);
        let cmd = std::iter::once(p.resolved_program())
            .chain(p.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        let names: Vec<String> = schemes.iter().map(|x| x.name.clone()).collect();
        let scheme_btn = button(
            format!("profile-{i}-scheme"),
            p.color_scheme.clone(),
            false,
            th,
        )
        .on_press(move |s: &mut State| {
            let cur = names
                .iter()
                .position(|n| *n == s.config.profiles[i].color_scheme)
                .map_or(0, |x| x + 1);
            s.config.profiles[i].color_scheme = names[cur % names.len()].clone();
        });
        let opacity = row()
            .gap(6.0)
            .center_y()
            .child(
                button(format!("profile-{i}-opacity-down"), "−", false, th).on_press(
                    move |s: &mut State| {
                        let o = &mut s.config.profiles[i].opacity;
                        *o = (*o - 0.05).max(0.0);
                    },
                ),
            )
            .child(value(format!("{:.0}%", p.opacity * 100.0), th).w(48.0))
            .child(
                button(format!("profile-{i}-opacity-up"), "+", false, th).on_press(
                    move |s: &mut State| {
                        let o = &mut s.config.profiles[i].opacity;
                        *o = (*o + 0.05).min(1.0);
                    },
                ),
            );
        c = c.child(
            col()
                .key(format!("profile-card-{i}"))
                .w_full()
                .padding(16.0)
                .gap(2.0)
                .radius(8.0)
                .fill(th.card)
                .border(1.0, th.border)
                .child(
                    text(p.name.clone())
                        .font(type_::SECTION)
                        .color(th.text)
                        .pb(6.0),
                )
                .child(field("Command line", value(cmd, th), th))
                .child(field(
                    "Starting directory",
                    value(p.cwd.clone().unwrap_or_else(|| "(inherit)".into()), th),
                    th,
                ))
                .child(field(
                    "Color scheme",
                    row()
                        .gap(10.0)
                        .center_y()
                        .child(scheme_btn)
                        .child(swatches(&s.config.scheme(&p.color_scheme))),
                    th,
                ))
                .child(field(
                    "Font",
                    value(format!("{} {} pt", font.family, font.size), th),
                    th,
                ))
                .child(field("Background opacity", opacity, th)),
        );
    }
    c
}

fn schemes(s: &State, th: &Chrome) -> El {
    let mut c = col().w_full().gap(8.0).child(heading("Color schemes", th));
    for (i, sc) in s.config.all_schemes().iter().enumerate() {
        c = c.child(
            row()
                .key(format!("scheme-{i}"))
                .w_full()
                .padding(12.0)
                .gap(16.0)
                .center_y()
                .radius(8.0)
                .fill(th.card)
                .border(1.0, th.border)
                .child(
                    el().size(120.0, 40.0)
                        .radius(6.0)
                        .fill(sc.bg())
                        .center()
                        .child(text("Aa $ ls").font(type_::BODY.mono()).color(sc.fg())),
                )
                .child(
                    col()
                        .gap(6.0)
                        .child(value(sc.name.clone(), th))
                        .child(swatches(sc)),
                ),
        );
    }
    c
}

fn actions(s: &State, th: &Chrome) -> El {
    let mut c = col().w_full().gap(0.0).child(heading("Actions", th));
    let mut all: Vec<Action> = crate::action::SIMPLE.to_vec();
    for b in s.keymap.bindings() {
        if !all.contains(&b.action) {
            all.push(b.action);
        }
    }
    for (i, a) in all.into_iter().enumerate() {
        let keys = s
            .keymap
            .keys_for(a)
            .iter()
            .map(|k| pretty(k))
            .collect::<Vec<_>>();
        let mut chips = row().gap(6.0);
        for k in &keys {
            chips = chips.child(
                text(k.clone())
                    .font(type_::MONO_CAPTION)
                    .color(th.text)
                    .px(6.0)
                    .py(2.0)
                    .radius(4.0)
                    .fill(th.input),
            );
        }
        if keys.is_empty() {
            chips = chips.child(text("unbound").font(type_::LABEL).color(th.text_dim));
        }
        c = c.child(
            row()
                .key(format!("binding-{i}"))
                .w_full()
                .h(32.0)
                .gap(16.0)
                .center_y()
                .justify_between()
                .child(text(a.title()).font(type_::BODY).color(th.text))
                .child(
                    row()
                        .gap(12.0)
                        .center_y()
                        .child(
                            text(a.to_string())
                                .font(type_::MONO_CAPTION)
                                .color(th.text_dim),
                        )
                        .child(chips),
                ),
        );
    }
    c
}
