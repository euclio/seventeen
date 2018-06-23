use std::collections::HashMap;
use std::io::{self, prelude::*, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use failure::Fail;
use futures::{self, Complete, Future};
use log::*;
use serde_json::{self, Value};

use protocol::*;
use Event;

#[derive(Debug, Fail)]
pub enum CoreError {
    #[fail(display = "i/o error: {}", _0)]
    Io(#[cause] io::Error),

    #[fail(display = "unexpected protocol format: {}", _0)]
    Protocol(#[cause] serde_json::Error),

    #[fail(display = "core returned an error value: {}", _0)]
    BadResponse(Value),
}

#[derive(Debug)]
pub struct Core {
    stdin: Option<ChildStdin>,
    process: Child,
    request_map: Arc<Mutex<HashMap<u64, Complete<Response>>>>,
    next_id: u64,
}

impl Core {
    pub fn spawn(path: impl AsRef<Path>, event_tx: Sender<Event>) -> io::Result<Self> {
        info!("spawning core");

        let mut core = Command::new(path.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let request_map = Arc::new(Mutex::new(HashMap::<u64, Complete<Response>>::new()));

        let stdout = core.stdout.take().unwrap();
        let response_map = request_map.clone();
        thread::spawn(move || -> io::Result<()> {
            let stdout = BufReader::new(stdout);
            for line in stdout.lines() {
                let line = line?;
                trace!("<- {}", line);
                let message = match serde_json::from_str(&line) {
                    Ok(message) => message,
                    Err(err) => {
                        error!("could not deserialize message from core, skipping: {}", err);
                        continue;
                    },
                };
                match message {
                    Message::Notification(not) => {
                        event_tx.send(Event::CoreNotification(not)).unwrap()
                    }
                    Message::Request { id, req } => {
                        error!(
                            "xi-core is not known to send requests, but got request ({:?}, {:?})",
                            id, req
                        );
                    }
                    Message::Response { id, res } => {
                        let completer = response_map
                            .lock()
                            .unwrap()
                            .remove(&id)
                            .expect("got response without a request");
                        completer.send(res).unwrap();
                    }
                }
            }

            Ok(())
        });

        let stderr = core.stderr.take().unwrap();
        thread::spawn(move || -> io::Result<()> {
            let stderr = BufReader::new(stderr);
            for line in stderr.lines() {
                info!("xi-core: {}", line?);
            }

            Ok(())
        });

        Ok(Self {
            stdin: core.stdin.take(),
            process: core,
            request_map,
            next_id: 0,
        })
    }

    pub fn client_started<P: Into<PathBuf>>(&mut self, config_dir: Option<P>) -> io::Result<()> {
        self.notify(&Notification::ClientStarted {
            config_dir: config_dir.map(Into::into),
            client_extras_dir: None,
        })
    }

    pub fn scroll(&mut self, view_id: ViewId, (first, last): (u16, u16)) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::Scroll(first, last),
            view_id,
        })
    }

    pub fn insert(&mut self, view_id: ViewId, chars: String) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::Insert { chars },
            view_id,
        })
    }

    pub fn delete_backward(&mut self, view_id: ViewId) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::DeleteBackward,
            view_id,
        })
    }

    pub fn move_right(&mut self, view_id: ViewId) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::MoveRight,
            view_id,
        })
    }

    pub fn move_left(&mut self, view_id: ViewId) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::MoveLeft,
            view_id,
        })
    }

    pub fn move_up(&mut self, view_id: ViewId) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::MoveUp,
            view_id,
        })
    }

    pub fn move_down(&mut self, view_id: ViewId) -> io::Result<()> {
        self.notify(&Notification::Edit {
            method: EditMethod::MoveDown,
            view_id,
        })
    }

    pub fn set_theme(&mut self, theme: &str) -> io::Result<()> {
        self.notify(&Notification::SetTheme {
            theme_name: String::from(theme),
        })
    }

    pub fn new_view<P: Into<PathBuf>>(
        &mut self,
        file_path: Option<P>,
    ) -> impl Future<Item = ViewId, Error = CoreError> {
        self.request(Request::NewView {
            file_path: file_path.map(Into::into),
        }).and_then(|res: Response| res.into_result())
    }

    fn notify(&mut self, notification: &Notification) -> io::Result<()> {
        let json = serde_json::to_string(&notification).unwrap();
        trace!("-> {}", json);
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json)
    }

    fn request(&mut self, req: Request) -> impl Future<Item = Response, Error = CoreError> {
        let (c, p) = futures::oneshot::<Response>();

        let id = self.next_id;

        {
            let mut map = self.request_map.lock().unwrap();
            debug!("creating request with id: {}", id);
            let existing_req = map.insert(id, c);
            assert!(existing_req.is_none(), "existing request for id {}", id);
            while map.contains_key(&self.next_id) {
                self.next_id = id.wrapping_add(1);
            }
            debug!("next_id: {}", self.next_id);
        }

        // FIXME: Potential I/O error here
        self.send_to_core(&Message::Request { id, req }).unwrap();

        p.map_err(|e| panic!("{}", e))
    }

    fn send_to_core(&mut self, message: &Message) -> io::Result<()> {
        let json = serde_json::to_string(message).unwrap();
        trace!("-> {}", json);
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // xi-core closes gracefully when its stdin is closed.
        self.stdin.take().unwrap();

        match self.process.wait() {
            Ok(exit_status) => info!("core exited with {}", exit_status),
            Err(e) => error!("core exited unexpectedly: {}", e),
        }
    }
}
