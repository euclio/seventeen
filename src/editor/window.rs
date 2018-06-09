use std::collections::HashMap;
use std::fmt::Write;
use std::io;
use std::ops::{Index, IndexMut};

use log::*;
use termion::{self, clear, cursor, style};

use protocol::ViewId;
use terminal::Coordinate;
use Core;

use super::line_cache::LineCache;

#[derive(Debug)]
pub struct Window {
    pub line_cache: LineCache,
    pub rows: u16,
    pub cols: u16,
    pub cursor: Coordinate,
    pub view_id: ViewId,
}

impl Window {
    pub fn render<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let mut sequences = String::new();

        let mut cursors = self.line_cache.iter_cursors().collect::<Vec<_>>();

        // Sort cursors so that we can iterate through them top to bottom, left to right
        cursors.sort_by_key(|cursor| (cursor.y, cursor.x));

        for (i, line) in self
            .line_cache
            .iter_lines()
            .enumerate()
            .take(self.rows as usize)
        {
            let line_no = (i + 1) as u16;

            // There might be a newline at the end of the current line, but we're already iterating
            // linewise.
            let mut line = String::from(line.trim_right_matches('\n'));

            // Add any cursors or styles to the current line. We have to add them in reverse order
            // so we don't invalidate the indices.
            for cursor in cursors.iter().rev() {
                if cursor.y == line_no {
                    insert_cursor(&mut line, u64::from(cursor.x - 1));
                }
            }

            write!(
                sequences,
                "{}{}{}",
                cursor::Goto(1, line_no),
                clear::CurrentLine,
                line
            ).unwrap();
        }

        // If there are more rows in the window than lines in the cache, then fill out the
        // remaining rows with tildes.
        let starting_line_no = self.line_cache.len() as u16 + 1;

        // FIXME: Don't display the trailing newline here.
        //
        // To do this, we need to distinguish between the file having a trailing newline and a file
        // having its last line being written. It'd be nice to use the cursor for this, but the
        // cursor doesn't actually get its correct position until the `scroll_to` notification gets
        // sent later.

        for line_no in starting_line_no..=self.rows {
            write!(
                sequences,
                "{}{}~",
                cursor::Goto(1, line_no),
                clear::CurrentLine,
            ).unwrap();
        }

        write!(writer, "{}", sequences)?;
        writer.flush()?;

        Ok(())
    }
}

/// Draws a "cursor" at the given byte index in the line.
///
/// Since a terminal can only support a single cursor at a time, we simulate multiple cursors by
/// drawing a reverse highlight wherever we need a cursor.
fn insert_cursor(line: &mut String, index: u64) {
    // Event if the line is empty, we still need to have a character on the line to highlight.
    if line.is_empty() {
        *line = String::from(" ");
    }

    let index = index as usize;
    if let Some(c) = line.chars().nth(index) {
        // FIXME: The range here might not work for multibyte characters. Write a test.
        line.replace_range(
            index..index + 1,
            &format!("{}{}{}", style::Invert, c, style::NoInvert),
        );
    } else {
        line.push_str(&format!("{} {}", style::Invert, style::NoInvert));
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
            cursor: Coordinate { x: 1, y: 1 },
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
mod tests {
    use termion::style;

    #[test]
    fn insert_cursor() {
        let mut line = String::from("");
        super::insert_cursor(&mut line, 0);
        assert_eq!(line, format!("{} {}", style::Invert, style::NoInvert));

        let mut line = String::from("test");
        super::insert_cursor(&mut line, 0);
        assert_eq!(line, format!("{}t{}est", style::Invert, style::NoInvert));

        let mut line = String::from("Hello, world!");
        super::insert_cursor(&mut line, 7);
        assert_eq!(
            line,
            format!("Hello, {}w{}orld!", style::Invert, style::NoInvert),
        );

        let mut line = String::from("Goodbye, world!");
        super::insert_cursor(&mut line, 15);
        assert_eq!(
            line,
            format!("Goodbye, world!{} {}", style::Invert, style::NoInvert),
        );
    }
}
