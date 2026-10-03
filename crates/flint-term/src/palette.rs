//! The terminal's colours: flint's theme plus ANSI colours tuned to match,
//! and resolution of a cell's colour (named, 256-indexed or truecolor, with
//! any OSC overrides the program set) to RGB.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::Color;
use alacritty_terminal::vte::ansi::NamedColor;
use alacritty_terminal::vte::ansi::Rgb;

/// An RGB colour, independent of any UI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn hex(value: u32) -> Self {
        Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
        }
    }

    /// Mixes toward `other` by `t` in `[0, 1]`.
    pub fn mix(self, other: Rgb8, t: f32) -> Rgb8 {
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb8 {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
        }
    }
}

impl From<Rgb> for Rgb8 {
    fn from(rgb: Rgb) -> Self {
        Self {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }
    }
}

/// flint's terminal palette.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub background: Rgb8,
    pub foreground: Rgb8,
    pub cursor: Rgb8,
    pub selection: Rgb8,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then the
    /// eight bright variants.
    pub ansi: [Rgb8; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Rgb8::hex(0x0e0e10),
            foreground: Rgb8::hex(0xe7e7ea),
            cursor: Rgb8::hex(0xff8a3d),
            selection: Rgb8::hex(0x4a3226),
            ansi: [
                Rgb8::hex(0x1c1c20),
                Rgb8::hex(0xf0645a),
                Rgb8::hex(0x7fd88f),
                Rgb8::hex(0xf5c46b),
                Rgb8::hex(0x6fa8ff),
                Rgb8::hex(0xc792ea),
                Rgb8::hex(0x5fd7d7),
                Rgb8::hex(0xc8c8cc),
                Rgb8::hex(0x55555c),
                Rgb8::hex(0xff8577),
                Rgb8::hex(0xa3e9ae),
                Rgb8::hex(0xffd58f),
                Rgb8::hex(0x94bfff),
                Rgb8::hex(0xdcb2f5),
                Rgb8::hex(0x8eeaea),
                Rgb8::hex(0xf4f4f6),
            ],
        }
    }
}

impl Palette {
    /// The 256-colour table: 16 ANSI, the 6×6×6 cube, then 24 greys.
    pub fn indexed(&self, index: u8) -> Rgb8 {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                let i = index - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Rgb8 {
                    r: level(i / 36),
                    g: level((i / 6) % 6),
                    b: level(i % 6),
                }
            }
            232..=255 => {
                let v = 8 + (index - 232) * 10;
                Rgb8 { r: v, g: v, b: v }
            }
        }
    }

    fn named(&self, name: NamedColor) -> Rgb8 {
        match name {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::DimForeground => self.foreground.mix(self.background, 0.35),
            NamedColor::DimBlack
            | NamedColor::DimRed
            | NamedColor::DimGreen
            | NamedColor::DimYellow
            | NamedColor::DimBlue
            | NamedColor::DimMagenta
            | NamedColor::DimCyan
            | NamedColor::DimWhite => {
                let base = name as usize - NamedColor::DimBlack as usize;
                self.ansi[base].mix(self.background, 0.35)
            }
            other => {
                let index = other as usize;
                if index < 16 {
                    self.ansi[index]
                } else {
                    self.foreground
                }
            }
        }
    }

    /// RGB for a cell colour, honouring OSC overrides in `colors`.
    pub fn resolve(&self, color: Color, colors: &Colors) -> Rgb8 {
        match color {
            Color::Spec(rgb) => rgb.into(),
            Color::Indexed(index) => {
                colors[index as usize].map_or_else(|| self.indexed(index), Into::into)
            }
            Color::Named(name) => colors[name].map_or_else(|| self.named(name), Into::into),
        }
    }
}
