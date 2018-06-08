use std::io::Write;
use std::path::PathBuf;

use futures::Future;
use log::*;
use termion::event::{Event as TEvent, Key};

use core::Core;
use protocol::{Message, Notification, Update, ViewId};
use Event;

mod line_cache;
mod window;

use self::window::WindowMap;

pub struct Editor<W> {
    core: Core,
    windows: WindowMap,
    screen: W,
}

impl<W: Write> Editor<W> {
    pub fn new<P: Into<PathBuf>>(mut core: Core, screen: W, initial_path: Option<P>) -> Self {
        core.client_started().unwrap();
        let view_id = core.new_view(initial_path).wait().unwrap();
        let windows = WindowMap::new(&mut core, view_id);

        Self {
            core,
            windows,
            screen,
        }
    }

    fn update(&mut self, view_id: ViewId, update: Update) {
        let window = &mut self.windows[&view_id];

        window.line_cache.update(update);
        window.render(&mut self.screen).unwrap();
        self.screen.flush().unwrap();
    }

    fn scroll_to(&mut self, view_id: ViewId, line: u64, col: u64) {
        let active_window = self.windows.get_active_window_mut();
        debug_assert!(view_id == active_window.view_id);
        active_window.cursor.x = col as u16 + 1;
        active_window.cursor.y = line as u16 + 1;
        debug!("scrolled cursor to {:?}", active_window.cursor);
    }

    fn move_up(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        self.core.move_up(active_window.view_id.clone()).unwrap();
    }

    fn move_down(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        self.core.move_down(active_window.view_id.clone()).unwrap();
    }

    fn move_left(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        if active_window.cursor.x != 1 {
            self.core.move_left(active_window.view_id.clone()).unwrap();
        }
    }

    fn move_right(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        if !active_window.line_cache.is_eol(&active_window.cursor) {
            self.core.move_right(active_window.view_id.clone()).unwrap();
        }
    }

    pub fn run<E>(mut self, events: E)
    where
        E: IntoIterator<Item = Event>,
    {
        for event in events {
            match event {
                Event::Input(TEvent::Key(Key::Char('h'))) => {
                    self.move_left();
                }
                Event::Input(TEvent::Key(Key::Char('j'))) => {
                    self.move_down();
                }
                Event::Input(TEvent::Key(Key::Char('k'))) => {
                    self.move_up();
                }
                Event::Input(TEvent::Key(Key::Char('l'))) => {
                    self.move_right();
                }
                Event::Input(TEvent::Key(Key::Char('q'))) => {
                    break;
                }
                Event::CoreRpc(Message::Notification(not)) => match not {
                    Notification::Update { view_id, update } => self.update(view_id, update),
                    Notification::ScrollTo { view_id, line, col } => {
                        self.scroll_to(view_id, line, col)
                    }
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
