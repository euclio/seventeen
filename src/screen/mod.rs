use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

use log::*;
use ndarray::prelude::*;
use termion::{
    self, clear,
    color::{Bg, Fg, Reset, Rgb},
    cursor,
    raw::IntoRawMode,
    screen::AlternateScreen,
    style,
};

mod color;

pub use self::color::Color;

/// The number of style IDs that are reserved by the backend.
const RESERVED_STYLES: usize = 2;

type Buffer = Array2<Cell>;

#[derive(Debug, Default, Clone)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub underline: bool,
    pub italic: bool,
}

impl Style {
    /// Returns a string containing the appropriate escape sequences to enable the style.
    fn enable_sequences(&self) -> String {
        let mut sequences = String::new();

        if let Some(fg) = &self.fg {
            write!(sequences, "{}", Fg(Rgb(fg.r, fg.g, fg.b))).unwrap();
        }

        sequences
    }
}

/// A position in the terminal, zero-indexed.
#[derive(Debug, Clone, Copy)]
pub struct Coordinate {
    pub x: u16,
    pub y: u16,
}

impl From<(u16, u16)> for Coordinate {
    fn from((y, x): (u16, u16)) -> Self {
        Coordinate { y, x }
    }
}

/// An ncurses-like abstraction over the terminal screen.
///
/// This struct allows writing characters and attributes to an intermediate buffer, and then
/// writing the appropriate escape sequences by calling `refresh()`.
///
/// Note that unlike raw escape sequences, all indices expected by this struct are 0-based.
pub struct Screen {
    /// A buffer containing what should be displayed on the screen at the next refresh.
    buf: Buffer,

    /// Styles defined by the backend.
    styles: Vec<Style>,

    /// Foreground and background color for text.
    text_color: (Option<Color>, Option<Color>),

    out: Box<Write>,
}

impl Screen {
    pub fn new() -> io::Result<Self> {
        let (width, height) = termion::terminal_size()?;

        let mut screen = AlternateScreen::from(io::stdout().into_raw_mode()?);
        write!(screen, "{}{}", cursor::Hide, clear::All)?;
        screen.flush()?;

        Self::new_from_write(usize::from(height), usize::from(width), screen)
    }

    fn new_from_write<W>(height: usize, width: usize, write: W) -> io::Result<Self>
    where
        W: Write + 'static,
    {
        let out = Box::new(write);
        let buf = Buffer::from_elem((height, width), Cell::default());
        Ok(Self {
            buf,
            out,
            styles: vec![Style::default(); RESERVED_STYLES],
            text_color: (None, None),
        })
    }

    pub fn define_style(&mut self, id: u64, style: Style) {
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

        if id == self.styles.len() as u64 {
            self.styles.push(style);
        } else {
            self.styles[id as usize] = style;
        }
    }

    pub fn apply_style(&mut self, id: u64, Coordinate { y, x }: Coordinate, n: usize) {
        let mut row = self.buf.row_mut(usize::from(y));
        row[usize::from(x)].span_start = Some(id);
        row[(u64::from(x) + n as u64) as usize].span_end = Some(id);
    }

    pub fn write_str(&mut self, Coordinate { y, x }: Coordinate, s: &str) {
        let mut row = self.buf.row_mut(usize::from(y));

        for (i, c) in s.chars().enumerate() {
            row[usize::from(x) + i] = Cell {
                c,
                ..Default::default()
            };
        }
    }

    pub fn set_text_color(&mut self, fg: Option<Color>, bg: Option<Color>) {
        self.text_color = (fg, bg);
    }

    pub fn draw_cursor(&mut self, Coordinate { y, x }: Coordinate) {
        self.buf.row_mut(usize::from(y))[usize::from(x)].is_reverse = true;
    }

    /// Erase all characters from the screen.
    pub fn erase(&mut self) {
        self.buf.fill(Cell::default());
    }

    /// Push the contents of the internal buffer to the screen.
    pub fn refresh(&mut self) -> io::Result<()> {
        debug!("refreshing screen contents");

        // FIXME: Right now this is a completely naive implementation. We redraw the entire screen
        // on refresh, even if very little changed.

        let mut sequences = String::new();

        if let Some(fg) = &self.text_color.0 {
            write!(sequences, "{}", Fg(Rgb(fg.r, fg.g, fg.b))).unwrap();
        }

        if let Some(bg) = &self.text_color.1 {
            write!(sequences, "{}", Bg(Rgb(bg.r, bg.g, bg.b))).unwrap();
        }

        // enumerate() doesn't seem to work here?
        let mut i = 1;
        for row in self.buf.genrows() {
            write!(sequences, "{}{}", cursor::Goto(1, i), clear::CurrentLine).unwrap();

            for cell in row {
                match (cell.span_start, cell.span_end) {
                    (Some(style_id), _) => {
                        let style = &self.styles[style_id as usize];
                        write!(sequences, "{}", style.enable_sequences()).unwrap();
                    },
                    (None, Some(_)) => {
                        write!(sequences, "{}", Fg(Reset)).unwrap();
                    }
                    _ => (),
                }

                if cell.is_reverse {
                    write!(sequences, "{}{}{}", style::Invert, cell.c, style::NoInvert).unwrap();
                } else {
                    write!(sequences, "{}", cell.c).unwrap();
                }
            }

            i += 1;
        }

        self.out.write_all(sequences.as_bytes())?;
        self.out.flush()?;

        Ok(())
    }
}

/// A single position in the terminal display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    /// The character that should be displayed by this cell.
    c: char,

    /// Whether the cell should be reversed.
    is_reverse: bool,

    span_start: Option<u64>,
    span_end: Option<u64>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            is_reverse: false,
            span_start: None,
            span_end: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::Screen;

    #[test]
    fn write_str() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(5, 15, buf).unwrap();

        screen.write_str((0, 0).into(), "Hello, world!");

        println!("{:?}", screen.buf);

        assert_eq!(
            screen
                .buf
                .row(0)
                .into_slice()
                .unwrap()
                .iter()
                .map(|cell| cell.c)
                .collect::<String>(),
            "Hello, world!  "
        );
    }
}
