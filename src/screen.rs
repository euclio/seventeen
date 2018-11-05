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

    /// Styles defined by the backend.
    styles: Vec<Style>,

    /// Foreground and background color for text.
    text_color: (Option<Color>, Option<Color>),

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
            styles: vec![Style::default(); RESERVED_STYLES],
            text_color: (None, None),
        })
    }
}

impl<W: Write> Screen<W> {
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

        if id as usize >= self.styles.len() {
            self.styles.resize(id as usize + 1, Style::default());
        }

        self.styles[id as usize] = style;
    }

    fn style(&self, id: u64) -> &Style {
        &self.styles[id as usize]
    }

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

    pub fn set_text_color(&mut self, fg: Option<Color>, bg: Option<Color>) {
        self.text_color = (fg, bg);
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
    pub fn refresh(&mut self) -> io::Result<()> {
        debug!("refreshing screen contents");

        if let Some(fg) = &self.text_color.0 {
            write!(self.out, "{}", Fg(Rgb(fg.r, fg.g, fg.b)))?;
        }

        if let Some(bg) = &self.text_color.1 {
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
                let starting_style = cell.span_start.map(|id| self.style(id).clone());
                let ending_style = cell.span_end.map(|id| self.style(id).clone());

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

    use super::{Color, Coordinate, Screen, Style};

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "Hello, world!");
        screen.define_style(
            1,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.apply_style(Coordinate::new(0, 0), 1, 5);
        screen.refresh().unwrap();

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "bolditalic");
        screen.define_style(
            1,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.define_style(
            2,
            Style {
                italic: true,
                ..Default::default()
            },
        );
        screen.apply_style(Coordinate::new(0, 0), 1, 4);
        screen.apply_style(Coordinate::new(4, 0), 2, 6);
        screen.refresh().unwrap();

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "int main() {}");
        screen.define_style(
            2,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.define_style(
            3,
            Style {
                bold: false,
                ..Default::default()
            },
        );
        screen.define_style(
            4,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.apply_style(Coordinate::new(3, 0), 3, 1);
        screen.apply_style(Coordinate::new(4, 0), 4, 4);
        screen.refresh().unwrap();

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(10, 3), buf).unwrap();
        screen.write_str(Coordinate::new(0, 0).into(), "foobarbaz");
        screen.write_str(Coordinate::new(0, 1).into(), "quux quux");
        screen.write_str(Coordinate::new(0, 2).into(), "xyzzyxyzzy");
        screen.define_style(
            2,
            Style {
                bold: true,
                ..Default::default()
            },
        );
        screen.define_style(
            3,
            Style {
                italic: true,
                ..Default::default()
            },
        );
        screen.define_style(
            4,
            Style {
                underline: true,
                ..Default::default()
            },
        );

        screen.apply_style(Coordinate::new(0, 0), 2, 20);
        screen.apply_style(Coordinate::new(5, 1), 3, 10);
        screen.apply_style(Coordinate::new(20, 2), 4, 5);
        screen.refresh().unwrap();

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(15, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "redgreenblue");
        screen.define_style(
            2,
            Style {
                fg: Some(Color { r: 255, g: 0, b: 0 }),
                ..Default::default()
            },
        );
        screen.define_style(
            3,
            Style {
                fg: Some(Color { r: 0, g: 255, b: 0 }),
                ..Default::default()
            },
        );
        screen.define_style(
            4,
            Style {
                fg: Some(Color { r: 0, g: 0, b: 255 }),
                ..Default::default()
            },
        );
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.apply_style(Coordinate::new(3, 0), 3, 5);
        screen.apply_style(Coordinate::new(8, 0), 4, 4);
        screen.refresh().unwrap();

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

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.define_style(
            2,
            Style {
                bold: true,
                italic: true,
                underline: true,
                ..Default::default()
            },
        );
        screen.apply_style(Coordinate::new(0, 0), 2, 3);
        screen.draw_cursor(Coordinate::new(1, 0));
        screen.refresh().unwrap();

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
        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(Size2D::new(4, 1), buf).unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh().unwrap();

        screen.write_str(Coordinate::new(0, 0), "foo");
        screen.refresh().unwrap();

        let sequences = String::from_utf8(screen.out.into_inner()).unwrap();
        assert_eq!(
            sequences,
            format!("{}{}foo ", Goto(1, 1), clear::CurrentLine),
        );
    }
}
