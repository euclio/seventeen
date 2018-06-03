#![feature(use_extern_macros)]
#![warn(unused_extern_crates)]

extern crate log;
extern crate log4rs;
extern crate log_panics;
extern crate seventeen;
extern crate structopt;
extern crate termion;

use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use log::*;
use log4rs::{
    append::file::FileAppender, config::{Appender, Config, Logger, Root},
    encode::pattern::PatternEncoder,
};
use structopt::StructOpt;
use termion::{input::TermRead, raw::IntoRawMode, screen::AlternateScreen};

use seventeen::{Core, Editor, Event};

#[derive(Debug, StructOpt)]
struct Opt {
    /// The file to open.
    #[structopt(parse(from_os_str))]
    file: Option<PathBuf>,

    /// Write log messages to this file
    #[structopt(long = "log-file", parse(from_os_str), default_value = "/tmp/seventeen.log")]
    log_file: PathBuf,
}

fn run(opt: Opt) -> Result<(), Box<Error>> {
    let mut screen = AlternateScreen::from(io::stdout().into_raw_mode()?);
    write!(screen, "{}{}", termion::cursor::Hide, termion::clear::All)?;
    screen.flush()?;

    let (event_tx, event_rx) = mpsc::channel::<Event>();

    let input_event_tx = event_tx.clone();
    thread::spawn(move || -> io::Result<()> {
        let tty = termion::get_tty()?;

        for event in tty.events() {
            input_event_tx.send(Event::Input(event?)).unwrap();
        }

        Ok(())
    });

    let core = Core::spawn(event_tx.clone())?;
    let mut editor = Editor::new(core, screen);

    editor.new_window(opt.file);

    editor.run(event_rx);

    // We hid the cursor earlier, so we have to restore it before we exit.
    print!("{}", termion::cursor::Show);
    io::stdout().flush()?;

    Ok(())
}

fn init_logging(opt: &Opt) -> Result<(), Box<Error>> {
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} {M} - {m}\n")))
        .build(&opt.log_file)?;

    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .logger(
            Logger::builder().build(env!("CARGO_PKG_NAME").replace('-', "_"), LevelFilter::Trace),
        )
        .build(Root::builder().appender("file").build(LevelFilter::Warn))?;

    log4rs::init_config(config)?;

    Ok(())
}

fn main() {
    let opt = Opt::from_args();

    log_panics::init();
    init_logging(&opt).unwrap();

    run(opt).unwrap();
}
