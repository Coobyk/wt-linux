use rune::Color;

use crate::model::VtColor;

/// The colours a terminal draws with, as sRGB bytes (Windows Terminal's "Campbell" by default).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub cursor: [u8; 3],
    pub selection: [u8; 3],
    pub ansi: [[u8; 3]; 16],
}

impl Default for Palette {
    fn default() -> Palette {
        Palette {
            fg: [0xcc, 0xcc, 0xcc],
            bg: [0x0c, 0x0c, 0x0c],
            cursor: [0xff, 0xff, 0xff],
            selection: [0x3a, 0x5f, 0x9e],
            ansi: [
                [0x0c, 0x0c, 0x0c],
                [0xc5, 0x0f, 0x1f],
                [0x13, 0xa1, 0x0e],
                [0xc1, 0x9c, 0x00],
                [0x00, 0x37, 0xda],
                [0x88, 0x17, 0x98],
                [0x3a, 0x96, 0xdd],
                [0xcc, 0xcc, 0xcc],
                [0x76, 0x76, 0x76],
                [0xe7, 0x48, 0x56],
                [0x16, 0xc6, 0x0c],
                [0xf9, 0xf1, 0xa5],
                [0x3b, 0x78, 0xff],
                [0xb4, 0x00, 0x9e],
                [0x61, 0xd6, 0xd6],
                [0xf2, 0xf2, 0xf2],
            ],
        }
    }
}

impl Palette {
    /// The sRGB bytes of xterm colour `i`: 16 palette entries, a 6×6×6 cube, then 24 greys.
    pub fn indexed(&self, i: u8) -> [u8; 3] {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let n = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                [level(n / 36), level((n / 6) % 6), level(n % 6)]
            }
            _ => {
                let v = 8 + (i - 232) * 10;
                [v, v, v]
            }
        }
    }

    pub(crate) fn resolve(&self, c: VtColor, default: [u8; 3]) -> [u8; 3] {
        match c {
            VtColor::Default => default,
            VtColor::Indexed(i) => self.indexed(i),
            VtColor::Rgb(r, g, b) => [r, g, b],
        }
    }
}

/// Rune colours are linear light; terminal colours are sRGB bytes.
pub fn color(rgb: [u8; 3]) -> Color {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    Color {
        r: lin(rgb[0]),
        g: lin(rgb[1]),
        b: lin(rgb[2]),
        a: 1.0,
    }
}
