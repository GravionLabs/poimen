//! Client of the `poimen-server` socket API (`docs/protocol.md`): connect, requests and the event
//! stream. Blocking and free of any app dependency, so the app, a CLI and `poimen attach` share it.
//!
//! Requests use short-lived connections; [`Client::subscribe`] keeps its connection open.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub mod agent;
pub mod transport;
pub use poimen_protocol as protocol;
use poimen_protocol::{
    AttachMode, AttachResult, ErrorCode, Event, PingResult, Request, ServerLine, SetColors, Space,
    SpaceClose, SpaceCreate, SpaceList, SpaceRename, Tab, TabClose, TabCreate, TabFocus, TabMove,
    TabRename, TabResizeSplit, TabSplit, Terminal, TerminalAttach, TerminalClose, TerminalCreate,
    TerminalDetach, TerminalInput, TerminalList, TerminalRename, TerminalResize,
};
use serde_json::{Value, json};

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

type Stream = transport::Conn;

fn connect_stream(path: &Path) -> io::Result<Stream> {
    // Never talk to a socket in a directory another user could have replaced it in. A missing
    // directory falls through to the connect error. (Windows: the pipe's DACL protects it.)
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        match poimen_protocol::check_private_dir(dir, false) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    transport::Conn::connect(path).map_err(|e| if cfg!(windows) { explain_denied(e) } else { e })
}

/// A denied connect on Windows: the pipe admits only its owner, and a server started elevated
/// is owned by Administrators. The error says so (it keeps its kind).
fn explain_denied(e: io::Error) -> io::Error {
    if e.kind() != io::ErrorKind::PermissionDenied {
        return e;
    }
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "access denied ({e}); poimen-server probably runs elevated (as administrator): \
             quit it and start poimen normally, not as administrator"
        ),
    )
}

/// One request on a connection of its own, bounded by the socket timeouts where they exist.
fn request_on(path: &Path, method: &str, params: Value) -> Result<Value, ClientError> {
    let mut conn = Connection::open(path, Some(REQUEST_TIMEOUT))?;
    conn.call(method, params)
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

    /// Sends one request and returns its `result`, waiting at most [`REQUEST_TIMEOUT`].
    pub fn request(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        if transport::TIMEOUTS_SUPPORTED {
            return request_on(&self.path, method, params);
        }
        // Named pipes have no I/O timeouts: the request runs on a thread of its own, which ends
        // when the connection does.
        let (tx, rx) = mpsc::channel();
        let (path, method) = (self.path.clone(), method.to_string());
        thread::spawn(move || {
            let _ = tx.send(request_on(&path, &method, params));
        });
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(ClientError::Io(io::ErrorKind::TimedOut.into()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClientError::Io(io::Error::other(
                "the request thread ended without an answer",
            ))),
        }
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

    /// The spaces with their tabs and split trees (`space.list`).
    pub fn list_layout(&self) -> Result<SpaceList, ClientError> {
        self.typed("space.list", Value::Null, None)
    }

    /// A new tab at the end of the space with `terminal_id` (moved out of its tab) or a new
    /// terminal; returns the tab and its terminal.
    pub fn create_tab(&self, params: &TabCreate) -> Result<(Tab, Terminal), ClientError> {
        self.tab_and_terminal("tab.create", to_value(params))
    }

    pub fn rename_tab(&self, tab_id: &str, name: &str) -> Result<Tab, ClientError> {
        let params = TabRename {
            tab_id: tab_id.into(),
            name: name.into(),
        };
        self.typed("tab.rename", to_value(&params), Some("tab"))
    }

    /// Closes the tab and its terminals.
    pub fn close_tab(&self, tab_id: &str) -> Result<(), ClientError> {
        let params = TabClose {
            tab_id: tab_id.into(),
        };
        self.request("tab.close", to_value(&params)).map(drop)
    }

    /// Moves the tab to `index` of its space's order; returns the space with the new order.
    pub fn move_tab(&self, tab_id: &str, index: usize) -> Result<Space, ClientError> {
        let params = TabMove {
            tab_id: tab_id.into(),
            index,
        };
        self.typed("tab.move", to_value(&params), Some("space"))
    }

    /// Creates a terminal and splits the leaf of `params.terminal_id`; returns the tab and the
    /// new terminal.
    pub fn split(&self, params: &TabSplit) -> Result<(Tab, Terminal), ClientError> {
        self.tab_and_terminal("tab.split", to_value(params))
    }

    /// Sets the ratio of the split `split_id` (an unknown one is `not_found`).
    pub fn resize_split(
        &self,
        tab_id: &str,
        split_id: &str,
        ratio: f64,
    ) -> Result<Tab, ClientError> {
        let params = TabResizeSplit {
            tab_id: tab_id.into(),
            split_id: split_id.into(),
            ratio,
        };
        self.typed("tab.resize_split", to_value(&params), Some("tab"))
    }

    /// Focuses the tab (and the leaf of `terminal_id` in it), for every client.
    pub fn focus_tab(&self, tab_id: &str, terminal_id: Option<&str>) -> Result<Tab, ClientError> {
        let params = TabFocus {
            tab_id: tab_id.into(),
            terminal_id: terminal_id.map(str::to_string),
        };
        self.typed("tab.focus", to_value(&params), Some("tab"))
    }

    fn tab_and_terminal(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(Tab, Terminal), ClientError> {
        let mut result = self.request(method, params)?;
        let parse = |e: serde_json::Error| ClientError::Protocol(format!("{method}: {e}"));
        Ok((
            serde_json::from_value(result["tab"].take()).map_err(parse)?,
            serde_json::from_value(result["terminal"].take()).map_err(parse)?,
        ))
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

    /// Sends the colors the server answers terminal color queries with while no controller is
    /// attached (`server.set_colors`).
    pub fn set_colors(&self, colors: &SetColors) -> Result<(), ClientError> {
        self.request("server.set_colors", to_value(colors))?;
        Ok(())
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

    /// Asks the server to end every terminal and exit (`server.shutdown`). Only after the user
    /// confirmed; the server answers, then goes away shortly after.
    pub fn shutdown(&self) -> Result<(), ClientError> {
        self.request("server.shutdown", Value::Null).map(drop)
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
    conn: Connection,
}

impl Attachment {
    /// Sets how long [`Attachment::next_record`] waits for a record (`None`: for ever).
    /// Unsupported on Windows (named pipes have no read timeout).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ClientError> {
        if !transport::TIMEOUTS_SUPPORTED {
            return Err(ClientError::Io(io::Error::new(
                io::ErrorKind::Unsupported,
                "named pipes have no read timeout",
            )));
        }
        self.conn
            .reader
            .get_ref()
            .set_read_timeout(timeout)
            .map_err(ClientError::Io)
    }

    /// The next stream record (`terminal.output`, `terminal.resized`, `terminal.detached`,
    /// `terminal.exited`). An error when the connection ended or the wait timed out.
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

/// The write half of an attachment, for another thread than the one reading records. Requests
/// are sent on the attached connection (so the server applies the controller rules) and their
/// responses are not awaited: [`Attachment::next_record`] skips them.
pub struct AttachmentWriter {
    stream: Stream,
    terminal_id: String,
}

impl Attachment {
    /// A writer on the same connection. Closing it ends the attachment and makes a blocked
    /// [`Attachment::next_record`] fail.
    pub fn writer(&self) -> Result<AttachmentWriter, ClientError> {
        Ok(AttachmentWriter {
            stream: self.conn.reader.get_ref().clone(),
            terminal_id: self.info.terminal_id.clone(),
        })
    }
}

impl AttachmentWriter {
    /// Input for the terminal (accepted from the controller only).
    pub fn input(&mut self, data: &str) -> Result<(), ClientError> {
        let params = to_value(&TerminalInput {
            terminal_id: self.terminal_id.clone(),
            data: data.into(),
        });
        self.send("terminal.input", params)
    }

    /// Resizes the terminal (accepted from the controller only).
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), ClientError> {
        let params = to_value(&TerminalResize {
            terminal_id: self.terminal_id.clone(),
            cols,
            rows,
        });
        self.send("terminal.resize", params)
    }

    /// Ends the attachment; the terminal keeps running. Sends `terminal.detach`, after which the
    /// server closes the connection (named pipes on Windows have no `shutdown`), and on Unix
    /// also shuts the socket down, so a blocked [`Attachment::next_record`] fails at once.
    pub fn close(&self) {
        let params = to_value(&TerminalDetach {
            terminal_id: self.terminal_id.clone(),
        });
        let _ = self.send("terminal.detach", params);
        let _ = self.stream.shutdown();
    }

    fn send(&self, method: &str, params: Value) -> Result<(), ClientError> {
        let request = Request {
            id: next_id(),
            method: method.into(),
            params,
        };
        let mut line =
            serde_json::to_string(&request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        line.push('\n');
        let mut stream = &self.stream;
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(ClientError::Io)
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
    fn open(path: &Path, timeout: Option<Duration>) -> Result<Self, ClientError> {
        let stream = connect_stream(path).map_err(ClientError::Connect)?;
        stream.set_read_timeout(timeout).map_err(ClientError::Io)?;
        stream.set_write_timeout(timeout).map_err(ClientError::Io)?;
        Ok(Self {
            reader: BufReader::new(stream),
            pending: VecDeque::new(),
        })
    }

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
    conn: Connection,
}

impl Iterator for EventStream {
    type Item = Result<Event, ClientError>;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport::Listener;

    fn temp_socket(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-client-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        dir.join("s.sock")
    }

    /// A server that answers each connection with the given lines, after reading one request.
    fn fake_server(path: &Path, lines: Vec<String>) -> thread::JoinHandle<String> {
        let listener = Listener::bind(path).unwrap();
        thread::spawn(move || {
            let stream = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.clone());
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
    fn list_layout_reads_spaces_with_their_tabs() {
        let path = temp_socket("layout");
        let server =
            fake_server(
                &path,
                vec![concat!(
                r#"{"id":"$ID","result":{"spaces":[{"id":"s1","name":"a","cwd":"/x","#,
                r#""tabs":["tab2"],"active_tab":"tab2"}],"tabs":[{"tab_id":"tab2","#,
                r#""space_id":"s1","name":"sh","root":{"kind":"terminal","terminal_id":"t3"},"#,
                r#""active_terminal":"t3"}]}}"#
            )
            .into()],
            );
        let layout = Client::new(&path).list_layout().unwrap();
        assert_eq!(layout.spaces[0].active_tab.as_deref(), Some("tab2"));
        assert_eq!(layout.tabs[0].root.terminal_ids(), ["t3"]);
        server.join().unwrap();
    }

    #[test]
    fn split_sends_its_params_and_returns_the_tab_and_the_new_terminal() {
        let path = temp_socket("split");
        let server =
            fake_server(
                &path,
                vec![concat!(
                r#"{"id":"$ID","result":{"tab":{"tab_id":"tab2","space_id":"s1","name":"sh","#,
                r#""root":{"kind":"split","split_id":"split5","direction":"down","ratio":0.4,"#,
                r#""children":[{"kind":"terminal","terminal_id":"t3"},"#,
                r#"{"kind":"terminal","terminal_id":"t4"}]},"active_terminal":"t3"},"#,
                r#""terminal":{"id":"t4","space_id":"s1","name":"sh","cwd":"/x","cols":80,"#,
                r#""rows":24,"status":"running"}}}"#
            )
            .into()],
            );
        let (tab, terminal) = Client::new(&path)
            .split(&TabSplit {
                terminal_id: "t3".into(),
                direction: protocol::SplitDirection::Down,
                ratio: Some(0.4),
                cwd: None,
                name: None,
                command: None,
            })
            .unwrap();
        assert_eq!(tab.root.split_ids(), ["split5"]);
        assert_eq!(terminal.id, "t4");
        let sent = server.join().unwrap();
        assert!(sent.contains(r#""method":"tab.split""#), "{sent}");
        assert!(sent.contains(r#""direction":"down""#), "{sent}");
        assert!(sent.contains(r#""ratio":0.4"#), "{sent}");
        assert!(!sent.contains("cwd"), "{sent}");
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
    fn an_attachment_writer_sends_on_the_same_connection_and_close_ends_the_reader() {
        let path = temp_socket("writer");
        let listener = Listener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.clone());
            let mut attach = String::new();
            reader.read_line(&mut attach).unwrap();
            let id = serde_json::from_str::<Value>(&attach).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            writeln!(
                stream,
                r#"{{"id":"{id}","result":{{"terminal_id":"t1","seq":0,"cols":80,"rows":24,"mode":"control","snapshot":""}}}}"#
            )
            .unwrap();
            let mut input = String::new();
            reader.read_line(&mut input).unwrap();
            // The response to the input must not confuse the reader.
            writeln!(stream, r#"{{"id":"x","result":{{}}}}"#).unwrap();
            writeln!(
                stream,
                r#"{{"event":"terminal.output","data":{{"terminal_id":"t1","seq":1,"data":"YQ=="}}}}"#
            )
            .unwrap();
            // Closing the writer asks the server to detach; the server then ends the connection
            // (all a named pipe can do), which makes the blocked reader fail.
            let mut detach = String::new();
            reader.read_line(&mut detach).unwrap();
            (input, detach)
        });
        let mut attachment = Client::new(&path)
            .attach("t1", AttachMode::Control, false)
            .unwrap();
        let mut writer = attachment.writer().unwrap();
        writer.input("ls\r").unwrap();
        assert!(matches!(
            attachment.next_record().unwrap(),
            Event::TerminalOutput { seq: 1, .. }
        ));
        writer.close();
        assert!(attachment.next_record().is_err());
        let (input, detach) = server.join().unwrap();
        assert!(detach.contains(r#""method":"terminal.detach""#), "{detach}");
        assert!(detach.contains(r#""terminal_id":"t1""#), "{detach}");
        assert!(input.contains(r#""method":"terminal.input""#));
        assert!(input.contains(r#""terminal_id":"t1""#));
        assert!(input.contains(r#""data":"ls\r""#));
    }

    #[test]
    fn a_denied_connect_explains_elevation_and_keeps_its_kind() {
        let e = explain_denied(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("not as administrator"), "{e}");
        let other = explain_denied(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(other.kind(), io::ErrorKind::NotFound);
        assert!(!other.to_string().contains("administrator"));
    }

    /// Unix only on purpose: Windows has no directory modes; the pipe's DACL protects it.
    #[cfg(unix)]
    #[test]
    fn a_socket_in_a_loose_directory_is_refused_before_connecting() {
        let path = temp_socket("loose");
        let _server = Listener::bind(&path).unwrap();
        let dir = path.parent().unwrap();
        std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o777)).unwrap();
        let err = Client::new(&path).ping().unwrap_err();
        assert!(
            matches!(err, ClientError::Connect(ref e) if e.kind() == io::ErrorKind::PermissionDenied)
        );
    }
}
