use std::path::PathBuf;

use futures::Future;
use log::*;
use termion::event::Key;
use xdg::BaseDirectories;

use core::Core;
use protocol::{ConfigChanges, Notification, ThemeSettings, Update, ViewId};
use screen::{Color, Coordinate, Screen, Style};
use Event;

mod line_cache;
mod window;

use self::window::WindowMap;

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
        active_window.cursor = Coordinate {
            y: line as u16,
            x: col as u16,
        };
        debug!("scrolled cursor to {:?}", active_window.cursor);
    }

    fn config_changed(&mut self, changes: ConfigChanges) {
        self.core.set_theme(&changes.theme).unwrap();
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

        if active_window.cursor.y > 0 {
            self.core.move_up(active_window.view_id.clone()).unwrap();
        }
    }

    fn move_down(&mut self) {
        let active_window = self.windows.get_active_window_mut();

        if active_window.cursor.y < active_window.rows {
            self.core.move_down(active_window.view_id.clone()).unwrap();
        }
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

    fn handle_normal_key(&mut self, key: Key) {
        match key {
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

    pub fn run<E>(mut self, events: E)
    where
        E: IntoIterator<Item = Event>,
    {
        for event in events {
            match event {
                Event::Input(Key::Char('q')) if self.mode == Mode::Normal => break,
                Event::Input(key) => match self.mode {
                    Mode::Normal => self.handle_normal_key(key),
                    Mode::Insert => self.handle_insert_key(key),
                },
                Event::CoreNotification(not) => match not {
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
                    Notification::ScrollTo { view_id, line, col } => {
                        self.scroll_to(view_id, line, col)
                    }
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
                    _ => error!("unhandled notification: {:?}", not),
                },
            }
        }
    }
}
