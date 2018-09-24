#![warn(unused_extern_crates)]

extern crate crossbeam_channel as channel;
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

pub use core::Core;
pub use editor::Editor;
pub use protocol::Notification;
