use std::fmt::{self, Display};

use termion::color::Rgb;

use crate::protocol;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn from_argb(argb: u32) -> Self {
        Self {
            r: ((argb & 0x00ff_0000) >> 16) as u8,
            g: ((argb & 0x0000_ff00) >> 8) as u8,
            b: (argb & 0x0000_00ff) as u8,
        }
    }

    pub fn as_escapes(&self) -> Rgb {
        Rgb(self.r, self.g, self.b)
    }
}

impl Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl From<protocol::Color> for Color {
    fn from(color: protocol::Color) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn argb_conversion() {
        assert_eq!(
            Color::from_argb(4292032130),
            Color {
                r: 211,
                g: 54,
                b: 130,
            }
        );
    }

    #[test]
    fn display() {
        assert_eq!(Color::from_argb(4292032130).to_string(), "#d33682");
    }
}
