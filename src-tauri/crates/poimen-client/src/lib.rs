//! Client of the `poimen-server` socket API (`docs/protocol.md`): connect, requests and the event
//! stream. Blocking and free of any app dependency, so the app, a CLI and `poimen attach` share it.
//!
//! Requests use short-lived connections; [`Client::subscribe`] keeps its connection open.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub use poimen_protocol as protocol;
use poimen_protocol::{
    AttachMode, AttachResult, ErrorCode, Event, PingResult, Request, ServerLine, Space, SpaceClose,
    SpaceCreate, SpaceRename, Terminal, TerminalAttach, TerminalClose, TerminalCreate,
    TerminalInput, TerminalList, TerminalRename, TerminalResize,
};
use serde_json::{Value, json};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// How long a request waits for its response.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum ClientError {
    /// Nothing listens on the socket (the server is not running).
    Connect(io::Error),
    Io(io::Error),
    /// The server sent something that is not the protocol.
    Protocol(String),
    /// The server answered with an error.
    Server {
        code: ErrorCode,
        message: String,
    },
}

impl ClientError {
    /// Whether the server is simply not there (no socket, or nobody listening).
    pub fn is_not_running(&self) -> bool {
        matches!(self, ClientError::Connect(_))
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Connect(e) => write!(f, "poimen-server is not reachable: {e}"),
            ClientError::Io(e) => write!(f, "poimen-server connection failed: {e}"),
            ClientError::Protocol(m) => write!(f, "poimen-server protocol error: {m}"),
            ClientError::Server { code, message } => write!(f, "{code:?}: {message}"),
        }
    }
}

impl std::error::Error for ClientError {}

#[cfg(unix)]
type Stream = UnixStream;

#[cfg(not(unix))]
struct Stream;

#[cfg(not(unix))]
fn connect_stream(_: &Path) -> io::Result<Stream> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "poimen-server needs Unix sockets",
    ))
}

#[cfg(unix)]
fn connect_stream(path: &Path) -> io::Result<Stream> {
    // Never talk to a socket in a directory another user could have replaced it in. A missing
    // directory falls through to the connect error.
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        match poimen_protocol::check_private_dir(dir, false) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    UnixStream::connect(path)
}

/// A handle on the server socket. Cheap; holds no connection.
#[derive(Debug, Clone)]
pub struct Client {
    path: PathBuf,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

impl Default for Client {
    /// The socket of this user (`protocol::socket_path`).
    fn default() -> Self {
        Self::new(protocol::socket_path())
    }
}

impl Client {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn socket(&self) -> &Path {
        &self.path
    }

    /// Sends one request and returns its `result`.
    pub fn request(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        let mut conn = Connection::open(&self.path, Some(REQUEST_TIMEOUT))?;
        conn.call(method, params)
    }

    /// Opens a connection that receives every event after the call returns.
    pub fn subscribe(&self) -> Result<EventStream, ClientError> {
        let mut conn = Connection::open(&self.path, None)?;
        conn.call("events.subscribe", Value::Null)?;
        Ok(EventStream { conn })
    }

    fn typed<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
        key: Option<&str>,
    ) -> Result<T, ClientError> {
        let mut result = self.request(method, params)?;
        let value = match key {
            Some(key) => result[key].take(),
            None => result,
        };
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(format!("{method}: {e}")))
    }

    pub fn ping(&self) -> Result<PingResult, ClientError> {
        self.typed("ping", Value::Null, None)
    }

    pub fn list_spaces(&self) -> Result<Vec<Space>, ClientError> {
        self.typed("space.list", Value::Null, Some("spaces"))
    }

    pub fn create_space(&self, name: &str, cwd: &str) -> Result<Space, ClientError> {
        self.typed(
            "space.create",
            to_value(&SpaceCreate {
                name: name.into(),
                cwd: cwd.into(),
            }),
            Some("space"),
        )
    }

    pub fn rename_space(&self, space_id: &str, name: &str) -> Result<Space, ClientError> {
        self.typed(
            "space.rename",
            to_value(&SpaceRename {
                space_id: space_id.into(),
                name: name.into(),
            }),
            Some("space"),
        )
    }

    pub fn close_space(&self, space_id: &str) -> Result<(), ClientError> {
        self.request(
            "space.close",
            to_value(&SpaceClose {
                space_id: space_id.into(),
            }),
        )
        .map(drop)
    }

    pub fn list_terminals(&self, space_id: Option<&str>) -> Result<Vec<Terminal>, ClientError> {
        self.typed(
            "terminal.list",
            to_value(&TerminalList {
                space_id: space_id.map(str::to_string),
            }),
            Some("terminals"),
        )
    }

    pub fn create_terminal(&self, params: &TerminalCreate) -> Result<Terminal, ClientError> {
        self.typed("terminal.create", to_value(params), Some("terminal"))
    }

    pub fn rename_terminal(&self, terminal_id: &str, name: &str) -> Result<Terminal, ClientError> {
        self.typed(
            "terminal.rename",
            to_value(&TerminalRename {
                terminal_id: terminal_id.into(),
                name: name.into(),
            }),
            Some("terminal"),
        )
    }

    pub fn close_terminal(&self, terminal_id: &str) -> Result<(), ClientError> {
        self.request(
            "terminal.close",
            to_value(&TerminalClose {
                terminal_id: terminal_id.into(),
            }),
        )
        .map(drop)
    }

    pub fn input(&self, terminal_id: &str, data: &str) -> Result<(), ClientError> {
        self.request(
            "terminal.input",
            to_value(&TerminalInput {
                terminal_id: terminal_id.into(),
                data: data.into(),
            }),
        )
        .map(drop)
    }

    /// Attaches to a terminal on a connection of its own. The result carries the snapshot; the
    /// [`Attachment`] then yields the stream records and sends input and resizes on the same
    /// connection (the server accepts those only from the controller).
    pub fn attach(
        &self,
        terminal_id: &str,
        mode: AttachMode,
        take_over: bool,
    ) -> Result<Attachment, ClientError> {
        let mut conn = Connection::open(&self.path, None)?;
        let result = conn.call(
            "terminal.attach",
            to_value(&TerminalAttach {
                terminal_id: terminal_id.into(),
                mode,
                take_over,
            }),
        )?;
        let info: AttachResult = serde_json::from_value(result)
            .map_err(|e| ClientError::Protocol(format!("terminal.attach: {e}")))?;
        Ok(Attachment { info, conn })
    }

    pub fn resize(&self, terminal_id: &str, cols: u16, rows: u16) -> Result<(), ClientError> {
        self.request(
            "terminal.resize",
            to_value(&TerminalResize {
                terminal_id: terminal_id.into(),
                cols,
                rows,
            }),
        )
        .map(drop)
    }
}

/// A connection attached to a terminal. Dropping it detaches.
pub struct Attachment {
    pub info: AttachResult,
    #[cfg_attr(not(unix), allow(dead_code))]
    conn: Connection,
}

impl Attachment {
    /// Sets how long [`Attachment::next_record`] waits for a record (`None`: for ever).
    #[cfg(unix)]
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ClientError> {
        self.conn
            .reader
            .get_ref()
            .set_read_timeout(timeout)
            .map_err(ClientError::Io)
    }

    /// The next stream record (`terminal.output`, `terminal.resized`, `terminal.detached`,
    /// `terminal.exited`). An error when the connection ended or the wait timed out.
    #[cfg(unix)]
    pub fn next_record(&mut self) -> Result<Event, ClientError> {
        loop {
            if let Some(event) = self.conn.pending.pop_front() {
                return Ok(event);
            }
            if let ServerLine::Event(event) = self.conn.read_line()? {
                return Ok(event);
            }
        }
    }

    #[cfg(not(unix))]
    pub fn next_record(&mut self) -> Result<Event, ClientError> {
        unreachable!("connect_stream fails off Unix")
    }

    /// Input for the terminal (controller only).
    pub fn input(&mut self, data: &str) -> Result<(), ClientError> {
        let params = to_value(&TerminalInput {
            terminal_id: self.info.terminal_id.clone(),
            data: data.into(),
        });
        self.conn.call("terminal.input", params).map(drop)
    }

    /// Resizes the terminal (controller only; the size change reaches everyone as a record).
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), ClientError> {
        let params = to_value(&TerminalResize {
            terminal_id: self.info.terminal_id.clone(),
            cols,
            rows,
        });
        self.conn.call("terminal.resize", params).map(drop)
    }
}

fn to_value<T: serde::Serialize>(params: &T) -> Value {
    // The protocol types always serialize.
    serde_json::to_value(params).unwrap_or_else(|_| json!({}))
}

struct Connection {
    reader: BufReader<Stream>,
    /// Events that arrived while a request waited for its response.
    pending: VecDeque<Event>,
}

impl Connection {
    #[cfg(unix)]
    fn open(path: &Path, timeout: Option<Duration>) -> Result<Self, ClientError> {
        let stream = connect_stream(path).map_err(ClientError::Connect)?;
        stream.set_read_timeout(timeout).map_err(ClientError::Io)?;
        stream.set_write_timeout(timeout).map_err(ClientError::Io)?;
        Ok(Self {
            reader: BufReader::new(stream),
            pending: VecDeque::new(),
        })
    }

    #[cfg(not(unix))]
    fn open(path: &Path, _: Option<Duration>) -> Result<Self, ClientError> {
        connect_stream(path).map_err(ClientError::Connect)?;
        unreachable!("connect_stream fails off Unix")
    }

    #[cfg(unix)]
    fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = next_id();
        let request = Request {
            id: id.clone(),
            method: method.into(),
            params,
        };
        let mut line =
            serde_json::to_string(&request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        line.push('\n');
        let stream = self.reader.get_mut();
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(ClientError::Io)?;
        loop {
            match self.read_line()? {
                // Events may interleave on a subscribed or attached connection; a request waits
                // for its id and keeps them for the reader.
                ServerLine::Event(event) => self.pending.push_back(event),
                ServerLine::Response(r) if r.id.as_deref() == Some(&id) || r.id.is_none() => {
                    return match (r.result, r.error) {
                        (_, Some(e)) => Err(ClientError::Server {
                            code: e.code,
                            message: e.message,
                        }),
                        (Some(result), None) => Ok(result),
                        (None, None) => Ok(Value::Null),
                    };
                }
                ServerLine::Response(_) => {}
            }
        }
    }

    #[cfg(not(unix))]
    fn call(&mut self, _: &str, _: Value) -> Result<Value, ClientError> {
        unreachable!("connect_stream fails off Unix")
    }

    #[cfg(unix)]
    fn read_line(&mut self) -> Result<ServerLine, ClientError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).map_err(ClientError::Io)?;
        if n == 0 {
            return Err(ClientError::Io(io::ErrorKind::UnexpectedEof.into()));
        }
        protocol::parse_server_line(line.trim_end())
            .map_err(|e| ClientError::Protocol(format!("{e}: {}", line.trim_end())))
    }
}

/// The events of a subscribed connection; ends with an error when the connection drops (the
/// server stopped), after which the caller resubscribes and re-reads the lists.
pub struct EventStream {
    #[cfg_attr(not(unix), allow(dead_code))]
    conn: Connection,
}

impl Iterator for EventStream {
    type Item = Result<Event, ClientError>;

    #[cfg(unix)]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(event) = self.conn.pending.pop_front() {
                return Some(Ok(event));
            }
            match self.conn.read_line() {
                Ok(ServerLine::Event(event)) => return Some(Ok(event)),
                Ok(ServerLine::Response(_)) => {}
                Err(e) => return Some(Err(e)),
            }
        }
    }

    #[cfg(not(unix))]
    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn temp_socket(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-client-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        dir.join("s.sock")
    }

    /// A server that answers each connection with the given lines, after reading one request.
    fn fake_server(path: &Path, lines: Vec<String>) -> thread::JoinHandle<String> {
        let listener = UnixListener::bind(path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let id = serde_json::from_str::<Value>(&request).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let mut stream = stream;
            for line in lines {
                writeln!(stream, "{}", line.replace("$ID", &id)).unwrap();
            }
            request
        })
    }

    #[test]
    fn request_returns_the_result_and_sends_one_ndjson_line() {
        let path = temp_socket("ok");
        let server = fake_server(
            &path,
            vec![r#"{"id":"$ID","result":{"spaces":[{"id":"s1","name":"a","cwd":"/x"}]}}"#.into()],
        );
        let spaces = Client::new(&path).list_spaces().unwrap();
        assert_eq!(spaces[0].name, "a");
        let sent = server.join().unwrap();
        assert!(sent.ends_with('\n'));
        assert!(sent.contains(r#""method":"space.list""#));
    }

    #[test]
    fn server_errors_keep_code_and_message() {
        let path = temp_socket("err");
        let server = fake_server(
            &path,
            vec![r#"{"id":"$ID","error":{"code":"not_found","message":"no such space"}}"#.into()],
        );
        match Client::new(&path).close_space("nope") {
            Err(ClientError::Server { code, message }) => {
                assert_eq!(code, ErrorCode::NotFound);
                assert_eq!(message, "no such space");
            }
            other => panic!("{other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn a_missing_socket_is_not_running() {
        let err = Client::new(temp_socket("missing")).ping().unwrap_err();
        assert!(err.is_not_running());
    }

    #[test]
    fn subscribe_yields_events_then_an_error_when_the_connection_ends() {
        let path = temp_socket("events");
        let server = fake_server(
            &path,
            vec![
                r#"{"id":"$ID","result":{"subscribed":true}}"#.into(),
                r#"{"event":"space.closed","data":{"space_id":"s1"}}"#.into(),
            ],
        );
        let mut stream = Client::new(&path).subscribe().unwrap();
        assert_eq!(
            stream.next().unwrap().unwrap(),
            Event::SpaceClosed {
                space_id: "s1".into()
            }
        );
        server.join().unwrap();
        assert!(stream.next().unwrap().is_err());
    }

    #[test]
    fn garbage_from_the_server_is_a_protocol_error() {
        let path = temp_socket("garbage");
        let server = fake_server(&path, vec!["not json".into()]);
        assert!(matches!(
            Client::new(&path).ping(),
            Err(ClientError::Protocol(_))
        ));
        server.join().unwrap();
    }

    #[test]
    fn a_socket_in_a_loose_directory_is_refused_before_connecting() {
        let path = temp_socket("loose");
        let _server = UnixListener::bind(&path).unwrap();
        let dir = path.parent().unwrap();
        std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o777)).unwrap();
        let err = Client::new(&path).ping().unwrap_err();
        assert!(
            matches!(err, ClientError::Connect(ref e) if e.kind() == io::ErrorKind::PermissionDenied)
        );
    }
}
