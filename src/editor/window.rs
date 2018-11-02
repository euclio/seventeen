use std::io::{self, Write};

use euclid::{Rect, SideOffsets2D};
use log::*;

use super::line_cache::LineCache;
use crate::screen::{Coordinate, Screen};

#[derive(Debug)]
pub struct Window {
    pub cursor: Coordinate,
    pub line_cache: LineCache,

    /// The offsets of the window compared to the contents of the cache. Used for scrolling the
    /// window.
    offsets: SideOffsets2D<usize>,
}

impl Window {
    pub fn new() -> Self {
        Self {
            offsets: SideOffsets2D::zero(),
            line_cache: LineCache::new(),
            cursor: Coordinate::zero(),
        }
    }

    pub fn render<W: Write>(&self, bounds: &Rect<usize>, screen: &mut Screen<W>) -> io::Result<()> {
        let start = self.offsets.top;
        let end = start + bounds.size.height;

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
                .skip(self.offsets.left)
                .take(bounds.size.width) // FIXME: this width check is bogus for non-ASCII
                .collect::<String>();
            screen.write_str(Coordinate::new(0, i), &text);

            for mut style_span in line.iter_style_spans() {
                // Skip any spans that end before the column offset or start after the end of the
                // screen.
                if style_span.start + style_span.length <= self.offsets.left
                    || self.offsets.left + bounds.size.width <= style_span.start
                {
                    continue;
                }

                if style_span.start < self.offsets.left {
                    style_span.length -= self.offsets.left - style_span.start;
                    style_span.start = 0;
                } else {
                    style_span.start -= self.offsets.left;
                }

                screen.apply_style(
                    Coordinate::new(style_span.start, i),
                    style_span.id,
                    style_span.length,
                );
            }

            for offset in line.iter_cursors() {
                // Skip any cursors that aren't on the screen.
                if offset < self.offsets.left.into()
                    || self.offsets.left + bounds.size.width <= offset
                {
                    continue;
                }

                screen.draw_cursor(Coordinate::new(offset - self.offsets.left, i));
            }
        }

        // If there are more rows in the window than lines in the cache, then fill out the
        // remaining rows with tildes.
        let starting_line_no = self.line_cache.len();

        // FIXME: Don't display the trailing newline here.
        // To do this, we need to distinguish between the file having a trailing newline and a file
        // having its last line being written. It'd be nice to use the cursor for this, but the
        // cursor doesn't actually get its correct position until the `scroll_to` notification gets
        // sent later.

        for line_no in starting_line_no..bounds.size.height {
            screen.write_str(Coordinate::new(0, line_no), "~");
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

        if self.cursor.x >= self.offsets.left + bounds.size.width {
            self.offsets.left = self.cursor.x - (bounds.size.width - 1);
        } else if self.cursor.x < self.offsets.left {
            self.offsets.left = self.cursor.x;
        }

        if self.cursor.y > self.offsets.top + bounds.size.height - 1 {
            self.offsets.top = self.cursor.y - (bounds.size.height - 1);
        } else if self.cursor.y < self.offsets.top {
            self.offsets.top = self.cursor.y;
        }

        debug!("scrolled cursor to {:?}, {:?}", self.cursor, self.offsets);
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
        let mut screen = Screen::new_from_write(bounds.size, buf).unwrap();

        window.render(&bounds, &mut screen).unwrap();
    }

    #[test]
    fn window_smaller_than_cache() {
        let bounds = Rect::from_size(Size2D::new(5, 1));
        let mut window = Window::new();
        window.line_cache = LineCache::new_from_lines(&["hello, world!", "goodbye, world!"]);

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(bounds.size, buf).unwrap();

        window.render(&bounds, &mut screen).unwrap();
    }

    #[test]
    fn scroll_to() {
        let bounds = Rect::from_size(Size2D::new(10, 5));

        let mut window = Window::new();
        window.line_cache =
            LineCache::new_from_lines(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);

        window.scroll_to(&bounds, Coordinate::new(0, 3));
        assert_eq!(window.offsets.top, 0);

        window.scroll_to(&bounds, Coordinate::new(0, 5));
        assert_eq!(window.offsets.top, 1);

        window.scroll_to(&bounds, Coordinate::new(0, 7));
        assert_eq!(window.offsets.top, 3);

        window.scroll_to(&bounds, Coordinate::new(0, 3));
        assert_eq!(window.offsets.top, 3);

        window.scroll_to(&bounds, Coordinate::new(0, 0));
        assert_eq!(window.offsets.top, 0);
    }
}
