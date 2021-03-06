use euclid::Rect;

use super::styles::{Style, Styles};
use crate::screen::{Coordinate, Screen};

#[derive(Debug, Default)]
pub struct CommandLine {
    buf: String,
}

impl CommandLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, c: char) {
        self.buf.push(c);
    }

    pub fn delete(&mut self) {
        self.buf.pop();
    }

    pub fn command(&self) -> &str {
        &self.buf
    }

    pub fn render(&self, styles: &Styles, bounds: Rect<usize>, screen: &mut Screen) {
        let mut line = String::from(":");
        line.push_str(&self.buf);
        screen.erase_line(bounds.origin.y);
        screen.write_str(bounds.origin, &line);
        screen.draw_cursor(Coordinate {
            x: bounds.origin.x + line.chars().count(),
            ..bounds.origin
        });
        screen.apply_style(
            bounds.origin,
            line.chars().count(),
            &Style {
                fg: styles.fg,
                bg: styles.bg,
                ..Default::default()
            },
        );
        screen.refresh().unwrap();
    }
}
