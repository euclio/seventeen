use std::collections::HashMap;

use euclid::{Rect, Size2D};
use log::*;

use crate::protocol::ViewId;

#[derive(Debug)]
pub struct Layout {
    screen: Size2D<usize>,
    windows: HashMap<ViewId, Rect<usize>>,
}

impl Layout {
    pub fn new(screen_size: Size2D<usize>) -> Self {
        Layout {
            screen: screen_size,
            windows: Default::default(),
        }
    }

    pub fn add_view(&mut self, view_id: &ViewId) -> Rect<usize> {
        if self.windows.len() >= 1 {
            unimplemented!("only one view is supported");
        }

        // TODO: implement some fancy placement logic here.
        let rect = Rect::from_size(self.screen);
        self.windows.insert(view_id.clone(), rect);

        info!("created window at {:?}", rect);

        rect
    }

    /// Returns a bounding rectangle for the given view.
    ///
    /// # Panics
    ///
    /// Panics if the view id is not contained in the layout.
    pub fn of_view(&self, view_id: &ViewId) -> Rect<usize> {
        self.windows[view_id]
    }
}
