use std::io::{self, Stdout, Write};

use euclid::{Point2D, Size2D};
use log::*;
use ndarray::prelude::*;
use termion::{
    clear,
    color::{Bg, Fg, Reset, Rgb},
    cursor,
    raw::{IntoRawMode, RawTerminal},
    screen::AlternateScreen,
    style,
};

use crate::editor::styles::{Style, Styles};

mod color;

pub use self::color::Color;

type Buffer = Array2<Cell>;

/// A coordinate on the screen.
pub type Coordinate = Point2D<usize>;

/// An ncurses-like abstraction over the terminal screen.
///
/// This struct allows writing characters and attributes to an intermediate buffer, and then
/// writing the appropriate escape sequences by calling `refresh()`.
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
        let buf = Buffer::from_elem((size.height, size.width), Cell::default());
        Ok(Screen {
            cur_buf: buf.clone(),
            buf,
            out: write,
        })
    }
}

impl<W: Write> Screen<W> {
    pub fn apply_style(&mut self, Coordinate { y, x, .. }: Coordinate, id: u64, n: usize) {
        let mut row = self.buf.row_mut(y);

        if let Some(cell) = &mut row.get_mut(x) {
            cell.span_start = Some(id);
        }
        if let Some(cell) = &mut row.get_mut(x + n) {
            cell.span_end = Some(id);
        }
    }

    pub fn write_str(&mut self, Coordinate { x, y, .. }: Coordinate, s: &str) {
        let mut row = self.buf.row_mut(y);

        for (i, c) in s.chars().enumerate() {
            row[x + i] = Cell {
                c,
                ..Default::default()
            };
        }
    }

    pub fn draw_cursor(&mut self, Coordinate { x, y, .. }: Coordinate) {
        self.buf.row_mut(usize::from(y))[usize::from(x)].is_reverse = true;
    }

    /// Erase all characters from the screen.
    pub fn erase(&mut self) {
        self.buf.fill(Cell::default());
    }

    pub fn erase_line(&mut self, line: usize) {
        self.buf.row_mut(line).fill(Cell::default());
    }

    /// Push the contents of the internal buffer to the screen.
    pub fn refresh(&mut self, styles: &Styles) -> io::Result<()> {
        debug!("refreshing screen contents");

        if let Some(fg) = &styles.fg {
            write!(self.out, "{}", Fg(Rgb(fg.r, fg.g, fg.b)))?;
        }

        if let Some(bg) = &styles.bg {
            write!(self.out, "{}", Bg(Rgb(bg.r, bg.g, bg.b)))?;
        }

        for (i, row) in self.buf.genrows().into_iter().enumerate() {
            if row == self.cur_buf.subview(Axis(0), i) {
                continue;
            }

            write!(
                self.out,
                "{}{}",
                cursor::Goto(1, i as u16 + 1),
                clear::CurrentLine
            )?;

            let mut bold_spans = 0u32;
            let mut italic_spans = 0u32;
            let mut underline_spans = 0u32;

            for cell in row {
                let starting_style: Option<&Style> = cell.span_start.map(|id| &styles[id]);
                let ending_style: Option<&Style> = cell.span_end.map(|id| &styles[id]);

                if let Some(style) = &starting_style {
                    if let Some(fg) = &style.fg {
                        write!(self.out, "{}", Fg(fg.as_escapes()))?;
                    }

                    if style.bold {
                        if bold_spans == 0 {
                            write!(self.out, "{}", style::Bold)?;
                        }
                        bold_spans += 1;
                    }

                    if style.italic {
                        if italic_spans == 0 {
                            write!(self.out, "{}", style::Italic)?;
                        }
                        italic_spans += 1;
                    }

                    if style.underline {
                        if underline_spans == 0 {
                            write!(self.out, "{}", style::Underline)?;
                        }
                        underline_spans += 1;
                    }
                }

                if let Some(style) = ending_style {
                    if style.fg.is_some()
                        && !starting_style
                            .as_ref()
                            .map(|style| style.fg.is_some())
                            .unwrap_or_default()
                    {
                        write!(self.out, "{}", Fg(Reset))?;
                    }

                    if style.bg.is_some()
                        && starting_style
                            .map(|style| style.bg.is_some())
                            .unwrap_or_default()
                    {
                        write!(self.out, "{}", Bg(Reset))?;
                    }

                    if style.bold {
                        bold_spans -= 1;
                        if bold_spans == 0 {
                            // While disabling bold is technically the right escape sequence to use
                            // here (SGR 21), it's not supported by iTerm2 or xterm. So, we emit
                            // SGR 22, which disables bold and faint. We don't use faint anywhere
                            // else, so it's OK.
                            //
                            // See https://gitlab.com/gnachman/iterm2/issues/3208
                            write!(self.out, "{}", style::NoFaint)?;
                        }
                    }

                    if style.italic {
                        italic_spans -= 1;
                        if italic_spans == 0 {
                            write!(self.out, "{}", style::NoItalic)?;
                        }
                    }

                    if style.underline {
                        underline_spans -= 1;
                        if underline_spans == 0 {
                            write!(self.out, "{}", style::NoUnderline)?;
                        }
                    }
                }

                if cell.is_reverse {
                    write!(self.out, "{}{}{}", style::Invert, cell.c, style::NoInvert)?;
                } else {
                    write!(self.out, "{}", cell.c)?;
                }
            }

            // The ending spans might not have been applied if the window is too small.
            if bold_spans > 0 {
                write!(self.out, "{}", style::NoFaint)?;
            }

            if italic_spans > 0 {
                write!(self.out, "{}", style::NoItalic)?;
            }

            if underline_spans > 0 {
                write!(self.out, "{}", style::NoUnderline)?;
            }
        }

        self.out.flush()?;
        self.cur_buf = self.buf.clone();

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

    use euclid::Size2D;
    use termion::{
        clear,
        color::{Fg, Reset, Rgb},
        cursor::Goto,
        style::{Bold, Invert, Italic, NoFaint, NoInvert, NoItalic, NoUnderline, Underline},
    };

    use super::{Color, Coordinate, Screen};
    use crate::editor::styles::{Style, Styles};

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
                .map(|cell| cell.c)
                .collect::<String>(),
            "Hello, world!  "
        );
    }

    #[test]
    fn simple_span() {
        let mut styles = Styles::new();
        styles.define(
            1,
            Style {
                bold: true,
                ..Default::default()
            },
        );

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "Hello, world!");
        screen.apply_style(Coordinate::new(0, 0), 1, 5);
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}Hello{}, world!  ",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                NoFaint,
            )
        );
    }

    #[test]
    fn end_to_end_spans() {
        let mut styles = Styles::new();
        styles.define(
            1,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        styles.define(
            2,
            Style {
                italic: true,
                ..Default::default()
            },
        );

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "bolditalic");
        screen.apply_style(Coordinate::new(0, 0), 1, 4);
        screen.apply_style(Coordinate::new(4, 0), 2, 6);
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}bold{}{}italic{}     ",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                Italic,
                NoFaint,
                NoItalic,
            )
        );
    }

    #[test]
    fn disjoint_spans() {
        let mut styles = Styles::new();
        styles.define(
            2,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        styles.define(
            3,
            Style {
                bold: false,
                ..Default::default()
            },
        );
        styles.define(
            4,
            Style {
                bold: true,
                ..Default::default()
            },
        );

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "int main() {}");
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.apply_style(Coordinate::new(3, 0), 3, 1);
        screen.apply_style(Coordinate::new(4, 0), 4, 4);
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}int{} {}main{}() {{}}  ",
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
    fn span_out_of_bounds() {
        let mut styles = Styles::new();
        styles.define(
            2,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        styles.define(
            3,
            Style {
                italic: true,
                ..Default::default()
            },
        );
        styles.define(
            4,
            Style {
                underline: true,
                ..Default::default()
            },
        );

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(10, 3), buf).unwrap();
        screen.write_str(Coordinate::new(0, 0).into(), "foobarbaz");
        screen.write_str(Coordinate::new(0, 1).into(), "quux quux");
        screen.write_str(Coordinate::new(0, 2).into(), "xyzzyxyzzy");
        screen.apply_style(Coordinate::new(0, 0), 2, 20);
        screen.apply_style(Coordinate::new(5, 1), 3, 10);
        screen.apply_style(Coordinate::new(20, 2), 4, 5);
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}foobarbaz {}\
                 {}{}quux {}quux {}\
                 {}{}xyzzyxyzzy",
                Goto(1, 1),
                clear::CurrentLine,
                Bold,
                NoFaint,
                Goto(1, 2),
                clear::CurrentLine,
                Italic,
                NoItalic,
                Goto(1, 3),
                clear::CurrentLine,
            ),
        );
    }

    #[test]
    fn color_change() {
        let mut styles = Styles::new();
        styles.define(
            2,
            Style {
                fg: Some(Color { r: 255, g: 0, b: 0 }),
                ..Default::default()
            },
        );
        styles.define(
            3,
            Style {
                fg: Some(Color { r: 0, g: 255, b: 0 }),
                ..Default::default()
            },
        );
        styles.define(
            4,
            Style {
                fg: Some(Color { r: 0, g: 0, b: 255 }),
                ..Default::default()
            },
        );

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "redgreenblue");
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.apply_style(Coordinate::new(3, 0), 3, 5);
        screen.apply_style(Coordinate::new(8, 0), 4, 4);
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}red{}green{}blue{}   ",
                Goto(1, 1),
                clear::CurrentLine,
                Fg(Rgb(255, 0, 0)),
                Fg(Rgb(0, 255, 0)),
                Fg(Rgb(0, 0, 255)),
                Fg(Reset),
            ),
        );
    }

    #[test]
    fn styles() {
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(4, 1), buf).unwrap();
        let mut styles = Styles::new();

        styles.define(
            2,
            Style {
                bold: true,
                italic: true,
                underline: true,
                ..Default::default()
            },
        );

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.draw_cursor(Coordinate::new(1, 0));
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!(
                "{}{}{}{}{}f{}o{}o{}{}{} ",
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
        let styles = Styles::new();
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(4, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh(&styles).unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh(&styles).unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!("{}{}foo ", Goto(1, 1), clear::CurrentLine),
        );
    }
}
