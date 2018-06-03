//! Core types used by the [xi-core protocol].
//!
//! The protocol used by the core is very similar to [JSON-RPC]. It is tempting to write these
//! types so that they use [`serde_json::Value`] internally, and then later deserialize the values
//! into the types they represent, but this incurs a significant overhead. Instead, these types are
//! written as close to the JSON representation as possible. This makes matching on the
//! [`Message`], more awkward, but it ensures that there is only a single deserialization step.
//!
//! [xi-core protocol]: https://google.github.io/xi-editor/docs/frontend-protocol.html
//! [JSON-RPC]: http://www.jsonrpc.org/specification

use std::path::PathBuf;

use core::CoreError;

use serde::Deserialize;
use serde_derive::{Deserialize, Serialize};
use serde_json::{self, Value};

mod types;

pub use self::types::*;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Message {
    Request {
        id: u64,

        #[serde(flatten)]
        req: Request,
    },
    Response {
        id: u64,

        #[serde(flatten)]
        res: Response,
    },
    Notification(Notification),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Request {
    NewView {
        #[serde(skip_serializing_if = "Option::is_none")]
        file_path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

impl Response {
    pub fn into_result<T>(self) -> Result<T, CoreError>
    where
        for<'de> T: Deserialize<'de>,
    {
        match (self.result, self.error) {
            (Some(result), None) => {
                Ok(serde_json::from_value(result).map_err(CoreError::Protocol)?)
            }
            (None, Some(error)) => Err(CoreError::BadResponse(error)),
            _ => {
                // FIXME: This should be an error in deserialization
                panic!("expected exactly one of `result` or `error`");
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    // Frontend -> Backend
    ClientStarted {
        #[serde(skip_serializing_if = "Option::is_none")]
        config_dir: Option<PathBuf>,

        #[serde(skip_serializing_if = "Option::is_none")]
        client_extras_dir: Option<PathBuf>,
    },

    // Frontend -> Backend
    Edit {
        #[serde(flatten)]
        method: EditMethod,
        view_id: ViewId,
    },

    // Backend -> Frontend
    AvailableThemes {
        themes: Vec<String>,
    },

    // Backend -> Frontend
    AvailablePlugins {
        view_id: ViewId,
        plugins: Vec<Plugin>,
    },

    // Backend -> Frontend
    ConfigChanged {
        view_id: ViewId,
        changes: ConfigChanges,
    },

    // Backend -> Frontend
    Update {
        view_id: ViewId,
        update: Update,
    },

    // Backend -> Frontend
    ScrollTo {
        view_id: ViewId,
        line: u64,
        col: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "method", content = "params")]
pub enum EditMethod {
    Scroll(u16, u16),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{self, json, json_internal};

    use super::{EditMethod, Message, Notification, Request, Response, ViewId};

    #[test]
    fn client_started() {
        let notification = Notification::ClientStarted {
            config_dir: None,
            client_extras_dir: None,
        };
        let json = json!({ "method": "client_started", "params": {} });

        let actual: Notification = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(notification, actual);
        let actual: Message = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(Message::Notification(notification.clone()), actual);

        let actual = serde_json::to_value(notification).unwrap();
        assert_eq!(json, actual);
    }

    #[test]
    fn new_view() {
        let req = Message::Request {
            id: 0,
            req: Request::NewView {
                file_path: Some(PathBuf::from("/test/path")),
            },
        };
        let json = json!({ "id": 0, "method": "new_view", "params": { "file_path": "/test/path" }});

        let actual: Message = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(req, actual);

        let actual = serde_json::to_value(req).unwrap();
        assert_eq!(json, actual);

        let res = Message::Response {
            id: 0,
            res: Response {
                result: Some(json!("view-id-1")),
                error: None,
            },
        };
        let json = json!({ "id": 0, "result": "view-id-1" });

        let actual: Message = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(res, actual);

        let actual = serde_json::to_value(res).unwrap();
        assert_eq!(json, actual);
    }

    #[test]
    fn scroll() {
        let not = Notification::Edit {
            method: EditMethod::Scroll(0, 18),
            view_id: ViewId(String::from("view-id-4")),
        };

        let json = json!({
            "method": "edit",
            "params": {
                "method": "scroll",
                "params": vec![0, 18],
                "view_id": "view-id-4",
            },
        });

        assert_eq!(serde_json::to_value(not).unwrap(), json);
    }
}
