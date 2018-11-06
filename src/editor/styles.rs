use std::ops::Index;

use log::*;

use crate::screen::Color;

#[derive(Debug, Default, Clone)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub underline: bool,
    pub italic: bool,
}

#[derive(Debug, Default)]
pub struct Styles {
    /// Default foreground color for text.
    pub fg: Option<Color>,

    /// Default background color for text.
    pub bg: Option<Color>,

    styles: Vec<Style>,
}

impl Styles {
    pub fn new() -> Self {
        Styles::default()
    }

    pub fn define(&mut self, id: u64, style: Style) {
        info!(
            "defined style {}: fg={} bg={} bold={} underline={} italic={}",
            id,
            style
                .fg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| String::from("none")),
            style
                .bg
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| String::from("none")),
            style.bold,
            style.underline,
            style.italic,
        );

        if id as usize >= self.styles.len() {
            self.styles.resize(id as usize + 1, Style::default());
        }

        self.styles[id as usize] = style;
    }
}

impl Index<u64> for Styles {
    type Output = Style;

    fn index(&self, idx: u64) -> &Style {
        &self.styles[idx as usize]
    }
}
