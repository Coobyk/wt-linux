//! Window chrome colours (tab bar, palette, settings), Windows-Terminal-like.
use rune::Color;

use crate::scheme::rgb;

#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    pub window: Color,
    pub tab_bar: Color,
    pub tab_active: Color,
    pub tab_hover: Color,
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
    pub fn of(dark: bool) -> Chrome {
        if dark {
            Chrome {
                window: rgb(12, 12, 12),
                tab_bar: rgb(32, 32, 32),
                tab_active: rgb(12, 12, 12),
                tab_hover: rgb(48, 48, 48),
                text: rgb(255, 255, 255),
                text_dim: rgb(160, 160, 160),
                accent: rgb(59, 120, 255),
                border: rgb(64, 64, 64),
                card: rgb(40, 40, 40),
                input: rgb(28, 28, 28),
                selected: rgb(58, 58, 64),
                divider: rgb(56, 56, 56),
            }
        } else {
            Chrome {
                window: rgb(250, 250, 250),
                tab_bar: rgb(232, 232, 232),
                tab_active: rgb(250, 250, 250),
                tab_hover: rgb(218, 218, 218),
                text: rgb(20, 20, 20),
                text_dim: rgb(96, 96, 96),
                accent: rgb(0, 95, 184),
                border: rgb(204, 204, 204),
                card: rgb(255, 255, 255),
                input: rgb(243, 243, 243),
                selected: rgb(222, 230, 245),
                divider: rgb(200, 200, 200),
            }
        }
    }
}
