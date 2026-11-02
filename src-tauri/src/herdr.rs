//! Minimal client for the herdr socket API (newline-delimited JSON over a Unix socket).
//!
//! Requests open a short-lived connection; subscriptions keep theirs open.
//! See <https://herdr.dev> (docs/socket-api) for the protocol.

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(String),
    Api { code: String, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "herdr socket: {err}"),
            Error::Protocol(msg) => write!(f, "herdr protocol: {msg}"),
            Error::Api { code, message } => write!(f, "herdr {code}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

/// Resolves the socket path like the herdr CLI does (minus the explicit `--session` flag).
pub fn socket_path_from(env: impl Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(path) = env("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let base = PathBuf::from(env("HOME").unwrap_or_default()).join(".config/herdr");
    match env("HERDR_SESSION").filter(|s| !s.is_empty() && s != "default") {
        Some(session) => base.join("sessions").join(session).join("herdr.sock"),
        None => base.join("herdr.sock"),
    }
}

pub fn request_line(id: &str, method: &str, params: &Value) -> String {
    let mut line = json!({ "id": id, "method": method, "params": params }).to_string();
    line.push('\n');
    line
}

fn api_error(error: &Value) -> Error {
    Error::Api {
        code: error["code"].as_str().unwrap_or("unknown").to_owned(),
        message: error["message"].as_str().unwrap_or_default().to_owned(),
    }
}

/// Parses one response line: `{"id", "result"}` or `{"id", "error": {"code", "message"}}`.
pub fn parse_response(line: &str) -> Result<Value, Error> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Protocol(format!("invalid json: {err}")))?;
    if let Some(error) = value.get("error") {
        return Err(api_error(error));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| Error::Protocol("response without result".into()))
}

/// A pushed event: `{"event": "<kind>", "data": {...}}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Event {
    pub kind: String,
    pub data: Value,
}

pub fn parse_event(line: &str) -> Result<Event, Error> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Protocol(format!("invalid json: {err}")))?;
    if let Some(error) = value.get("error") {
        // e.g. events_lost: the server closes the subscription after this line.
        return Err(api_error(error));
    }
    let kind = value["event"]
        .as_str()
        .ok_or_else(|| Error::Protocol("event without kind".into()))?
        .to_owned();
    Ok(Event {
        kind,
        data: value["data"].clone(),
    })
}

#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
}

/// The herdr protocol version poimen is written against.
pub const SUPPORTED_PROTOCOL: u32 = 22;

/// Version and protocol of the running herdr, from the `ping` reply.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ServerInfo {
    pub version: String,
    /// `None` when the server reports a version but no protocol: shown, but not checked.
    pub protocol: Option<u32>,
    /// The protocol poimen supports, so the frontend can flag a mismatch.
    pub supported: u32,
}

/// Reads `{"type":"pong","version":"0.9.5","protocol":22}`; `None` when the server reports no version.
pub fn parse_ping(result: &Value) -> Option<ServerInfo> {
    Some(ServerInfo {
        version: result.get("version")?.as_str()?.to_string(),
        protocol: result
            .get("protocol")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok()),
        supported: SUPPORTED_PROTOCOL,
    })
}

/// Where `pane.split` puts the new pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    fn as_str(self) -> &'static str {
        match self {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        }
    }
}

fn non_empty(cwd: Option<&str>) -> Option<&str> {
    cwd.map(str::trim).filter(|c| !c.is_empty())
}

/// Params of `pane.split`: splits `target`, optionally starting the new shell in `cwd`.
pub fn split_params(target: &str, direction: SplitDirection, cwd: Option<&str>) -> Value {
    let mut params = json!({ "target_pane_id": target, "direction": direction.as_str() });
    if let Some(cwd) = non_empty(cwd) {
        params["cwd"] = json!(cwd);
    }
    params
}

/// Params of `pane.close`.
pub fn close_params(pane_id: &str) -> Value {
    json!({ "pane_id": pane_id })
}

/// Params of `tab.create`: a new tab (with one terminal pane) in a workspace, optionally in `cwd`.
pub fn tab_create_params(workspace_id: &str, cwd: Option<&str>) -> Value {
    let mut params = json!({ "workspace_id": workspace_id });
    if let Some(cwd) = non_empty(cwd) {
        params["cwd"] = json!(cwd);
    }
    params
}

/// The pane a `pane.split` (`pane_info`) or `tab.create` (`tab_created`) result created.
pub fn created_pane_id(result: &Value) -> Option<String> {
    let pane = result.get("pane").or_else(|| result.get("root_pane"))?;
    Some(pane.get("pane_id")?.as_str()?.to_owned())
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("poimen_{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

impl Client {
    pub fn from_env() -> Self {
        Self {
            socket: socket_path_from(|key| std::env::var(key).ok()),
        }
    }

    fn connect(&self, method: &str, params: &Value) -> Result<BufReader<UnixStream>, Error> {
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.write_all(request_line(&next_id(), method, params).as_bytes())?;
        Ok(BufReader::new(stream))
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let mut reader = self.connect(method, &params)?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::Protocol("connection closed before response".into()));
        }
        parse_response(&line)
    }

    /// The running herdr's version, or `None` when it is unreachable or too old to say.
    pub fn info(&self) -> Option<ServerInfo> {
        parse_ping(&self.request("ping", json!({})).ok()?)
    }

    pub fn subscribe(&self, subscriptions: Vec<Value>) -> Result<Subscription, Error> {
        let mut reader = self.connect(
            "events.subscribe",
            &json!({ "subscriptions": subscriptions }),
        )?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::Protocol(
                "connection closed before subscription ack".into(),
            ));
        }
        parse_response(&line)?;
        Ok(Subscription { reader })
    }
}

pub struct Subscription {
    reader: BufReader<UnixStream>,
}

impl Subscription {
    /// Blocks until the next event. Errors when the server closes or reports `events_lost`.
    pub fn next_event(&mut self) -> Result<Event, Error> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(Error::Protocol("subscription closed".into()));
            }
            if !line.trim().is_empty() {
                return parse_event(&line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn socket_path_defaults_to_config_dir() {
        let path = socket_path_from(env(&[("HOME", "/home/u")]));
        assert_eq!(path, PathBuf::from("/home/u/.config/herdr/herdr.sock"));
    }

    #[test]
    fn socket_path_uses_named_session() {
        let path = socket_path_from(env(&[("HOME", "/home/u"), ("HERDR_SESSION", "work")]));
        assert_eq!(
            path,
            PathBuf::from("/home/u/.config/herdr/sessions/work/herdr.sock")
        );
    }

    #[test]
    fn socket_path_override_wins() {
        let path = socket_path_from(env(&[
            ("HOME", "/home/u"),
            ("HERDR_SESSION", "work"),
            ("HERDR_SOCKET_PATH", "/tmp/x.sock"),
        ]));
        assert_eq!(path, PathBuf::from("/tmp/x.sock"));
    }

    #[test]
    fn request_line_is_one_json_line() {
        let line = request_line("r1", "pane.list", &json!({}));
        assert!(line.ends_with('\n'));
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(
            value,
            json!({ "id": "r1", "method": "pane.list", "params": {} })
        );
    }

    #[test]
    fn parse_response_returns_result() {
        let result = parse_response(r#"{"id":"r1","result":{"type":"pong"}}"#).unwrap();
        assert_eq!(result, json!({ "type": "pong" }));
    }

    #[test]
    fn parse_response_maps_api_error() {
        let err = parse_response(r#"{"id":"r1","error":{"code":"not_found","message":"no pane"}}"#)
            .unwrap_err();
        assert!(matches!(err, Error::Api { ref code, .. } if code == "not_found"));
    }

    #[test]
    fn parse_event_reads_envelope() {
        let event = parse_event(
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:p1","agent_status":"blocked"}}"#,
        )
        .unwrap();
        assert_eq!(event.kind, "pane.agent_status_changed");
        assert_eq!(event.data["agent_status"], "blocked");
    }

    #[test]
    fn parse_event_surfaces_events_lost() {
        let err = parse_event(r#"{"id":"sub","error":{"code":"events_lost","message":"behind"}}"#)
            .unwrap_err();
        assert!(matches!(err, Error::Api { ref code, .. } if code == "events_lost"));
    }

    #[test]
    fn parse_ping_reads_version_and_protocol() {
        let pong = json!({"type":"pong","version":"0.9.5","protocol":22,"capabilities":{}});
        assert_eq!(
            parse_ping(&pong),
            Some(ServerInfo {
                version: "0.9.5".into(),
                protocol: Some(22),
                supported: SUPPORTED_PROTOCOL
            })
        );
    }

    #[test]
    fn split_params_name_target_direction_and_cwd() {
        assert_eq!(
            split_params("w1:p1", SplitDirection::Right, Some("/tmp/x")),
            json!({ "target_pane_id": "w1:p1", "direction": "right", "cwd": "/tmp/x" })
        );
        assert_eq!(
            split_params("w1:p1", SplitDirection::Down, None),
            json!({ "target_pane_id": "w1:p1", "direction": "down" })
        );
    }

    #[test]
    fn blank_cwd_is_left_out() {
        assert_eq!(
            split_params("p", SplitDirection::Down, Some("  ")),
            json!({ "target_pane_id": "p", "direction": "down" })
        );
        assert_eq!(
            tab_create_params("w1", Some("")),
            json!({ "workspace_id": "w1" })
        );
    }

    #[test]
    fn tab_create_params_carry_workspace_and_cwd() {
        assert_eq!(
            tab_create_params("w1", Some(" /work ")),
            json!({ "workspace_id": "w1", "cwd": "/work" })
        );
    }

    #[test]
    fn close_params_name_the_pane() {
        assert_eq!(close_params("w1:p2"), json!({ "pane_id": "w1:p2" }));
    }

    #[test]
    fn split_direction_parses_from_the_ipc_payload() {
        let right: SplitDirection = serde_json::from_str("\"right\"").unwrap();
        assert_eq!(right, SplitDirection::Right);
        assert!(serde_json::from_str::<SplitDirection>("\"left\"").is_err());
    }

    #[test]
    fn created_pane_id_reads_split_and_tab_results() {
        let split = json!({"type":"pane_info","pane":{"pane_id":"w1:p3"}});
        let tab =
            json!({"type":"tab_created","tab":{"tab_id":"w1:t2"},"root_pane":{"pane_id":"w1:p4"}});
        assert_eq!(created_pane_id(&split).as_deref(), Some("w1:p3"));
        assert_eq!(created_pane_id(&tab).as_deref(), Some("w1:p4"));
        assert_eq!(created_pane_id(&json!({"type":"ok"})), None);
    }

    #[test]
    fn parse_ping_is_none_for_an_old_server() {
        assert_eq!(parse_ping(&json!({"type":"pong"})), None);
        assert_eq!(parse_ping(&json!({"protocol":22})), None);
    }

    #[test]
    fn parse_ping_keeps_the_version_without_a_protocol() {
        assert_eq!(
            parse_ping(&json!({"type":"pong","version":"0.1"})),
            Some(ServerInfo {
                version: "0.1".into(),
                protocol: None,
                supported: SUPPORTED_PROTOCOL
            })
        );
    }
}
