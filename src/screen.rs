use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

use log::*;
use ndarray::prelude::*;
use termion::{
    self, clear,
    color::{Bg, Fg, Rgb},
    cursor,
    raw::IntoRawMode,
    screen::AlternateScreen,
    style,
};

use protocol::Color;

type Buffer = Array2<Cell>;

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
            text_color: (None, None),
        })
    }

    pub fn write_str(&mut self, Coordinate { y, x }: Coordinate, s: &str) {
        let mut row = self.buf.row_mut(usize::from(y));

        for (i, c) in s.chars().enumerate() {
            row[usize::from(x) + i] = Cell {
                c,
                is_reverse: false,
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
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            is_reverse: false,
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
