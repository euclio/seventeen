use std::io::{self, Write};

use euclid::Rect;
use log::*;

use super::line_cache::LineCache;
use crate::screen::{Coordinate, Screen};

#[derive(Debug)]
pub struct Window {
    pub cursor: Coordinate,
    pub line_cache: LineCache,
    row_offset: u16,
    col_offset: u16,
}

impl Window {
    pub fn new() -> Self {
        Self {
            row_offset: 0,
            col_offset: 0,
            line_cache: LineCache::new(),
            cursor: (0, 0).into(),
        }
    }

    pub fn render<W: Write>(&self, bounds: &Rect<usize>, screen: &mut Screen<W>) -> io::Result<()> {
        let start = usize::from(self.row_offset);
        let end = start + usize::from(bounds.size.height);

        // If there are more rows in the window than are in the cache, skip rendering it. The next
        // cache update will contain enough rows.
        let lines = match self.line_cache.iter_lines(start..=end) {
            Some(lines) => lines,
            None => return Ok(()),
        };

        screen.erase();

        for (i, line) in lines.enumerate() {
            // There might be a newline at the end of the current line, but the terminal already
            // operates linewise.
            let text = line
                .text
                .trim_right_matches('\n')
                .chars()
                .skip(self.col_offset.into())
                .take(bounds.size.width) // FIXME: this width check is bogus for non-ASCII
                .collect::<String>();
            screen.write_str((i as u16, 0).into(), &text);

            for mut style_span in line.iter_style_spans() {
                // Skip any spans that end before the column offset or start after the end of the
                // screen.
                if style_span.start + style_span.length <= self.col_offset.into()
                    || usize::from(self.col_offset + bounds.size.width as u16) <= style_span.start
                {
                    continue;
                }

                if style_span.start < usize::from(self.col_offset) {
                    style_span.length -= usize::from(self.col_offset) - style_span.start;
                    style_span.start = 0;
                } else {
                    style_span.start -= usize::from(self.col_offset);
                }

                screen.apply_style(
                    style_span.id,
                    (i as u16, style_span.start as u16).into(),
                    style_span.length,
                );
            }

            for offset in line.iter_cursors() {
                // Skip any cursors that aren't on the screen.
                if offset < self.col_offset.into()
                    || u64::from(self.col_offset + bounds.size.width as u16) <= offset
                {
                    continue;
                }

                screen.draw_cursor(Coordinate {
                    x: offset as u16 - self.col_offset,
                    y: i as u16,
                });
            }
        }

        // If there are more rows in the window than lines in the cache, then fill out the
        // remaining rows with tildes.
        let starting_line_no = self.line_cache.len() as u16;

        // FIXME: Don't display the trailing newline here.
        // To do this, we need to distinguish between the file having a trailing newline and a file
        // having its last line being written. It'd be nice to use the cursor for this, but the
        // cursor doesn't actually get its correct position until the `scroll_to` notification gets
        // sent later.

        for line_no in usize::from(starting_line_no)..bounds.size.height {
            screen.write_str((line_no as u16, 0).into(), "~");
        }

        Ok(())
    }

    /// The total number of lines in the window's buffer.
    pub fn buffer_len(&self) -> usize {
        self.line_cache.len()
    }

    /// Scrolls the main cursor in a window to a coordinate.
    ///
    /// If the coordinate is off-screen, an internal offset is updated so that the cursor will
    /// appear on-screen on the next screen refresh.
    pub fn scroll_to(&mut self, bounds: &Rect<usize>, coordinate: Coordinate) {
        self.cursor = coordinate;

        if self.cursor.x >= self.col_offset + bounds.size.width as u16 {
            self.col_offset = self.cursor.x - (bounds.size.width as u16 - 1);
        } else if self.cursor.x < self.col_offset {
            self.col_offset = self.cursor.x;
        }

        if self.cursor.y > self.row_offset + bounds.size.height as u16 - 1 {
            self.row_offset = self.cursor.y - (bounds.size.height as u16 - 1);
        } else if self.cursor.y < self.row_offset {
            self.row_offset = self.cursor.y;
        }

        debug!(
            "scrolled cursor to {:?}, row_offset={}, col_offset={}",
            self.cursor, self.row_offset, self.col_offset
        );
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use euclid::{Rect, Size2D};

    use super::{LineCache, Screen, Window};
    use crate::screen::Coordinate;

    #[test]
    fn cache_smaller_than_window() {
        let bounds = Rect::from_size(Size2D::new(5, 5));
        let mut window = Window::new();
        window.line_cache = LineCache::new_from_lines(&["foo", "bar"]);

        let buf = Cursor::new(vec![]);
        let mut screen =
            Screen::new_from_write(bounds.size.height, bounds.size.width, buf).unwrap();

        window.render(&bounds, &mut screen).unwrap();
    }

    #[test]
    fn window_smaller_than_cache() {
        let bounds = Rect::from_size(Size2D::new(5, 1));
        let mut window = Window::new();
        window.line_cache = LineCache::new_from_lines(&["hello, world!", "goodbye, world!"]);

        let buf = Cursor::new(vec![]);
        let mut screen =
            Screen::new_from_write(bounds.size.height, bounds.size.width, buf).unwrap();

        window.render(&bounds, &mut screen).unwrap();
    }

    #[test]
    fn scroll_to() {
        let bounds = Rect::from_size(Size2D::new(10, 5));

        let mut window = Window::new();
        window.line_cache =
            LineCache::new_from_lines(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);

        window.scroll_to(&bounds, Coordinate { x: 0, y: 3 });
        assert_eq!(window.row_offset, 0);

        window.scroll_to(&bounds, Coordinate { x: 0, y: 5 });
        assert_eq!(window.row_offset, 1);

        window.scroll_to(&bounds, Coordinate { x: 0, y: 7 });
        assert_eq!(window.row_offset, 3);

        window.scroll_to(&bounds, Coordinate { x: 0, y: 3 });
        assert_eq!(window.row_offset, 3);

        window.scroll_to(&bounds, Coordinate { x: 0, y: 0 });
        assert_eq!(window.row_offset, 0);
    }
}
