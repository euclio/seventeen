#![feature(use_extern_macros)]
#![warn(unused_extern_crates)]

extern crate failure;
extern crate futures;
extern crate log;
extern crate ndarray;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate termion;
extern crate xdg;

mod core;
mod editor;
mod protocol;
mod screen;

use protocol::Message;
use termion::event::Key;

pub use core::Core;
pub use editor::Editor;

pub enum Event {
    Input(Key),
    CoreRpc(Message),
}
