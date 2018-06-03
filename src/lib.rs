#![feature(use_extern_macros)]
#![warn(unused_extern_crates)]

extern crate failure;
extern crate futures;
extern crate log;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate termion;

mod core;
mod editor;
mod protocol;
mod terminal;

pub use core::Core;
pub use editor::Editor;

pub enum Event {
    Input(termion::event::Event),
    CoreRpc(protocol::Message),
}
