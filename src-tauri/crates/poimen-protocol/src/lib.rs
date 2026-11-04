//! Wire types of the `poimen-server` socket API, documented in `docs/protocol.md`.
//!
//! NDJSON over a Unix socket: one JSON object per line. A client sends [`Request`]s and reads one
//! [`Response`] per request; after `events.subscribe` the connection also carries [`Event`]s. The
//! crate has no I/O and no dependency on the app, so any client (the app, a CLI) can use it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version the server speaks and this crate is written for. Raised on every
/// incompatible change; `ping` reports it.
pub const PROTOCOL_VERSION: u32 = 1;

/// Environment variable that overrides the socket path (and is set in every terminal).
pub const ENV_SOCKET: &str = "POIMEN_SOCKET";
/// Environment variable set in every terminal: its own id.
pub const ENV_TERMINAL_ID: &str = "POIMEN_TERMINAL_ID";

/// The socket path: `$POIMEN_SOCKET`, else `$XDG_RUNTIME_DIR/poimen/server.sock`, else
/// `<temp dir>/poimen-<user>/server.sock`. One server per user listens there.
pub fn socket_path() -> PathBuf {
    socket_path_from(
        std::env::var_os(ENV_SOCKET),
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var("USER").ok(),
        std::env::temp_dir(),
    )
}

/// [`socket_path`] with the environment passed in; empty values count as unset.
pub fn socket_path_from(
    socket: Option<std::ffi::OsString>,
    runtime_dir: Option<std::ffi::OsString>,
    user: Option<String>,
    temp_dir: PathBuf,
) -> PathBuf {
    if let Some(socket) = socket.filter(|s| !s.is_empty()) {
        return PathBuf::from(socket);
    }
    if let Some(dir) = runtime_dir.filter(|s| !s.is_empty()) {
        return PathBuf::from(dir).join("poimen").join("server.sock");
    }
    let user = user
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "user".into());
    temp_dir.join(format!("poimen-{user}")).join("server.sock")
}

/// A request line. `id` is echoed in the response; any string the client picks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A response line: exactly one of `result` and `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// `None` when the request line could not be parsed far enough to know its id.
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn ok(id: &str, result: Value) -> Self {
        Self {
            id: Some(id.to_string()),
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<&str>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            id: id.map(str::to_string),
            result: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The line is not JSON.
    ParseError,
    /// JSON, but not a request (no `id` or `method`).
    InvalidRequest,
    UnknownMethod,
    /// The `params` do not fit the method.
    InvalidParams,
    /// A space or terminal id that does not exist.
    NotFound,
    /// The terminal exists but its process has exited.
    Exited,
    /// The shell or command could not be started.
    SpawnFailed,
    Internal,
}

/// A line sent by the server: a response, or an event on a subscribed connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerLine {
    Event(Event),
    Response(Response),
}

/// Parses one line of the server. A line with an `event` field is an [`Event`].
pub fn parse_server_line(line: &str) -> Result<ServerLine, serde_json::Error> {
    serde_json::from_str(line)
}

/// Parses one request line into a request, or the error response to send back.
pub fn parse_request(line: &str) -> Result<Request, Response> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| Response::err(None, ErrorCode::ParseError, e.to_string()))?;
    let id = value.get("id").and_then(Value::as_str).map(str::to_string);
    serde_json::from_value(value)
        .map_err(|e| Response::err(id.as_deref(), ErrorCode::InvalidRequest, e.to_string()))
}

/// Deserializes the `params` of a request; the error is the response to send back.
pub fn params<T: for<'de> Deserialize<'de>>(request: &Request) -> Result<T, Response> {
    let value = if request.params.is_null() {
        Value::Object(Default::default())
    } else {
        request.params.clone()
    };
    serde_json::from_value(value).map_err(|e| {
        Response::err(
            Some(&request.id),
            ErrorCode::InvalidParams,
            format!("{}: {e}", request.method),
        )
    })
}

// --- model -------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Running,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terminal {
    pub id: String,
    pub space_id: String,
    pub name: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub status: TerminalStatus,
    /// Set once `status` is `exited`; `None` when the process was killed by a signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

/// What an agent in a terminal is doing (reported by hooks, later PBIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
}

// --- request params and results ----------------------------------------------------------------

/// Result of `ping`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    pub version: String,
    pub protocol: u32,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceCreate {
    pub name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRename {
    pub space_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceClose {
    pub space_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalList {
    /// Only the terminals of this space.
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCreate {
    pub space_id: String,
    /// Defaults to the cwd of the space.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Program and arguments, never through a shell; defaults to the user's `$SHELL`.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRename {
    pub terminal_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalClose {
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInput {
    pub terminal_id: String,
    /// Written to the terminal as is (UTF-8).
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalResize {
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

// --- events ------------------------------------------------------------------------------------

/// An event line: `{"event": "<name>", "data": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum Event {
    #[serde(rename = "space.created")]
    SpaceCreated { space: Space },
    #[serde(rename = "space.renamed")]
    SpaceRenamed { space: Space },
    #[serde(rename = "space.closed")]
    SpaceClosed { space_id: String },
    #[serde(rename = "terminal.created")]
    TerminalCreated { terminal: Terminal },
    #[serde(rename = "terminal.renamed")]
    TerminalRenamed { terminal: Terminal },
    #[serde(rename = "terminal.closed")]
    TerminalClosed {
        terminal_id: String,
        space_id: String,
    },
    #[serde(rename = "terminal.exited")]
    TerminalExited {
        terminal_id: String,
        exit_code: Option<u32>,
    },
    #[serde(rename = "agent.status_changed")]
    AgentStatusChanged {
        terminal_id: String,
        status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_request_with_and_without_params() {
        let r =
            parse_request(r#"{"id":"1","method":"space.create","params":{"name":"a","cwd":"/x"}}"#)
                .unwrap();
        assert_eq!(r.method, "space.create");
        let p: SpaceCreate = params(&r).unwrap();
        assert_eq!(p.cwd, "/x");

        let r = parse_request(r#"{"id":"2","method":"ping"}"#).unwrap();
        assert_eq!(r.params, Value::Null);
    }

    #[test]
    fn rejects_garbage_with_parse_error_and_no_id() {
        let e = parse_request("not json").unwrap_err();
        assert_eq!(e.id, None);
        assert_eq!(e.error.unwrap().code, ErrorCode::ParseError);
    }

    #[test]
    fn rejects_a_request_without_method_but_keeps_its_id() {
        let e = parse_request(r#"{"id":"7"}"#).unwrap_err();
        assert_eq!(e.id.as_deref(), Some("7"));
        assert_eq!(e.error.unwrap().code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn wrong_params_are_invalid_params() {
        let r =
            parse_request(r#"{"id":"3","method":"terminal.resize","params":{"terminal_id":"t"}}"#)
                .unwrap();
        let e = params::<TerminalResize>(&r).unwrap_err();
        assert_eq!(e.id.as_deref(), Some("3"));
        assert_eq!(e.error.unwrap().code, ErrorCode::InvalidParams);
    }

    #[test]
    fn optional_params_default() {
        let r = parse_request(r#"{"id":"4","method":"terminal.list"}"#).unwrap();
        assert_eq!(params::<TerminalList>(&r).unwrap().space_id, None);
    }

    #[test]
    fn event_wire_shape() {
        let e = Event::TerminalExited {
            terminal_id: "t1".into(),
            exit_code: Some(3),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"event":"terminal.exited","data":{"terminal_id":"t1","exit_code":3}})
        );
        let status: Event = serde_json::from_value(
            json!({"event":"agent.status_changed","data":{"terminal_id":"t1","status":"blocked"}}),
        )
        .unwrap();
        assert_eq!(
            status,
            Event::AgentStatusChanged {
                terminal_id: "t1".into(),
                status: AgentStatus::Blocked,
                agent: None
            }
        );
    }

    #[test]
    fn server_lines_are_told_apart() {
        let line = r#"{"event":"space.closed","data":{"space_id":"s1"}}"#;
        assert!(matches!(
            parse_server_line(line).unwrap(),
            ServerLine::Event(Event::SpaceClosed { .. })
        ));
        let line = r#"{"id":"1","result":{"protocol":1}}"#;
        assert!(matches!(
            parse_server_line(line).unwrap(),
            ServerLine::Response(_)
        ));
        let line = r#"{"id":"1","error":{"code":"not_found","message":"x"}}"#;
        match parse_server_line(line).unwrap() {
            ServerLine::Response(r) => assert_eq!(r.error.unwrap().code, ErrorCode::NotFound),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn socket_path_precedence() {
        let tmp = PathBuf::from("/tmp");
        assert_eq!(
            socket_path_from(
                Some("/a.sock".into()),
                Some("/run/user/1".into()),
                None,
                tmp.clone()
            ),
            PathBuf::from("/a.sock")
        );
        assert_eq!(
            socket_path_from(
                Some("".into()),
                Some("/run/user/1".into()),
                None,
                tmp.clone()
            ),
            PathBuf::from("/run/user/1/poimen/server.sock")
        );
        assert_eq!(
            socket_path_from(None, None, Some("jk".into()), tmp),
            PathBuf::from("/tmp/poimen-jk/server.sock")
        );
    }
}
