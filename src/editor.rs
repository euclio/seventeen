use std::path::PathBuf;

use channel::{select, Receiver};
use futures::Future;
use log::*;
use termion::event::Key;
use xdg::BaseDirectories;

use crate::core::Core;
use crate::protocol::{ConfigChanges, Notification, ThemeSettings, Update, ViewId};
use crate::screen::{Color, Coordinate, Screen, Style};
use serde_json::Value;

mod line_cache;
mod window;

use self::window::WindowMap;

/// Returned when the editor should begin teardown.
#[derive(Debug)]
struct ExitRequest;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

pub struct Editor {
    core: Core,
    windows: WindowMap,
    mode: Mode,
    screen: Screen,
}

impl Editor {
    pub fn new<P: Into<PathBuf>>(mut core: Core, initial_path: Option<P>) -> Self {
        let xdg_dirs = BaseDirectories::with_prefix("xi").unwrap();
        core.client_started(Some(xdg_dirs.get_config_home()))
            .unwrap();
        let view_id = core.new_view(initial_path).wait().unwrap();
        let windows = WindowMap::new(&mut core, view_id);

        Self {
            core,
            windows,
            mode: Mode::Normal,
            screen: Screen::new().unwrap(),
        }
    }

    fn update(&mut self, view_id: ViewId, update: Update) {
        let window = &mut self.windows[&view_id];

        window.line_cache.update(update);
        window.render(&mut self.screen).unwrap();
        self.screen.refresh().unwrap();
    }

    fn scroll_to(&mut self, view_id: ViewId, line: u64, col: u64) {
        let active_window = self.windows.get_active_window_mut();
        debug_assert!(view_id == active_window.view_id);
        active_window.scroll_to(Coordinate {
            y: line as u16,
            x: col as u16,
        });
        active_window.render(&mut self.screen).unwrap();
        self.screen.refresh().unwrap();
    }

    fn config_changed(&mut self, changes: ConfigChanges) {
        // This will likely break in the future. `theme` is a nonstandard configuration option.
        // See https://github.com/google/xi-editor/issues/722 for the motivation.
        if let Some(Value::String(theme)) = changes.other.get("theme") {
            self.core.set_theme(&theme).unwrap();
        }
    }

    fn theme_changed(&mut self, name: String, theme: ThemeSettings) {
        info!("theme changed to {}", name);
        self.screen.set_text_color(
            theme.foreground.map(Into::into),
            theme.background.map(Into::into),
        );
    }

    fn move_up(&mut self) {
        let active_window = self.windows.get_active_window_mut();

        // If the `move_up` RPC is sent while the cursor is in the top row, the cursor will move to
        // the beginning of the line. vim will keep the cursor at the same position.
        if active_window.cursor.y == 0 {
            return;
        }

        self.core.move_up(active_window.view_id.clone()).unwrap();
    }

    fn move_down(&mut self) {
        let active_window = self.windows.get_active_window_mut();

        // If the `move_down` RPC is sent while the cursor is on the bottom row, the cursor will
        // move to the end of the line. vim will keep the cursor at the same position.
        if (active_window.cursor.y as usize) >= active_window.buffer_len() {
            return;
        }

        self.core.move_down(active_window.view_id.clone()).unwrap();
    }

    fn move_left(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        if active_window.cursor.x > 0 {
            self.core.move_left(active_window.view_id.clone()).unwrap();
        }
    }

    fn move_right(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        if !active_window.line_cache.is_eol(&active_window.cursor) {
            self.core.move_right(active_window.view_id.clone()).unwrap();
        }
    }

    fn move_word_left(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        self.core.move_word_left(active_window.view_id.clone()).unwrap();
    }

    fn move_word_right(&mut self) {
        let active_window = self.windows.get_active_window_mut();
        self.core.move_word_right(active_window.view_id.clone()).unwrap();

        // vim moves to the first letter of each word, while xi will move to the space between.
        self.move_right();
    }

    fn handle_normal_key(&mut self, key: Key) {
        match key {
            Key::Char('b') => {
                self.move_word_left();
            }
            Key::Char('h') => {
                self.move_left();
            }
            Key::Char('i') => {
                info!("entering insert mode");
                self.mode = Mode::Insert;
            }
            Key::Char('j') => {
                self.move_down();
            }
            Key::Char('k') => {
                self.move_up();
            }
            Key::Char('l') => {
                self.move_right();
            }
            Key::Char('w') => {
                self.move_word_right();
            }
            _ => warn!("unhandled key: {:?}", key),
        }
    }

    fn handle_insert_key(&mut self, key: Key) {
        match key {
            Key::Char(c) => {
                self.core
                    .insert(
                        self.windows.get_active_window_mut().view_id.clone(),
                        c.to_string(),
                    )
                    .unwrap();
            }
            Key::Backspace => {
                self.core
                    .delete_backward(self.windows.get_active_window_mut().view_id.clone())
                    .unwrap();
            }
            Key::Esc => {
                info!("entering normal mode");
                self.mode = Mode::Normal;
            }
            _ => warn!("unhandled key: {:?}", key),
        }
    }

    fn handle_notification(&mut self, notification: Notification) {
        match notification {
            Notification::Update { view_id, update } => self.update(view_id, update),
            Notification::DefStyle {
                id,
                fg_color,
                bg_color,
                weight,
                underline,
                italic,
            } => {
                self.screen.define_style(
                    id,
                    Style {
                        fg: fg_color.map(Color::from_argb),
                        bg: bg_color.map(Color::from_argb),
                        bold: weight.map(|weight| weight >= 700).unwrap_or_default(),
                        underline: underline.unwrap_or_default(),
                        italic: italic.unwrap_or_default(),
                    },
                );
            }
            Notification::ScrollTo { view_id, line, col } => self.scroll_to(view_id, line, col),
            Notification::ConfigChanged { changes, .. } => {
                // TODO: Handle view_id
                self.config_changed(changes);
            }
            Notification::ThemeChanged { name, theme } => self.theme_changed(name, theme),
            Notification::PluginStarted { view_id, plugin } => {
                info!("{} started on {:?}", plugin, view_id);
            }
            Notification::AvailableThemes { themes } => {
                info!("available themes: {:?}", themes);
            }
            Notification::AvailablePlugins { view_id, plugins } => {
                info!("available plugins for view {}: {:?}", view_id, plugins);
            }
            _ => error!("unhandled notification: {:?}", notification),
        }
    }

    fn handle_input(&mut self, key: Key) -> Option<ExitRequest> {
        match self.mode {
            Mode::Normal => match key {
                Key::Char('q') => return Some(ExitRequest),
                _ => self.handle_normal_key(key),
            },
            Mode::Insert => self.handle_insert_key(key),
        }

        None
    }

    pub fn run(mut self, input: Receiver<Key>, notifications: Receiver<Notification>) {
        loop {
            select! {
                recv(input, key) => if let Some(ExitRequest) = self.handle_input(key.unwrap()) {
                    break
                },
                recv(notifications, notification) => self.handle_notification(notification.unwrap()),
            }
        }
    }
}
