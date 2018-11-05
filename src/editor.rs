use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use channel::{select, Receiver};
use euclid::Size2D;
use futures::Future;
use log::*;
use termion::event::Key;
use xdg::BaseDirectories;

use crate::core::Core;
use crate::protocol::{ConfigChanges, Notification, ThemeSettings, Update, ViewId};
use crate::screen::{Color, Coordinate, Screen, Style};
use serde_json::Value;

mod command_line;
mod layout;
mod line_cache;
mod window;

use self::command_line::CommandLine;
use self::layout::Layout;
use self::window::Window;

/// Returned when the editor should begin teardown.
#[derive(Debug)]
struct ExitRequest;

#[derive(Debug)]
enum Mode {
    Normal,
    Insert,
    Command(CommandLine),
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

pub struct Editor {
    core: Core,
    mode: Mode,
    layout: Layout,
    screen: Screen,
    active_view: Option<ViewId>,
    windows: HashMap<ViewId, Window>,
}

impl Editor {
    pub fn new<P: Into<PathBuf>>(mut core: Core, initial_path: Option<P>) -> Self {
        let xdg_dirs = BaseDirectories::with_prefix("xi").unwrap();
        core.client_started(Some(xdg_dirs.get_config_home()))
            .unwrap();

        let (cols, rows) = termion::terminal_size().unwrap();
        let screen_size = Size2D::new(usize::from(cols), usize::from(rows));
        let layout = Layout::new(screen_size);

        let mut editor = Self {
            core,
            layout,
            screen: Screen::new(screen_size).unwrap(),
            active_view: None,
            mode: Mode::Normal,
            windows: HashMap::new(),
        };

        editor.new_view(initial_path).unwrap();

        editor
    }

    fn new_view<P: Into<PathBuf>>(&mut self, path: Option<P>) -> io::Result<()> {
        let view_id = self.core.new_view(path).wait().unwrap();
        let bounds = self.layout.add_view(&view_id);

        self.core.scroll(
            view_id.clone(),
            (bounds.min_y() as u16, bounds.max_y() as u16),
        )?;
        self.windows.insert(view_id.clone(), Window::new());
        self.active_view = Some(view_id);

        Ok(())
    }

    fn update(&mut self, view_id: ViewId, update: Update) {
        let window = self.windows.get_mut(&view_id).unwrap();
        window.line_cache.update(update);
        window
            .render(&self.layout.of_view(&view_id), &mut self.screen)
            .unwrap();
        self.screen.refresh().unwrap();
    }

    fn scroll_to(&mut self, view_id: ViewId, line: usize, col: usize) {
        let window = self.windows.get_mut(&view_id).unwrap();
        let bounds = self.layout.of_view(&view_id);

        window.scroll_to(&bounds, Coordinate::new(col, line));
        window.render(&bounds, &mut self.screen).unwrap();
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
        if let Some(id) = &self.active_view {
            // If the `move_up` RPC is sent while the cursor is in the top row, the cursor will move to
            // the beginning of the line. vim will keep the cursor at the same position.
            if self.windows[&id].cursor.y == 0 {
                return;
            }

            self.core.move_up(id.clone()).unwrap();
        }
    }

    fn move_down(&mut self) {
        if let Some(id) = &self.active_view {
            let window = &self.windows[&id];

            // If the `move_down` RPC is sent while the cursor is on the bottom row, the cursor will
            // move to the end of the line. vim will keep the cursor at the same position.
            if (window.cursor.y as usize) >= window.buffer_len() {
                return;
            }

            self.core.move_down(id.clone()).unwrap();
        }
    }

    fn move_left(&mut self) {
        if let Some(id) = &self.active_view {
            if self.windows[&id].cursor.x > 0 {
                self.core.move_left(id.clone()).unwrap();
            }
        }
    }

    fn move_right(&mut self) {
        if let Some(id) = &self.active_view {
            let window = &self.windows[id];
            if !window.line_cache.is_eol(&window.cursor) {
                self.core.move_right(id.clone()).unwrap();
            }
        }
    }

    fn move_word_left(&mut self) {
        if let Some(id) = &self.active_view {
            self.core.move_word_left(id.clone()).unwrap();
        }
    }

    fn move_word_right(&mut self) {
        if let Some(id) = &self.active_view {
            self.core.move_word_right(id.clone()).unwrap();

            // vim moves to the first letter of each word, while xi will move to the space between.
            self.move_right();
        }
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
            Key::Char(':') => {
                info!("entering command mode");
                let line = CommandLine::new();
                line.render(self.layout.of_command_line(), &mut self.screen);
                self.mode = Mode::Command(line);
            }
            _ => warn!("unhandled key: {:?}", key),
        }
    }

    fn handle_insert_key(&mut self, key: Key) {
        match key {
            Key::Char(c) => {
                if let Some(id) = &self.active_view {
                    self.core.insert(id.clone(), c.to_string()).unwrap();
                }
            }
            Key::Backspace => {
                if let Some(id) = &self.active_view {
                    self.core.delete_backward(id.clone()).unwrap();
                }
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
            Mode::Normal => self.handle_normal_key(key),
            Mode::Insert => self.handle_insert_key(key),
            Mode::Command(ref mut line) => {
                match key {
                    Key::Char('\n') | Key::Esc => {
                        if key == Key::Char('\n') {
                            // TODO: do some parsing here
                            match line.command() {
                                "q" => {
                                    return Some(ExitRequest);
                                }
                                _ => ()
                            }
                        }

                        self.screen.erase_line(self.layout.of_command_line().origin.y);
                        self.screen.refresh().unwrap();
                        info!("entering normal mode");
                        self.mode = Mode::Normal;
                    }
                    Key::Char(c) => {
                        line.insert(c);
                        line.render(self.layout.of_command_line(), &mut self.screen);
                    }
                    Key::Backspace => {
                        line.delete();
                        line.render(self.layout.of_command_line(), &mut self.screen);
                    }
                    _ => warn!("unhandled key: {:?}", key),
                }
            }
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
