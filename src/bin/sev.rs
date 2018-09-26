#![warn(unused_extern_crates)]

use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;

use log::*;
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config, Logger, Root},
    encode::pattern::PatternEncoder,
};
use structopt::StructOpt;
use termion::{
    cursor,
    event::{Event as TermionEvent, Key},
    input::TermRead,
};

use seventeen::{Core, Editor, Notification};

#[derive(Debug, StructOpt)]
struct Opt {
    /// The file to open.
    #[structopt(parse(from_os_str))]
    file: Option<PathBuf>,

    /// Path to the editor core executable
    #[structopt(long = "core", parse(from_os_str), default_value = "xi-core")]
    core: PathBuf,

    /// Write log messages to this file
    #[structopt(long = "log-file", parse(from_os_str), default_value = "/tmp/seventeen.log")]
    log_file: PathBuf,

    /// Log output verbosity
    ///
    /// By default, only errors are logged. Each occurrence of this flag raises the log level: `-v`
    /// for warnings, `-vv` for info, `-vvv` for debug, and `-vvvv` for trace.
    #[structopt(short = "v", parse(from_occurrences))]
    verbosity: u8,
}

fn run(opt: Opt) -> Result<(), Box<dyn Error>> {
    let (input_tx, input_rx) = channel::unbounded::<Key>();
    let (notification_tx, notification_rx) = channel::unbounded::<Notification>();

    thread::spawn(move || -> io::Result<()> {
        let tty = termion::get_tty()?;

        for event in tty.events() {
            match event? {
                TermionEvent::Key(key) => input_tx.send(key),
                ev @ TermionEvent::Mouse(_) | ev @ TermionEvent::Unsupported(_) => {
                    warn!("unsupported event encountered: {:?}", ev);
                }
            };
        }

        Ok(())
    });

    let core = Core::spawn(opt.core, notification_tx)?;
    let editor = Editor::new(core, opt.file);

    editor.run(input_rx, notification_rx);

    // We hid the cursor earlier, so we have to restore it before we exit.
    print!("{}", cursor::Show);
    io::stdout().flush()?;

    Ok(())
}

fn init_logging(opt: &Opt) -> Result<(), Box<dyn Error>> {
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} {M} - {m}\n")))
        .build(&opt.log_file)?;

    let verbosity = match opt.verbosity {
        0 => LevelFilter::Error,
        1 => LevelFilter::Warn,
        2 => LevelFilter::Info,
        3 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .logger(Logger::builder().build(env!("CARGO_PKG_NAME").replace('-', "_"), verbosity))
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
