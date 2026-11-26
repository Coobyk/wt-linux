//! Windows-Terminal-style colour schemes: 16 ANSI colours plus foreground, background,
//! cursor and selection, as `#rrggbb` strings.
use rune::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub selection_background: Option<String>,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub purple: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_purple: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

impl ColorScheme {
    /// ANSI colour `i` (0..16) as hex.
    pub fn ansi_hex(&self, i: u8) -> &str {
        [
            &self.black,
            &self.red,
            &self.green,
            &self.yellow,
            &self.blue,
            &self.purple,
            &self.cyan,
            &self.white,
            &self.bright_black,
            &self.bright_red,
            &self.bright_green,
            &self.bright_yellow,
            &self.bright_blue,
            &self.bright_purple,
            &self.bright_cyan,
            &self.bright_white,
        ][(i & 15) as usize]
    }

    pub fn fg(&self) -> Color {
        hex_color(&self.foreground)
    }

    pub fn bg(&self) -> Color {
        hex_color(&self.background)
    }

    pub fn cursor_color(&self) -> Color {
        self.cursor
            .as_deref()
            .map(hex_color)
            .unwrap_or_else(|| self.fg())
    }

    /// xterm 256-colour palette: 16 scheme colours, a 6x6x6 cube, then 24 greys.
    pub fn indexed(&self, i: u8) -> Color {
        match i {
            0..16 => hex_color(self.ansi_hex(i)),
            16..232 => {
                let n = i - 16;
                let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                rgb(step(n / 36), step((n / 6) % 6), step(n % 6))
            }
            _ => {
                let v = 8 + (i - 232) * 10;
                rgb(v, v, v)
            }
        }
    }
}

/// An sRGB byte triple as Rune's linear `Color`.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Color {
        r: lin(r),
        g: lin(g),
        b: lin(b),
        a: 1.0,
    }
}

/// `#rrggbb` (or `rrggbb`); malformed strings are magenta so they're easy to spot.
pub fn hex_color(s: &str) -> Color {
    parse_hex(s)
        .map(|(r, g, b)| rgb(r, g, b))
        .unwrap_or_else(|| rgb(255, 0, 255))
}

pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

fn scheme(name: &str, fg: &str, bg: &str, cursor: &str, c: [&str; 16]) -> ColorScheme {
    let s = |x: &str| x.to_string();
    ColorScheme {
        name: s(name),
        foreground: s(fg),
        background: s(bg),
        cursor: Some(s(cursor)),
        selection_background: Some(s("#ffffff")),
        black: s(c[0]),
        red: s(c[1]),
        green: s(c[2]),
        yellow: s(c[3]),
        blue: s(c[4]),
        purple: s(c[5]),
        cyan: s(c[6]),
        white: s(c[7]),
        bright_black: s(c[8]),
        bright_red: s(c[9]),
        bright_green: s(c[10]),
        bright_yellow: s(c[11]),
        bright_blue: s(c[12]),
        bright_purple: s(c[13]),
        bright_cyan: s(c[14]),
        bright_white: s(c[15]),
    }
}

/// Windows Terminal's stock schemes. The first is the fallback.
pub fn builtin_schemes() -> Vec<ColorScheme> {
    vec![
        scheme(
            "Campbell",
            "#cccccc",
            "#0c0c0c",
            "#ffffff",
            [
                "#0c0c0c", "#c50f1f", "#13a10e", "#c19c00", "#0037da", "#881798", "#3a96dd",
                "#cccccc", "#767676", "#e74856", "#16c60c", "#f9f1a5", "#3b78ff", "#b4009e",
                "#61d6d6", "#f2f2f2",
            ],
        ),
        scheme(
            "One Half Dark",
            "#dcdfe4",
            "#282c34",
            "#ffffff",
            [
                "#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2",
                "#dcdfe4", "#5a6374", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd",
                "#56b6c2", "#dcdfe4",
            ],
        ),
        scheme(
            "One Half Light",
            "#383a42",
            "#fafafa",
            "#4f525d",
            [
                "#383a42", "#e45649", "#50a14f", "#c18301", "#0184bc", "#a626a4", "#0997b3",
                "#fafafa", "#4f525d", "#df6c75", "#98c379", "#e4c07a", "#61afef", "#c577dd",
                "#56b5c1", "#ffffff",
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#0c0c0c"), Some((12, 12, 12)));
        assert_eq!(parse_hex("ffFF00"), Some((255, 255, 0)));
        assert_eq!(parse_hex("#fff"), None);
        assert!((hex_color("#ffffff").r - 1.0).abs() < 1e-6);
        for s in builtin_schemes() {
            for i in 0..16 {
                assert!(parse_hex(s.ansi_hex(i)).is_some(), "{} {i}", s.name);
            }
        }
    }
}
