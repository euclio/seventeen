use std::collections::HashMap;
use std::io;
use std::ops::{Index, IndexMut};

use log::*;
use termion;

use protocol::ViewId;
use screen::{Coordinate, Screen};
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
    pub fn render(&self, screen: &mut Screen) -> io::Result<()> {
        screen.erase();

        for (i, line) in self
            .line_cache
            .iter_lines()
            .enumerate()
            .take(self.rows as usize)
        {
            // There might be a newline at the end of the current line, but the terminal already
            // operates linewise.
            let line = line.trim_right_matches('\n');
            screen.write_str((i as u16, 0).into(), line);
        }

        // Add any cursors or styles to the current line.
        for cursor in self.line_cache.iter_cursors() {
            screen.draw_cursor(cursor);
        }

        // If there are more rows in the window than lines in the cache, then fill out the
        // remaining rows with tildes.
        let starting_line_no = self.line_cache.len() as u16;

        // FIXME: Don't display the trailing newline here.
        //
        // To do this, we need to distinguish between the file having a trailing newline and a file
        // having its last line being written. It'd be nice to use the cursor for this, but the
        // cursor doesn't actually get its correct position until the `scroll_to` notification gets
        // sent later.

        for line_no in starting_line_no..self.rows {
            screen.write_str((line_no, 0).into(), "~");
        }

        Ok(())
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
