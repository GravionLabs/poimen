//! Wire types of the `poimen-server` socket API, documented in `docs/protocol.md`.
//!
//! NDJSON over a local socket (a Unix socket, a named pipe on Windows): one JSON object per line. A client sends [`Request`]s and reads one
//! [`Response`] per request; after `events.subscribe` the connection also carries [`Event`]s. The
//! crate has no I/O and no dependency on the app, so any client (the app, a CLI) can use it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version the server speaks and this crate is written for. Raised on every
/// incompatible change; `ping` reports it.
pub const PROTOCOL_VERSION: u32 = 3;

/// Environment variable that overrides the socket path (and is set in every terminal).
pub const ENV_SOCKET: &str = "POIMEN_SOCKET";
/// Environment variable set in every terminal: its own id.
pub const ENV_TERMINAL_ID: &str = "POIMEN_TERMINAL_ID";
/// Environment variable of `poimen-server integration install`: the absolute path the hooks
/// call instead of the running binary. The app sets it when it runs a copy whose path is not
/// stable (inside an AppImage), pointing at the stable link of its staged copy.
pub const ENV_HOOK_EXE: &str = "POIMEN_HOOK_EXE";

/// The directory under `%LOCALAPPDATA%` that holds the socket marker, `server.lock`,
/// `server.json` and the started server copies on Windows (not the install directory).
pub const WINDOWS_DIR: &str = "poimen-server";

/// The socket path. Unix: `$POIMEN_SOCKET`, else `$XDG_RUNTIME_DIR/poimen/server.sock`, else
/// `<temp dir>/poimen-<user>/server.sock`. Windows: `$POIMEN_SOCKET`, else
/// `%LOCALAPPDATA%\poimen-server\server.sock` (a marker file; the server listens on the named
/// pipe of that path), else `<temp dir>\poimen-<user>\server.sock`. One server per user.
pub fn socket_path() -> PathBuf {
    if cfg!(windows) {
        windows_socket_path_from(
            std::env::var_os(ENV_SOCKET),
            std::env::var_os("LOCALAPPDATA"),
            std::env::var("USERNAME").ok(),
            std::env::temp_dir(),
        )
    } else {
        socket_path_from(
            std::env::var_os(ENV_SOCKET),
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var("USER").ok(),
            std::env::temp_dir(),
        )
    }
}

/// The Windows [`socket_path`] with the environment passed in; empty values count as unset.
pub fn windows_socket_path_from(
    socket: Option<std::ffi::OsString>,
    local_app_data: Option<std::ffi::OsString>,
    user: Option<String>,
    temp_dir: PathBuf,
) -> PathBuf {
    if let Some(socket) = socket.filter(|s| !s.is_empty()) {
        return PathBuf::from(socket);
    }
    if let Some(dir) = windows_dir_from(local_app_data) {
        return dir.join("server.sock");
    }
    temp_dir.join(user_dir_name(user)).join("server.sock")
}

/// `%LOCALAPPDATA%\poimen-server` when `LOCALAPPDATA` is set and not empty.
pub fn windows_dir_from(local_app_data: Option<std::ffi::OsString>) -> Option<PathBuf> {
    local_app_data
        .filter(|s| !s.is_empty())
        .map(|dir| PathBuf::from(dir).join(WINDOWS_DIR))
}

fn user_dir_name(user: Option<String>) -> String {
    let user = user
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "user".into());
    format!("poimen-{user}")
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
    temp_dir.join(user_dir_name(user)).join("server.sock")
}

/// The effective user id of this process, read as the owner of a file created now (std has no
/// safe uid getter and this crate forbids `unsafe`).
#[cfg(unix)]
pub fn current_uid() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    // Threads of one process may probe within the same clock tick (macOS: microseconds).
    static PROBE: AtomicU64 = AtomicU64::new(0);
    let probe = std::env::temp_dir().join(format!(
        "poimen-uid-{}-{}-{}",
        std::process::id(),
        PROBE.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let uid = file.metadata().map(|m| m.uid());
    drop(file);
    let _ = std::fs::remove_file(&probe);
    uid
}

/// Checks that `dir` is a real directory (not a symlink) owned by the current user with no
/// group/other permission bits. With `fix`, a directory we own but with loose bits is chmod 0700;
/// a symlink, a foreign owner or loose bits without `fix` give `PermissionDenied`. A missing
/// directory is `NotFound`.
#[cfg(unix)]
pub fn check_private_dir(dir: &std::path::Path, fix: bool) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let deny = |why: &str| {
        Error::new(
            ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        )
    };
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(deny("is not a real directory"));
    }
    if meta.uid() != current_uid()? {
        return Err(deny("is owned by another user"));
    }
    if meta.mode() & 0o077 != 0 {
        if !fix {
            return Err(deny("is accessible by group or others"));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
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
    /// The request needs the controller of the terminal and this connection is not it (an
    /// observer's input or resize, a resize while another connection controls).
    NotController,
    /// The terminal already has a controller and the attach did not ask to take over.
    Conflict,
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
    /// The program the terminal runs (the shell, for a default terminal); lets a client quote
    /// what it types for that shell. Absent from servers that do not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The agent in the terminal, as its hooks last reported it; absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// The free-form tokens tools in the terminal reported (`terminal.report_tokens`); expired
    /// ones are gone. Empty when there are none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
}

/// What an agent in a terminal is doing (reported by hooks). `Gone` is only ever reported: the
/// agent ended, the terminal then has no [`AgentInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Gone,
}

/// The agent of a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub status: AgentStatus,
    /// Which agent, e.g. `claude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The latest notification text of the agent (see [`clean_message`]); cleared when the agent
    /// turns `working` again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// When `message` was accepted, milliseconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_at: Option<u64>,
}

/// Longest notification text, in characters.
pub const MAX_MESSAGE_CHARS: usize = 300;

/// A notification text as the protocol carries it: control characters (newlines, tabs included)
/// become a space, runs of whitespace collapse, the result is trimmed and cut to
/// [`MAX_MESSAGE_CHARS`] characters. `None` when nothing is left.
pub fn clean_message(text: &str) -> Option<String> {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_MESSAGE_CHARS).collect();
    let cut = cut.trim_end().to_owned();
    (!cut.is_empty()).then_some(cut)
}

/// Params of `server.set_colors`: the colors the server answers terminal color queries with
/// while no controller is attached. `#rrggbb` strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetColors {
    pub foreground: String,
    pub background: String,
    /// Defaults to the foreground.
    #[serde(default)]
    pub cursor: Option<String>,
    /// The 16 ANSI colors; when absent the previous ones stay.
    #[serde(default)]
    pub palette: Option<Vec<String>>,
}

/// Params of `terminal.report_agent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReportAgent {
    pub terminal_id: String,
    /// Increases with every report of a reporter (e.g. nanoseconds since the epoch); a report
    /// whose `seq` is not above the last accepted one is dropped.
    pub seq: u64,
    pub status: AgentStatus,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Notification text of the agent; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Most tokens in one `terminal.report_tokens` (as herdr).
pub const MAX_TOKENS_PER_REPORT: usize = 16;
/// Longest token name (as herdr).
pub const MAX_TOKEN_NAME: usize = 32;

/// A token name is 1 to [`MAX_TOKEN_NAME`] ASCII letters, digits, `_` or `-` (as herdr).
pub fn valid_token_name(name: &str) -> bool {
    (1..=MAX_TOKEN_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Params of `terminal.report_tokens`: sets the tokens of `source` on the terminal; a `null`
/// value removes the token. `ttl_s` (above 0) expires the tokens set by this report after that
/// many seconds. A token is owned by the source that set it last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReportTokens {
    pub terminal_id: String,
    pub tokens: BTreeMap<String, Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_s: Option<u64>,
    pub source: String,
}

/// Params of `terminal.clear_tokens`: removes every token `source` set on the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalClearTokens {
    pub terminal_id: String,
    pub source: String,
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

/// `terminal.detach`: ends the attachment of this connection to `terminal_id`; the server
/// answers `{"detached": true}` and then closes the connection. For clients that cannot shut
/// a connection down themselves (named pipes on Windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDetach {
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

/// How a connection attaches to a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachMode {
    /// Receives the output and size changes; cannot send input or resize.
    Observe,
    /// The one connection that sends input and sets the size.
    Control,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalAttach {
    pub terminal_id: String,
    pub mode: AttachMode,
    /// With `control`: replace the current controller (which gets `terminal.detached` with
    /// reason `taken_over`) instead of failing with `conflict`.
    #[serde(default)]
    pub take_over: bool,
}

/// Result of `terminal.attach`. The records of the stream have a `seq` greater than `seq` here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachResult {
    pub terminal_id: String,
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
    pub mode: AttachMode,
    /// Base64 of the bytes that bring a fresh emulator of `cols` x `rows` to the current state
    /// (screen, scrollback, modes, cursor, title). Write it to the emulator as is.
    pub snapshot: String,
}

/// Why the server ended an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetachReason {
    /// The client did not keep up; attach again for a fresh snapshot.
    Overflow,
    /// Another connection took over the control.
    TakenOver,
    /// The terminal was closed.
    Closed,
}

/// Encodes bytes for the wire (standard base64).
pub fn encode_bytes(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Decodes [`encode_bytes`].
pub fn decode_bytes(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(data)
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
    /// Stream record of an attached connection: raw PTY bytes (base64).
    #[serde(rename = "terminal.output")]
    TerminalOutput {
        terminal_id: String,
        seq: u64,
        data: String,
    },
    /// Stream record of an attached connection: the controller changed the size.
    #[serde(rename = "terminal.resized")]
    TerminalResized {
        terminal_id: String,
        seq: u64,
        cols: u16,
        rows: u16,
    },
    /// Stream record: the server ended this attachment (the process did not exit).
    #[serde(rename = "terminal.detached")]
    TerminalDetached {
        terminal_id: String,
        reason: DetachReason,
    },
    /// A controller attached (subscribers).
    #[serde(rename = "terminal.controller_attached")]
    TerminalControllerAttached { terminal_id: String },
    /// The controller detached or was dropped (subscribers).
    #[serde(rename = "terminal.controller_detached")]
    TerminalControllerDetached { terminal_id: String },
    /// The tokens of a terminal changed (reported, cleared or expired); carries all of them.
    #[serde(rename = "terminal.tokens_changed")]
    TerminalTokensChanged {
        terminal_id: String,
        tokens: BTreeMap<String, String>,
    },
    #[serde(rename = "agent.status_changed")]
    AgentStatusChanged {
        terminal_id: String,
        status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_at: Option<u64>,
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
                agent: None,
                session_id: None,
                message: None,
                message_at: None
            }
        );
    }

    #[test]
    fn clean_message_strips_controls_collapses_and_truncates() {
        assert_eq!(
            clean_message("  Allow\u{1b}[31m Bash?\n\tnow\u{0} ").as_deref(),
            Some("Allow [31m Bash? now")
        );
        assert_eq!(clean_message("\n\t \u{7}"), None);
        assert_eq!(clean_message(""), None);
        let long = "ab ".repeat(200);
        let cut = clean_message(&long).unwrap();
        assert_eq!(cut.chars().count(), 299);
        assert!(cut.ends_with('b'));
        assert_eq!(
            clean_message(&"é".repeat(400)).unwrap().chars().count(),
            300
        );
    }

    #[test]
    fn set_colors_params_have_optional_cursor_and_palette() {
        let p: SetColors =
            serde_json::from_value(json!({"foreground":"#ffffff","background":"#000000"})).unwrap();
        assert_eq!((p.cursor, p.palette), (None, None));
    }

    #[test]
    fn report_agent_params_and_the_terminal_agent_wire_shape() {
        let p: TerminalReportAgent =
            serde_json::from_value(json!({"terminal_id":"t1","seq":5,"status":"gone"})).unwrap();
        assert_eq!(p.status, AgentStatus::Gone);
        assert_eq!((p.agent, p.session_id, p.message), (None, None, None));
        let p: TerminalReportAgent = serde_json::from_value(
            json!({"terminal_id":"t1","seq":6,"status":"blocked","message":"Allow Bash?"}),
        )
        .unwrap();
        assert_eq!(p.message.as_deref(), Some("Allow Bash?"));
        let t: Terminal = serde_json::from_value(json!({"id":"t1","space_id":"s1","name":"n",
            "cwd":"/","cols":80,"rows":24,"status":"running",
            "agent":{"status":"working","agent":"claude","session_id":"abc"}}))
        .unwrap();
        assert_eq!(t.agent.unwrap().session_id.as_deref(), Some("abc"));
    }

    #[test]
    fn token_wire_shapes_and_names() {
        let p: TerminalReportTokens = serde_json::from_value(json!({"terminal_id":"t1",
            "tokens":{"deliver_step":"build","deliver_pr":null},"ttl_s":30,"source":"deliver"}))
        .unwrap();
        assert_eq!(p.tokens["deliver_step"].as_deref(), Some("build"));
        assert_eq!(p.tokens["deliver_pr"], None);
        assert_eq!(p.ttl_s, Some(30));
        let p: TerminalReportTokens =
            serde_json::from_value(json!({"terminal_id":"t1","tokens":{},"source":"s"})).unwrap();
        assert_eq!(p.ttl_s, None);
        assert!(
            serde_json::from_value::<TerminalReportTokens>(
                json!({"terminal_id":"t1","tokens":{"a":1},"source":"s"})
            )
            .is_err()
        );
        let c: TerminalClearTokens =
            serde_json::from_value(json!({"terminal_id":"t1","source":"deliver"})).unwrap();
        assert_eq!(c.source, "deliver");
        let t: Terminal = serde_json::from_value(json!({"id":"t1","space_id":"s1","name":"n",
            "cwd":"/","cols":80,"rows":24,"status":"running","tokens":{"deliver":"on"}}))
        .unwrap();
        assert_eq!(t.tokens["deliver"], "on");
        let event: Event = serde_json::from_str(
            r#"{"event":"terminal.tokens_changed","data":{"terminal_id":"t1","tokens":{"a":"b"}}}"#,
        )
        .unwrap();
        assert_eq!(
            event,
            Event::TerminalTokensChanged {
                terminal_id: "t1".into(),
                tokens: [("a".to_owned(), "b".to_owned())].into()
            }
        );
        assert!(valid_token_name("deliver_step-2"));
        assert!(valid_token_name(&"a".repeat(32)));
        assert!(!valid_token_name(""));
        assert!(!valid_token_name(&"a".repeat(33)));
        assert!(!valid_token_name("a b"));
        assert!(!valid_token_name("é"));
        assert!(!valid_token_name("a.b"));
    }

    #[test]
    fn attach_wire_shapes() {
        let p: TerminalAttach =
            serde_json::from_value(json!({"terminal_id":"t1","mode":"control","take_over":true}))
                .unwrap();
        assert_eq!(p.mode, AttachMode::Control);
        assert!(p.take_over);
        let p: TerminalAttach =
            serde_json::from_value(json!({"terminal_id":"t1","mode":"observe"})).unwrap();
        assert!(!p.take_over);
        assert!(
            serde_json::from_value::<TerminalAttach>(json!({"terminal_id":"t1","mode":"x"}))
                .is_err()
        );
        let d: TerminalDetach = serde_json::from_value(json!({"terminal_id":"t1"})).unwrap();
        assert_eq!(d.terminal_id, "t1");
        assert!(serde_json::from_value::<TerminalDetach>(json!({})).is_err());

        let e = Event::TerminalDetached {
            terminal_id: "t1".into(),
            reason: DetachReason::TakenOver,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"event":"terminal.detached","data":{"terminal_id":"t1","reason":"taken_over"}})
        );
        let line =
            r#"{"event":"terminal.output","data":{"terminal_id":"t1","seq":3,"data":"aGk="}}"#;
        match parse_server_line(line).unwrap() {
            ServerLine::Event(Event::TerminalOutput { seq, data, .. }) => {
                assert_eq!((seq, decode_bytes(&data).unwrap()), (3, b"hi".to_vec()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bytes_round_trip_including_non_utf8() {
        let raw = [0u8, 0xff, 0x1b, b'[', 0xc3];
        assert_eq!(decode_bytes(&encode_bytes(&raw)).unwrap(), raw);
        assert!(decode_bytes("***").is_err());
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

    #[test]
    fn windows_socket_path_precedence() {
        let tmp = PathBuf::from(r"C:\Temp");
        let local = PathBuf::from(r"C:\Users\u\AppData\Local");
        assert_eq!(
            windows_socket_path_from(
                Some(r"D:\x.sock".into()),
                Some(local.clone().into()),
                None,
                tmp.clone()
            ),
            PathBuf::from(r"D:\x.sock")
        );
        assert_eq!(
            windows_socket_path_from(
                Some("".into()),
                Some(local.clone().into()),
                None,
                tmp.clone()
            ),
            local.join("poimen-server").join("server.sock")
        );
        assert_eq!(
            windows_socket_path_from(None, Some("".into()), Some("jk".into()), tmp.clone()),
            tmp.join("poimen-jk").join("server.sock")
        );
        assert_eq!(
            windows_socket_path_from(None, None, None, tmp.clone()),
            tmp.join("poimen-user").join("server.sock")
        );
    }

    #[test]
    fn the_windows_dir_needs_local_app_data() {
        assert_eq!(
            windows_dir_from(Some(r"C:\L".into())),
            Some(PathBuf::from(r"C:\L").join("poimen-server"))
        );
        assert_eq!(windows_dir_from(Some("".into())), None);
        assert_eq!(windows_dir_from(None), None);
    }
}

#[cfg(all(test, unix))]
mod private_dir_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fresh(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-pd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode(p: &std::path::Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_0700_dir_of_ours_passes() {
        let dir = fresh("ok");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        check_private_dir(&dir, false).unwrap();
    }

    #[test]
    fn a_0777_dir_is_fixed_with_fix_and_refused_without() {
        let dir = fresh("loose");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = check_private_dir(&dir, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(mode(&dir), 0o777);
        check_private_dir(&dir, true).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn a_symlink_to_a_dir_is_refused_even_with_fix() {
        let target = fresh("target");
        let link = std::env::temp_dir().join(format!("poimen-pd-{}-link", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink(&target, &link).unwrap();
        let err = check_private_dir(&link, true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&link).unwrap();
    }

    #[test]
    fn a_dir_of_another_user_is_refused() {
        // `/` is owned by root; skipped when the tests run as root.
        if current_uid().unwrap() == 0 {
            return;
        }
        let err = check_private_dir(std::path::Path::new("/"), true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_missing_dir_is_not_found() {
        let err = check_private_dir(&fresh("m").join("nope"), false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
