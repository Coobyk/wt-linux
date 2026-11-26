//! Cell colors and the 256-entry palette they resolve against.

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `0xRRGGBB` -> [`Rgb`].
    pub const fn hex(v: u32) -> Self {
        Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

/// The color of a cell's foreground, background or underline.
///
/// `Default` means "the terminal's default foreground" when used as `fg` (and for
/// underline color: "same as fg"), and "the default background" when used as `bg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    #[default]
    Default,
    /// Palette index: 0-7 ANSI, 8-15 bright ANSI, 16-231 6x6x6 cube, 232-255 grayscale.
    Indexed(u8),
    /// Truecolor (`SGR 38;2;r;g;b`).
    Rgb(Rgb),
}

/// Which default a [`Color::Default`] resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    Foreground,
    Background,
}

/// 256 indexed colors plus default fg/bg/cursor.
///
/// Defaults to Windows Terminal's "Campbell" scheme for 0-15 and the standard xterm
/// cube/grayscale ramp for 16-255.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub colors: [Rgb; 256],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self::campbell()
    }
}

/// Windows Terminal "Campbell" ANSI colors, in index order 0-15.
pub const CAMPBELL_ANSI: [u32; 16] = [
    0x0C0C0C, 0xC50F1F, 0x13A10E, 0xC19C00, 0x0037DA, 0x881798, 0x3A96DD, 0xCCCCCC, 0x767676,
    0xE74856, 0x16C60C, 0xF9F1A5, 0x3B78FF, 0xB4009E, 0x61D6D6, 0xF2F2F2,
];

impl Palette {
    pub fn campbell() -> Self {
        Self::from_ansi(
            CAMPBELL_ANSI.map(Rgb::hex),
            Rgb::hex(0xCCCCCC),
            Rgb::hex(0x0C0C0C),
            Rgb::hex(0xFFFFFF),
        )
    }

    /// Build a palette from 16 ANSI colors; 16-255 follow the xterm formula.
    pub fn from_ansi(ansi: [Rgb; 16], foreground: Rgb, background: Rgb, cursor: Rgb) -> Self {
        let mut colors = [Rgb::default(); 256];
        colors[..16].copy_from_slice(&ansi);
        for (i, c) in colors.iter_mut().enumerate().skip(16) {
            *c = xterm_color(i as u8);
        }
        Self {
            colors,
            foreground,
            background,
            cursor,
        }
    }

    /// Resolve a cell color to RGB. Does not apply bold-as-bright, dim or inverse; that
    /// is presentation policy and belongs to the renderer.
    pub fn resolve(&self, color: Color, role: ColorRole) -> Rgb {
        match color {
            Color::Rgb(rgb) => rgb,
            Color::Indexed(i) => self.colors[i as usize],
            Color::Default => match role {
                ColorRole::Foreground => self.foreground,
                ColorRole::Background => self.background,
            },
        }
    }
}

/// Standard xterm value of palette index `i` (for 16-255; 0-15 use xterm's VGA-ish set).
pub fn xterm_color(i: u8) -> Rgb {
    const ANSI: [u32; 16] = [
        0x000000, 0xCD0000, 0x00CD00, 0xCDCD00, 0x0000EE, 0xCD00CD, 0x00CDCD, 0xE5E5E5, 0x7F7F7F,
        0xFF0000, 0x00FF00, 0xFFFF00, 0x5C5CFF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
    ];
    match i {
        0..=15 => Rgb::hex(ANSI[i as usize]),
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            Rgb::new(v, v, v)
        }
    }
}
