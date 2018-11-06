use std::io::{self, Stdout, Write};

use bitflags::bitflags;
use euclid::{Point2D, Size2D};
use log::*;
use ndarray::prelude::*;
use termion::{
    clear,
    color::{Bg, Fg},
    cursor,
    raw::{IntoRawMode, RawTerminal},
    screen::AlternateScreen,
    style,
};

use crate::editor::styles::Style;

mod color;

pub use self::color::Color;

type Buffer = Array2<Cell>;

/// A coordinate on the screen.
pub type Coordinate = Point2D<usize>;

bitflags! {
    /// Terminal video attributes.
    #[derive(Default)]
    pub struct Attr: u8 {
        const BOLD = 1;
        const ITALIC = 1 << 1;
        const UNDERLINE = 1 << 2;
        const REVERSE = 1 << 3;
    }
}

/// A single position in the terminal display.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Cell {
    /// The character that should be displayed by this cell.
    c: Option<char>,
    fg: Option<Color>,
    bg: Option<Color>,
    attr: Attr,
}

/// An ncurses-like abstraction over the terminal screen.
///
/// This struct allows writing characters and attributes to an intermediate buffer, and then
/// writing the appropriate escape sequences to the screen by calling `refresh()`.
///
/// Note that unlike raw escape sequences, all indices expected by this struct are 0-based.
pub struct Screen<W = AlternateScreen<RawTerminal<Stdout>>>
where
    W: Write,
{
    /// A buffer containing what should be displayed on the screen at the next refresh.
    buf: Buffer,

    /// A buffer containing what is currently displayed on the screen.
    cur_buf: Buffer,

    out: W,
}

impl Screen {
    pub fn new(size: Size2D<usize>) -> io::Result<Self> {
        let mut screen = AlternateScreen::from(io::stdout().into_raw_mode()?);
        write!(screen, "{}{}", cursor::Hide, clear::All)?;
        screen.flush()?;

        Self::new_from_write(size, screen)
    }

    pub fn new_from_write<W>(size: Size2D<usize>, write: W) -> io::Result<Screen<W>>
    where
        W: Write,
    {
        let buf = Buffer::from_elem((size.height, size.width), Default::default());
        Ok(Screen {
            cur_buf: buf.clone(),
            buf,
            out: write,
        })
    }
}

impl<W: Write> Screen<W> {
    pub fn apply_style(&mut self, Coordinate { y, x, .. }: Coordinate, n: usize, style: &Style) {
        let mut row = self.buf.row_mut(y);

        for i in x..(x + n) {
            row[i].fg = style.fg;
            row[i].bg = style.bg;

            if style.bold {
                row[i].attr |= Attr::BOLD;
            }

            if style.italic {
                row[i].attr |= Attr::ITALIC;
            }

            if style.underline {
                row[i].attr |= Attr::UNDERLINE;
            }
        }
    }

    pub fn write_str(&mut self, Coordinate { x, y, .. }: Coordinate, s: &str) {
        let mut row = self.buf.row_mut(y);

        for (i, c) in s.chars().enumerate() {
            row[x + i].c = Some(c);
        }
    }

    pub fn draw_cursor(&mut self, Coordinate { x, y, .. }: Coordinate) {
        self.buf.row_mut(y)[x].attr |= Attr::REVERSE;
    }

    /// Erase all characters from the screen.
    pub fn erase(&mut self) {
        self.buf.fill(Cell::default());
    }

    pub fn erase_line(&mut self, line: usize) {
        self.buf.row_mut(line).fill(Cell::default());
    }

    /// Push the contents of the internal buffer to the screen.
    pub fn refresh(&mut self) -> io::Result<()> {
        debug!("refreshing screen contents");

        let mut fg = None;
        let mut bg = None;
        let mut is_bold = false;
        let mut is_italic = false;
        let mut is_underline = false;
        let mut is_reverse = false;
        let mut needs_goto = false;

        for (row_idx, row) in self.buf.genrows().into_iter().enumerate() {
            if row == self.cur_buf.subview(Axis(0), row_idx) {
                continue;
            }

            write!(
                self.out,
                "{}{}",
                cursor::Goto(1, row_idx as u16 + 1),
                clear::CurrentLine
            )?;

            for (cell_idx, cell) in row.into_iter().enumerate() {
                if cell.fg != fg {
                    fg = cell.fg;
                    if let Some(fg) = cell.fg {
                        write!(self.out, "{}", Fg(fg.as_escapes()))?;
                    }
                }

                if cell.bg != bg {
                    bg = cell.bg;
                    if let Some(bg) = cell.bg {
                        write!(self.out, "{}", Bg(bg.as_escapes()))?;
                    }
                }

                if cell.attr.contains(Attr::BOLD) && !is_bold {
                    write!(self.out, "{}", style::Bold)?
                } else if is_bold && !cell.attr.contains(Attr::BOLD) {
                    // While disabling bold is technically the right escape sequence to use here
                    // (SGR 21), it's not supported by iTerm2 or xterm. So, we emit SGR 22, which
                    // disables bold and faint. We don't use faint anywhere else, so it's OK.
                    //
                    // See https://gitlab.com/gnachman/iterm2/issues/3208
                    write!(self.out, "{}", style::NoFaint)?;
                }

                if cell.attr.contains(Attr::ITALIC) && !is_italic {
                    write!(self.out, "{}", style::Italic)?;
                } else if is_italic && !cell.attr.contains(Attr::ITALIC) {
                    write!(self.out, "{}", style::NoItalic)?;
                }

                if cell.attr.contains(Attr::UNDERLINE) && !is_underline {
                    write!(self.out, "{}", style::Underline)?;
                } else if is_underline && !cell.attr.contains(Attr::UNDERLINE) {
                    write!(self.out, "{}", style::NoUnderline)?;
                }

                if cell.attr.contains(Attr::REVERSE) && !is_reverse {
                    write!(self.out, "{}", style::Invert)?;
                } else if !cell.attr.contains(Attr::REVERSE) && is_reverse {
                    write!(self.out, "{}", style::NoInvert)?;
                }

                let c = match cell.c {
                    Some(c) => Some(c),
                    // Special case: reverse video needs a non-empty cell.
                    None if cell.attr.contains(Attr::REVERSE) => Some(' '),
                    None => None,
                };

                if let Some(c) = c {
                    if needs_goto {
                        write!(
                            self.out,
                            "{}",
                            cursor::Goto(cell_idx as u16 + 1, row_idx as u16 + 1)
                        )?;
                        needs_goto = false;
                    }
                    write!(self.out, "{}", c)?;
                } else {
                    needs_goto = true;
                }

                is_bold = cell.attr.contains(Attr::BOLD);
                is_italic = cell.attr.contains(Attr::ITALIC);
                is_underline = cell.attr.contains(Attr::UNDERLINE);
                is_reverse = cell.attr.contains(Attr::REVERSE);
            }
        }

        self.out.flush()?;
        self.cur_buf = self.buf.clone();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use euclid::Size2D;
    use termion::{
        clear,
        color::{Fg, Rgb},
        cursor::Goto,
        style::{Bold, Invert, Italic, NoFaint, NoInvert, NoItalic, NoUnderline, Underline},
    };

    use super::{Color, Coordinate, Screen};
    use crate::editor::styles::Style;

    #[test]
    fn write_str() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 5), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "Hello, world!");

        assert_eq!(
            screen
                .buf
                .row(0)
                .into_slice()
                .unwrap()
                .iter()
                .flat_map(|cell| cell.c)
                .collect::<String>(),
            "Hello, world!"
        );
    }

    #[test]
    fn write_with_gap() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(20, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "hello");
        screen.write_str(Coordinate::new(10, 0), "goodbye");
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}hello{}goodbye",
                Goto(1, 1),
                clear::CurrentLine,
                Goto(11, 1),
            )
        );
    }

    #[test]
    fn cursor_on_empty_cell() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(20, 1), buf).unwrap();

        screen.draw_cursor(Coordinate::new(10, 0));
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}{} {}",
                Goto(1, 1),
                clear::CurrentLine,
                Invert,
                Goto(11, 1),
                NoInvert
            )
        )
    }

    #[test]
    fn simple_span() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "Hello, world!");
        screen.apply_style(
            Coordinate::new(0, 0),
            5,
            &Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}Hello{}, world!",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                NoFaint,
            )
        );
    }

    #[test]
    fn end_to_end_spans() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "bolditalic");
        screen.apply_style(
            Coordinate::new(0, 0),
            4,
            &Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.apply_style(
            Coordinate::new(4, 0),
            6,
            &Style {
                italic: true,
                ..Default::default()
            },
        );
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}bold{}{}italic{}",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                NoFaint,
                Italic,
                NoItalic,
            )
        );
    }

    #[test]
    fn disjoint_spans() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        let bold = Style {
            bold: true,
            ..Default::default()
        };

        screen.write_str(Coordinate::new(0, 0), "int main() {}");
        screen.apply_style(Coordinate::new(0, 0), 3, &bold);
        screen.apply_style(Coordinate::new(3, 0), 1, &Style::default());
        screen.apply_style(Coordinate::new(4, 0), 4, &bold);
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}int{} {}main{}() {{}}",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                NoFaint,
                Bold,
                NoFaint,
            ),
        );
    }

    #[test]
    #[should_panic]
    fn style_length_too_long() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(10, 3), buf).unwrap();
        screen.write_str(Coordinate::new(0, 0), "foobarbaz");
        screen.apply_style(
            Coordinate::new(0, 0),
            20,
            &Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.refresh().unwrap();
    }

    #[test]
    #[should_panic]
    fn style_coordinate_out_of_bounds() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(10, 3), buf).unwrap();
        screen.write_str(Coordinate::new(0, 0), "foobarbaz");
        screen.apply_style(
            Coordinate::new(20, 2),
            5,
            &Style {
                underline: true,
                ..Default::default()
            },
        );
        screen.refresh().unwrap();
    }

    #[test]
    fn color_change() {
        let red = Style {
            fg: Some(Color { r: 255, g: 0, b: 0 }),
            ..Default::default()
        };
        let green = Style {
            fg: Some(Color { r: 0, g: 255, b: 0 }),
            ..Default::default()
        };
        let blue = Style {
            fg: Some(Color { r: 0, g: 0, b: 255 }),
            ..Default::default()
        };

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "redgreenblue");
        screen.apply_style(Coordinate::new(0, 0), 3, &red);
        screen.apply_style(Coordinate::new(3, 0), 5, &green);
        screen.apply_style(Coordinate::new(8, 0), 4, &blue);
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}red{}green{}blue",
                Goto(1, 1),
                clear::CurrentLine,
                Fg(Rgb(255, 0, 0)),
                Fg(Rgb(0, 255, 0)),
                Fg(Rgb(0, 0, 255)),
            ),
        );
    }

    #[test]
    fn styles() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(4, 1), buf).unwrap();

        let style = Style {
            bold: true,
            italic: true,
            underline: true,
            ..Default::default()
        };

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.apply_style(Coordinate::new(0, 0), 3, &style);
        screen.draw_cursor(Coordinate::new(1, 0));
        screen.refresh().unwrap();

        println!("{:#?}", screen.buf);

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}{}{}f{}o{}o{}{}{}",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                Italic,
                Underline,
                Invert,
                NoInvert,
                NoFaint,
                NoItalic,
                NoUnderline,
            )
        );
    }

    #[test]
    fn refresh() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(4, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh().unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!("{}{}foo", Goto(1, 1), clear::CurrentLine),
        );
    }
}
