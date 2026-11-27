//! Window chrome colours (tab bar, palette, settings), Windows-Terminal-like.
use rune::Color;

use crate::scheme::rgb;

#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    pub window: Color,
    pub tab_bar: Color,
    pub tab_active: Color,
    pub tab_hover: Color,
    pub caption_hover: Color,
    pub caption_close: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub border: Color,
    pub card: Color,
    pub input: Color,
    pub selected: Color,
    pub divider: Color,
}

impl Chrome {
    /// Windows Terminal's stock themes: the tab row is the dark Mica grey (#1F1F1F focused,
    /// #333 unfocused), the active tab takes the focused pane's `terminalBackground`, and
    /// inactive tabs are transparent.
    pub fn of(dark: bool) -> Chrome {
        if dark {
            Chrome {
                window: rgb(12, 12, 12),
                tab_bar: rgb(31, 31, 31),
                tab_active: rgb(12, 12, 12),
                tab_hover: Color::WHITE.alpha(0.08),
                caption_hover: Color::WHITE.alpha(0.10),
                caption_close: rgb(232, 17, 35),
                text: rgb(255, 255, 255),
                text_dim: rgb(160, 160, 160),
                accent: rgb(59, 120, 255),
                border: rgb(64, 64, 64),
                card: rgb(45, 45, 45),
                input: rgb(40, 40, 40),
                selected: Color::WHITE.alpha(0.10),
                divider: rgb(51, 51, 51),
            }
        } else {
            Chrome {
                window: rgb(250, 250, 250),
                tab_bar: rgb(232, 232, 232),
                tab_active: rgb(250, 250, 250),
                tab_hover: Color::BLACK.alpha(0.06),
                caption_hover: Color::BLACK.alpha(0.08),
                caption_close: rgb(196, 43, 28),
                text: rgb(20, 20, 20),
                text_dim: rgb(96, 96, 96),
                accent: rgb(0, 95, 184),
                border: rgb(204, 204, 204),
                card: rgb(249, 249, 249),
                input: rgb(255, 255, 255),
                selected: Color::BLACK.alpha(0.08),
                divider: rgb(200, 200, 200),
            }
        }
    }
}
