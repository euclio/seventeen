use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use futures::Future;
use log::*;
use termion::{
    self, event::{Event as TEvent, Key},
};

use core::Core;
use protocol::{Message, Notification, Update, ViewId};
use Event;

mod line_cache;
mod window;

use self::line_cache::LineCache;
use self::window::Window;

pub struct Editor<W> {
    core: Core,
    windows: HashMap<ViewId, Window>,
    screen: W,
}

impl<W: Write> Editor<W> {
    pub fn new(core: Core, screen: W) -> Self {
        let mut editor = Self {
            core,
            windows: HashMap::new(),
            screen,
        };

        editor.core.client_started().unwrap();
        editor
    }

    pub fn new_window<P: Into<PathBuf>>(&mut self, path: Option<P>) {
        let view_id = self.core.new_view(path).wait().unwrap();

        let (cols, rows) = termion::terminal_size().unwrap();
        let window = Window {
            rows,
            cols,
            line_cache: LineCache::new(),
        };
        info!(
            "creating window at {:?}: width={} height={}",
            (1, 1),
            cols,
            rows
        );

        // Rows are one-based, but `scroll` takes the first and last lines non-inclusively, so we
        // don't have to subtract 1.
        self.core.scroll(view_id.clone(), (0, rows)).unwrap();

        self.windows.insert(view_id, window);
    }

    pub fn update(&mut self, view_id: ViewId, update: Update) {
        let window = self
            .windows
            .get_mut(&view_id)
            .expect("got update for non-existent view");

        window.line_cache.update(update);
        window.render(&mut self.screen).unwrap();
        self.screen.flush().unwrap();
    }

    pub fn run<E>(mut self, events: E)
    where
        E: IntoIterator<Item = Event>,
    {
        for event in events {
            match event {
                Event::Input(TEvent::Key(Key::Char('q'))) => {
                    break;
                }
                Event::CoreRpc(Message::Notification(not)) => match not {
                    Notification::Update { view_id, update } => self.update(view_id, update),
                    _ => error!("unhandled notification: {:?}", not),
                },
                Event::CoreRpc(Message::Request { id, req }) => unreachable!(
                    "xi-core is not known to send RPC requests, but got a request ({:?}, {:?})",
                    id, req
                ),
                Event::CoreRpc(Message::Response { .. }) => {
                    unreachable!("responses are handled by the core")
                }
                _ => (),
            }
        }
    }
}
