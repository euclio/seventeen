use std::collections::HashMap;
use std::io::{self, Write};
use std::ops::{Index, IndexMut};

use log::*;
use termion;

use protocol::ViewId;
use screen::{Coordinate, Screen};
use Core;

use super::line_cache::LineCache;

#[derive(Debug)]
pub struct Window {
    pub row_offset: u16,
    pub col_offset: u16,
    pub line_cache: LineCache,
    pub rows: u16,
    pub cols: u16,
    pub cursor: Coordinate,
    pub view_id: ViewId,
}

impl Window {
    pub fn render<W: Write>(&self, screen: &mut Screen<W>) -> io::Result<()> {
        screen.erase();

        for (i, line) in self
            .line_cache
            .iter_lines()
            .skip(self.row_offset.into())
            .enumerate()
            .take(self.rows.into())
        {
            // There might be a newline at the end of the current line, but the terminal already
            // operates linewise.
            let text = line
                .text
                .trim_right_matches('\n')
                .chars()
                .skip(self.col_offset.into())
                .take(self.cols.into()) // FIXME: this width check is bogus for non-ASCII
                .collect::<String>();
            screen.write_str((i as u16, 0).into(), &text);

            for mut style_span in line.iter_style_spans() {
                // Skip any spans that end before the column offset or start after the end of the
                // screen.
                if style_span.start + style_span.length <= self.col_offset.into()
                    || usize::from(self.col_offset + self.cols) <= style_span.start {
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
                    || u64::from(self.col_offset + self.cols) <= offset
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

        for line_no in starting_line_no..self.rows {
            screen.write_str((line_no, 0).into(), "~");
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
    pub fn scroll_to(&mut self, coordinate: Coordinate) {
        self.cursor = coordinate;

        if self.cursor.x >= self.col_offset + self.cols {
            self.col_offset = self.cursor.x - (self.cols - 1);
        } else if self.cursor.x < self.col_offset {
            self.col_offset = self.cursor.x;
        }

        if self.cursor.y > self.row_offset + self.rows - 1 {
            self.row_offset = self.cursor.y - (self.rows - 1);
        } else if self.cursor.y < self.row_offset {
            self.row_offset = self.cursor.y;
        }

        debug!(
            "scrolled cursor to {:?}, row_offset={}, col_offset={}",
            self.cursor, self.row_offset, self.col_offset
        );
    }
}

#[derive(Debug)]
pub struct WindowMap {
    map: HashMap<ViewId, Window>,
    active_window: ViewId,
}

impl WindowMap {
    pub fn new(core: &mut Core, active_view_id: ViewId) -> Self {
        let (cols, rows) = termion::terminal_size().unwrap();
        let window = Window {
            col_offset: 0,
            row_offset: 0,
            cursor: (0, 0).into(),
            rows,
            cols,
            line_cache: LineCache::new(),
            view_id: active_view_id.clone(),
        };
        info!(
            "creating window at {:?}: width={} height={}",
            (1, 1),
            cols,
            rows
        );

        // Rows are one-based, but `scroll` takes the first and last lines non-inclusively, so we
        // don't have to subtract 1.
        core.scroll(active_view_id.clone(), (0, rows)).unwrap();

        let mut window_map = HashMap::new();
        window_map.insert(active_view_id.clone(), window);

        Self {
            map: window_map,
            active_window: active_view_id,
        }
    }

    pub fn get_active_window_mut(&mut self) -> &mut Window {
        self.map.get_mut(&self.active_window).unwrap()
    }
}

impl<'a> Index<&'a ViewId> for WindowMap {
    type Output = Window;

    fn index(&self, index: &'a ViewId) -> &Window {
        self.map.get(index).unwrap()
    }
}

impl<'a> IndexMut<&'a ViewId> for WindowMap {
    fn index_mut(&mut self, index: &'a ViewId) -> &mut Window {
        self.map.get_mut(index).unwrap()
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use super::{LineCache, Screen, ViewId, Window};
    use screen::Coordinate;

    #[test]
    fn window_smaller_than_cache() {
        const ROWS: u16 = 1;
        const COLS: u16 = 5;

        let cache = LineCache::new_from_lines(&["hello, world!", "goodbye, world!"]);

        let window = Window {
            row_offset: 0,
            col_offset: 0,
            line_cache: cache,
            cursor: (0, 0).into(),
            rows: ROWS,
            cols: COLS,
            view_id: ViewId("view-id-1".to_string()),
        };

        let buf = Cursor::new(vec![]);
        let mut screen = Screen::new_from_write(ROWS as usize, COLS as usize, buf).unwrap();

        window.render(&mut screen).unwrap();
    }

    #[test]
    fn scroll_to() {
        const ROWS: u16 = 5;
        const COLS: u16 = 10;

        let cache = LineCache::new_from_lines(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);

        let mut window = Window {
            row_offset: 0,
            col_offset: 0,
            line_cache: cache,
            cursor: (0, 0).into(),
            rows: ROWS,
            cols: COLS,
            view_id: ViewId("view-id-1".to_string()),
        };

        window.scroll_to(Coordinate { x: 0, y: 3 });
        assert_eq!(window.row_offset, 0);

        window.scroll_to(Coordinate { x: 0, y: 5 });
        assert_eq!(window.row_offset, 1);

        window.scroll_to(Coordinate { x: 0, y: 7 });
        assert_eq!(window.row_offset, 3);

        window.scroll_to(Coordinate { x: 0, y: 3 });
        assert_eq!(window.row_offset, 3);

        window.scroll_to(Coordinate { x: 0, y: 0 });
        assert_eq!(window.row_offset, 0);
    }
}
