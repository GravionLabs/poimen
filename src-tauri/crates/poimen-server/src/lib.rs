//! `poimen-server`: a headless daemon that owns the PTYs, so shells and agents keep running when
//! the app closes. One server per user listens on a local socket (a Unix socket, a named pipe on
//! Windows; `docs/protocol.md`).
//!
//! The server keeps spaces (name, cwd) with terminals, and per space the tabs in order with a
//! split tree of terminals each (`tabs.rs`). Spaces and tabs persist across restarts, terminals
//! do not; leaves whose terminal is gone are dropped at start.

pub mod claude;
pub mod copilot;
mod emulator;
pub mod hook;
mod pty;
pub mod report;
pub mod stdio;
mod store;
mod stream;
mod tabs;

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::vte::ansi::Rgb;
use emulator::Colors;
use poimen_protocol::{
    self as proto, AgentInfo, AgentStatus, AttachMode, DetachReason, ErrorCode, Event,
    PROTOCOL_VERSION, PingResult, Request, Response, SetColors, Space, SpaceClose, SpaceCreate,
    SpaceList, SpaceRename, TabClose, TabCreate, TabFocus, TabMove, TabRename, TabResizeSplit,
    TabSplit, Terminal, TerminalAttach, TerminalClearTokens, TerminalClose, TerminalCreate,
    TerminalDetach, TerminalInput, TerminalList, TerminalRename, TerminalReportAgent,
    TerminalReportTokens, TerminalResize, TerminalStatus,
};
use serde_json::{Value, json};

pub use emulator::DEFAULT_SCROLLBACK;
pub use store::state_file_path;
use stream::{CLIENT_QUEUE, Line, Link, TermStream, line_of};

/// How long a write to a client may block before the connection is given up. Only the writer
/// thread of that client waits; the PTY and the other clients never do.
/// How often the tokens with a TTL are checked.
const TOKEN_SWEEP: Duration = Duration::from_millis(500);

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

use poimen_client::transport::{self, Conn, Entry, Listener};

/// Where the server listens and keeps its spaces.
#[derive(Debug, Clone)]
pub struct Config {
    pub socket: PathBuf,
    pub state_file: PathBuf,
    /// Lines of scrollback kept per terminal for the attach snapshot.
    pub scrollback: usize,
}

impl Config {
    /// The paths of this user (`POIMEN_SOCKET`, `POIMEN_STATE_DIR` override the defaults) and
    /// the scrollback (`POIMEN_SCROLLBACK`, lines, default [`DEFAULT_SCROLLBACK`]).
    pub fn from_env() -> Self {
        Self {
            socket: proto::socket_path(),
            state_file: state_file_path(),
            scrollback: std::env::var("POIMEN_SCROLLBACK")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(DEFAULT_SCROLLBACK),
        }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// Another server answers on the socket.
    AlreadyRunning(PathBuf),
    Io(io::Error),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::AlreadyRunning(p) => {
                write!(f, "poimen-server already runs on {}", p.display())
            }
            StartError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<io::Error> for StartError {
    fn from(e: io::Error) -> Self {
        StartError::Io(e)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct TerminalEntry {
    info: Terminal,
    /// `None` once the process has exited.
    pty: Option<pty::Pty>,
    /// The screen state and the attached clients; outlives the process.
    stream: Arc<TermStream>,
    /// `seq` of the last accepted agent report; older or equal reports are dropped.
    agent_seq: Option<u64>,
    /// Who set which token and until when; `info.tokens` is the visible name-to-value view.
    tokens: Vec<TokenEntry>,
}

struct TokenEntry {
    name: String,
    source: String,
    expires: Option<Instant>,
}

impl TerminalEntry {
    /// Drops the tokens whose time is up; true when any went.
    fn expire_tokens(&mut self, now: Instant) -> bool {
        let before = self.tokens.len();
        let info = &mut self.info;
        self.tokens.retain(|t| {
            let alive = t.expires.is_none_or(|at| at > now);
            if !alive {
                info.tokens.remove(&t.name);
            }
            alive
        });
        self.tokens.len() != before
    }
}

#[derive(Default)]
struct Inner {
    /// The spaces with their tabs and split trees.
    layout: tabs::Layout,
    terminals: Vec<TerminalEntry>,
}

/// How long `server.shutdown` waits so its answer reaches the client first.
const SHUTDOWN_DELAY: Duration = Duration::from_millis(100);

pub(crate) struct Shared {
    inner: Mutex<Inner>,
    subscribers: Mutex<Vec<Sender<Event>>>,
    next_id: AtomicU64,
    socket: PathBuf,
    state_file: PathBuf,
    scrollback: usize,
    /// The scheme colors for terminal queries, shared with every terminal's stream.
    colors: Arc<RwLock<Colors>>,
    /// Run once the terminals are ended after `server.shutdown`; the binary exits the process.
    on_shutdown: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Shared {
    /// Sends the event to every subscriber, dropping the ones that went away. Called with the
    /// `inner` lock held, so events arrive in the order the changes happened.
    fn publish(&self, event: Event) {
        lock(&self.subscribers).retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Publishes the events of a layout change and saves the layout when there were any.
    fn apply(&self, inner: &Inner, events: Vec<Event>) {
        if events.is_empty() {
            return;
        }
        for event in events {
            self.publish(event);
        }
        self.persist(inner);
    }

    fn new_id(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn persist(&self, inner: &Inner) {
        let next_id = self.next_id.load(Ordering::Relaxed);
        if let Err(e) = store::save(&self.state_file, &inner.layout, next_id) {
            eprintln!(
                "poimen-server: could not save {}: {e}",
                self.state_file.display()
            );
        }
    }

    /// Expires the tokens whose TTL is up and tells the subscribers.
    pub(crate) fn expire_tokens(&self) {
        let now = Instant::now();
        let mut inner = lock(&self.inner);
        for entry in &mut inner.terminals {
            if entry.expire_tokens(now) {
                self.publish(Event::TerminalTokensChanged {
                    terminal_id: entry.info.id.clone(),
                    tokens: entry.info.tokens.clone(),
                });
            }
        }
    }

    /// Marks a terminal exited when its process ends (unless it was closed meanwhile).
    pub(crate) fn terminal_exited(&self, terminal_id: &str, exit_code: Option<u32>) {
        let mut inner = lock(&self.inner);
        let Some(entry) = inner
            .terminals
            .iter_mut()
            .find(|t| t.info.id == terminal_id)
        else {
            return;
        };
        if entry.info.status == TerminalStatus::Exited {
            return;
        }
        entry.info.status = TerminalStatus::Exited;
        entry.info.exit_code = exit_code;
        entry.pty = None;
        entry.info.agent = None;
        if !entry.info.tokens.is_empty() {
            entry.info.tokens.clear();
            entry.tokens.clear();
            self.publish(Event::TerminalTokensChanged {
                terminal_id: terminal_id.to_string(),
                tokens: Default::default(),
            });
        }
        entry.stream.exited(exit_code);
        self.publish(Event::TerminalExited {
            terminal_id: terminal_id.to_string(),
            exit_code,
        });
        // An exited terminal is no longer in the tree.
        let events = inner.layout.remove_terminal(terminal_id);
        self.apply(&inner, events);
    }
}

type ApiResult = Result<Value, (ErrorCode, String)>;

fn not_found(what: &str, id: &str) -> (ErrorCode, String) {
    (ErrorCode::NotFound, format!("no {what} {id}"))
}

/// A bound server, ready to [`serve`](Server::serve).
pub struct Server {
    listener: Listener,
    /// Keeps the lock next to the socket for as long as the server lives.
    _lock: Option<std::fs::File>,
    shared: Arc<Shared>,
}

impl Server {
    /// Binds the socket (Unix: directory mode 0700, socket 0600; Windows: the named pipe and its
    /// marker file), replacing a stale entry, and loads the persisted spaces. Fails with
    /// [`StartError::AlreadyRunning`] when a server answers.
    pub fn bind(config: Config) -> Result<Self, StartError> {
        let mut lock_file = None;
        if let Some(dir) = config.socket.parent().filter(|d| !d.as_os_str().is_empty()) {
            let file = open_lock(dir)?;
            // Held for the life of the server: whoever holds it is the only one to judge the
            // socket entry stale.
            if file.try_lock().is_err() {
                return Err(StartError::AlreadyRunning(config.socket));
            }
            lock_file = Some(file);
        }
        match transport::inspect(&config.socket)? {
            Entry::Missing => {}
            Entry::Other => {
                return Err(StartError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a socket", config.socket.display()),
                )));
            }
            Entry::Socket if transport::is_listening(&config.socket) => {
                return Err(StartError::AlreadyRunning(config.socket));
            }
            Entry::Socket => std::fs::remove_file(&config.socket)?,
        }
        let listener = Listener::bind(&config.socket).map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse {
                StartError::AlreadyRunning(config.socket.clone())
            } else {
                StartError::Io(e)
            }
        })?;

        let store::Stored {
            mut layout,
            next_id,
        } = store::load(&config.state_file);
        let next_id = AtomicU64::new(next_id);
        // No terminal survives a restart (yet): their leaves, the splits they leave behind and
        // the empty tabs go.
        let changed = layout.reconcile(&[], &mut |prefix| {
            format!("{prefix}{}", next_id.fetch_add(1, Ordering::Relaxed))
        });
        let shared = Shared {
            inner: Mutex::new(Inner {
                layout,
                terminals: Vec::new(),
            }),
            subscribers: Mutex::new(Vec::new()),
            next_id,
            socket: config.socket,
            state_file: config.state_file,
            scrollback: config.scrollback,
            colors: Arc::default(),
            on_shutdown: Mutex::new(None),
        };
        if changed {
            shared.persist(&lock(&shared.inner));
        }
        Ok(Self {
            listener,
            _lock: lock_file,
            shared: Arc::new(shared),
        })
    }

    pub fn socket(&self) -> &Path {
        &self.shared.socket
    }

    /// Sets what runs after a `server.shutdown` ended the terminals and removed the socket; the
    /// `poimen-server` binary exits the process there. Without it the server only stops serving
    /// its terminals (tests use this to observe a shutdown without ending their process).
    pub fn on_shutdown(&self, f: impl FnOnce() + Send + 'static) {
        *lock(&self.shared.on_shutdown) = Some(Box::new(f));
    }

    /// Accepts connections forever, one thread each.
    pub fn serve(self) {
        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            loop {
                thread::sleep(TOKEN_SWEEP);
                shared.expire_tokens();
            }
        });
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let shared = Arc::clone(&self.shared);
                    thread::spawn(move || handle_connection(&shared, stream));
                }
                Err(e) => eprintln!("poimen-server: accept failed: {e}"),
            }
        }
    }
}

/// Where a connection's responses go: straight to the socket, or, once it attached, through the
/// bounded queue of its writer thread (the stream records share that queue).
enum Out {
    Direct(Conn),
    Queue(SyncSender<Line>),
}

impl Out {
    fn send<T: serde::Serialize>(&mut self, value: &T) -> io::Result<()> {
        match self {
            Out::Direct(stream) => send(stream, value),
            Out::Queue(tx) => tx
                .send(Line {
                    text: line_of(value),
                    last: false,
                })
                .map_err(|_| io::ErrorKind::BrokenPipe.into()),
        }
    }
}

/// What a connection that attached to a terminal knows about it.
pub(crate) struct Attachment {
    stream: Arc<TermStream>,
    terminal_id: String,
    client_id: u64,
    mode: AttachMode,
}

fn handle_connection(shared: &Arc<Shared>, stream: Conn) {
    let mut attachment = None;
    serve_requests(shared, stream, &mut attachment);
    if let Some(a) = attachment {
        a.stream.detach(a.client_id);
    }
}

fn serve_requests(shared: &Arc<Shared>, stream: Conn, attachment: &mut Option<Attachment>) {
    let mut out = Out::Direct(stream.clone());
    for line in BufReader::new(stream.clone()).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let request = match proto::parse_request(&line) {
            Ok(r) => r,
            Err(response) => {
                if out.send(&response).is_err() {
                    return;
                }
                continue;
            }
        };
        if request.method == "events.subscribe" && attachment.is_none() {
            // The subscription is registered before the response is sent, so an event caused by
            // the next request on another connection is never missed.
            let (tx, rx) = mpsc::channel();
            lock(&shared.subscribers).push(tx);
            let ok = Response::ok(&request.id, json!({ "subscribed": true }));
            if out.send(&ok).is_err() {
                return;
            }
            // From here the connection only carries events.
            let mut writer = stream;
            for event in rx {
                if send(&mut writer, &event).is_err() {
                    return;
                }
            }
            return;
        }
        if request.method == "terminal.attach" {
            // On success the response (with the snapshot) is already queued for the client.
            match attach(shared, &request, &stream, attachment) {
                Ok(tx) => out = Out::Queue(tx),
                Err((code, message)) => {
                    let response = Response::err(Some(&request.id), code, message);
                    if out.send(&response).is_err() {
                        return;
                    }
                }
            }
            continue;
        }
        if request.method == "terminal.detach" {
            let response = match detach(&request, attachment.as_ref()) {
                Ok(()) => Response::ok(&request.id, json!({ "detached": true })),
                Err((code, message)) => Response::err(Some(&request.id), code, message),
            };
            let detached = response.error.is_none();
            // After the answer every handle of the connection is dropped, which closes it; the
            // caller then detaches (named pipes have no shutdown to wake the client's reader).
            if out.send(&response).is_err() || detached {
                return;
            }
            continue;
        }
        let response = match dispatch(shared, &request, attachment.as_ref()) {
            Ok(result) => Response::ok(&request.id, result),
            Err((code, message)) => Response::err(Some(&request.id), code, message),
        };
        if out.send(&response).is_err() {
            return;
        }
    }
}

/// `terminal.detach`: allowed on a connection attached to that terminal.
fn detach(request: &Request, attachment: Option<&Attachment>) -> Result<(), (ErrorCode, String)> {
    let p: TerminalDetach = parsed(request)?;
    match attachment {
        Some(a) if a.terminal_id == p.terminal_id => Ok(()),
        _ => Err((
            ErrorCode::InvalidRequest,
            format!("this connection is not attached to {}", p.terminal_id),
        )),
    }
}

/// `terminal.attach`: registers the connection with the terminal's stream and starts the writer
/// thread that owns the socket's write side from now on.
fn attach(
    shared: &Shared,
    request: &Request,
    stream: &Conn,
    attachment: &mut Option<Attachment>,
) -> Result<SyncSender<Line>, (ErrorCode, String)> {
    let p: TerminalAttach = parsed(request)?;
    if attachment.is_some() {
        return Err((
            ErrorCode::InvalidRequest,
            "this connection is already attached".into(),
        ));
    }
    let term = {
        let inner = lock(&shared.inner);
        let entry = inner
            .terminals
            .iter()
            .find(|t| t.info.id == p.terminal_id)
            .ok_or_else(|| not_found("terminal", &p.terminal_id))?;
        Arc::clone(&entry.stream)
    };
    let writer_stream = stream.clone();
    let (tx, rx) = sync_channel(CLIENT_QUEUE);
    let link = Arc::new(Link {
        overflowed: AtomicBool::new(false),
    });
    let attached = term.attach(
        &request.id,
        p.mode,
        p.take_over,
        tx.clone(),
        Arc::clone(&link),
    )?;
    let terminal_id = p.terminal_id.clone();
    thread::spawn(move || write_stream(writer_stream, &rx, &link, &terminal_id));
    *attachment = Some(Attachment {
        stream: term,
        terminal_id: p.terminal_id,
        client_id: attached.client_id,
        mode: attached.mode,
    });
    Ok(tx)
}

/// The writer of an attached connection: the lines of its queue in order, then, when the stream
/// ran over, a `detached` record. A client that stops reading blocks only this thread, which
/// gives up after [`WRITE_TIMEOUT`] (Unix; named pipes have no write timeout, so on Windows it
/// waits for that client alone).
fn write_stream(mut stream: Conn, rx: &Receiver<Line>, link: &Link, terminal_id: &str) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if write_line(&mut stream, &line.text).is_err() || line.last {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if link.overflowed.load(std::sync::atomic::Ordering::SeqCst) {
            let detached = Event::TerminalDetached {
                terminal_id: terminal_id.to_string(),
                reason: DetachReason::Overflow,
            };
            let _ = send(&mut stream, &detached);
            break;
        }
    }
    // Wakes the request loop of this connection, which then detaches.
    let _ = stream.shutdown();
}

fn write_line(writer: &mut Conn, text: &str) -> io::Result<()> {
    writer.write_all(text.as_bytes())?;
    writer.flush()
}

fn send<T: serde::Serialize>(writer: &mut Conn, value: &T) -> io::Result<()> {
    write_line(writer, &line_of(value))
}

/// Creates the socket directory and opens its `server.lock`. Unix: the directory is 0700 (an
/// existing one is tightened if ours, refused otherwise) and the lock 0600.
#[cfg(unix)]
fn open_lock(dir: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    proto::check_private_dir(dir, true)?;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join("server.lock"))
}

/// Creates the socket directory and opens its `server.lock`. Windows: no modes; the pipe's DACL
/// and the per-user directory protect the server.
#[cfg(not(unix))]
fn open_lock(dir: &Path) -> io::Result<std::fs::File> {
    std::fs::create_dir_all(dir)?;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("server.lock"))
}

fn parsed<T: for<'de> serde::Deserialize<'de>>(
    request: &Request,
) -> Result<T, (ErrorCode, String)> {
    proto::params(request).map_err(|r| {
        let e = r.error.expect("params errors carry an error");
        (e.code, e.message)
    })
}

fn dispatch(shared: &Arc<Shared>, request: &Request, ctx: Option<&Attachment>) -> ApiResult {
    match request.method.as_str() {
        "ping" => Ok(json!(PingResult {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: PROTOCOL_VERSION,
            pid: std::process::id(),
        })),
        "space.list" => {
            let inner = lock(&shared.inner);
            Ok(json!(SpaceList {
                spaces: inner.layout.spaces.clone(),
                tabs: inner.layout.tabs.clone(),
            }))
        }
        "space.create" => space_create(shared, parsed(request)?),
        "space.rename" => space_rename(shared, parsed(request)?),
        "space.close" => space_close(shared, parsed(request)?),
        "terminal.list" => terminal_list(shared, parsed(request)?),
        "terminal.create" => terminal_create(shared, parsed(request)?),
        "terminal.rename" => terminal_rename(shared, parsed(request)?),
        "terminal.close" => terminal_close(shared, parsed(request)?),
        "tab.create" => tab_create(shared, parsed(request)?),
        "tab.rename" => tab_rename(shared, parsed(request)?),
        "tab.close" => tab_close(shared, parsed(request)?),
        "tab.move" => tab_move(shared, parsed(request)?),
        "tab.split" => tab_split(shared, parsed(request)?),
        "tab.resize_split" => tab_resize_split(shared, parsed(request)?),
        "tab.focus" => tab_focus(shared, parsed(request)?),
        "server.shutdown" => server_shutdown(shared),
        "server.set_colors" => server_set_colors(shared, parsed(request)?),
        "terminal.report_agent" => terminal_report_agent(shared, parsed(request)?),
        "terminal.report_tokens" => terminal_report_tokens(shared, parsed(request)?),
        "terminal.clear_tokens" => terminal_clear_tokens(shared, parsed(request)?),
        "terminal.input" => terminal_input(shared, parsed(request)?, ctx),
        "terminal.resize" => terminal_resize(shared, parsed(request)?, ctx),
        other => Err((ErrorCode::UnknownMethod, format!("unknown method {other}"))),
    }
}

/// Answers, then (after the answer had time to leave) ends every terminal, removes the socket
/// and runs the hook of [`Server::on_shutdown`]. The app asks only after the user confirmed: this
/// ends every terminal.
fn server_shutdown(shared: &Arc<Shared>) -> ApiResult {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        thread::sleep(SHUTDOWN_DELAY);
        {
            let mut inner = lock(&shared.inner);
            let entries = std::mem::take(&mut inner.terminals);
            let mut events = Vec::new();
            for entry in entries {
                let id = entry.info.id.clone();
                close_entry(&shared, entry);
                events.extend(inner.layout.remove_terminal(&id));
            }
            shared.apply(&inner, events);
        }
        // Not on Windows: a named pipe has no file, and it goes with the process.
        if cfg!(unix) {
            let _ = std::fs::remove_file(&shared.socket);
        }
        if let Some(hook) = lock(&shared.on_shutdown).take() {
            hook();
        }
    });
    Ok(json!({ "shutting_down": true }))
}

fn require_name(name: &str) -> Result<String, (ErrorCode, String)> {
    let name = name.trim();
    if name.is_empty() {
        Err((ErrorCode::InvalidParams, "name must not be empty".into()))
    } else {
        Ok(name.to_string())
    }
}

fn space_create(shared: &Shared, p: SpaceCreate) -> ApiResult {
    let name = require_name(&p.name)?;
    if p.cwd.trim().is_empty() {
        return Err((ErrorCode::InvalidParams, "cwd must not be empty".into()));
    }
    let mut inner = lock(&shared.inner);
    let space = Space {
        id: shared.new_id("s"),
        name,
        cwd: p.cwd,
        ..Default::default()
    };
    inner.layout.spaces.push(space.clone());
    shared.persist(&inner);
    shared.publish(Event::SpaceCreated {
        space: space.clone(),
    });
    Ok(json!({ "space": space }))
}

fn space_rename(shared: &Shared, p: SpaceRename) -> ApiResult {
    let name = require_name(&p.name)?;
    let mut inner = lock(&shared.inner);
    let space = inner
        .layout
        .spaces
        .iter_mut()
        .find(|s| s.id == p.space_id)
        .ok_or_else(|| not_found("space", &p.space_id))?;
    space.name = name;
    let space = space.clone();
    shared.persist(&inner);
    shared.publish(Event::SpaceRenamed {
        space: space.clone(),
    });
    Ok(json!({ "space": space }))
}

fn space_close(shared: &Shared, p: SpaceClose) -> ApiResult {
    let mut inner = lock(&shared.inner);
    inner.layout.space(&p.space_id)?;
    let (closing, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut inner.terminals)
        .into_iter()
        .partition(|t| t.info.space_id == p.space_id);
    inner.terminals = keep;
    for entry in closing {
        close_entry(shared, entry);
    }
    for event in inner.layout.remove_space(&p.space_id)? {
        shared.publish(event);
    }
    shared.persist(&inner);
    shared.publish(Event::SpaceClosed {
        space_id: p.space_id,
    });
    Ok(json!({}))
}

fn terminal_list(shared: &Shared, p: TerminalList) -> ApiResult {
    shared.expire_tokens();
    let inner = lock(&shared.inner);
    let terminals: Vec<&Terminal> = inner
        .terminals
        .iter()
        .map(|t| &t.info)
        .filter(|t| p.space_id.as_ref().is_none_or(|id| &t.space_id == id))
        .collect();
    Ok(json!({ "terminals": terminals }))
}

/// `#rrggbb` (the `#` optional).
fn parse_hex(value: &str) -> Result<Rgb, (ErrorCode, String)> {
    let bad = || {
        (
            ErrorCode::InvalidParams,
            format!("{value:?} is not a #rrggbb color"),
        )
    };
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.is_ascii() {
        return Err(bad());
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
    Ok(Rgb {
        r: byte(0)?,
        g: byte(2)?,
        b: byte(4)?,
    })
}

fn server_set_colors(shared: &Shared, p: SetColors) -> ApiResult {
    let mut colors = Colors {
        foreground: parse_hex(&p.foreground)?,
        background: parse_hex(&p.background)?,
        ..Colors::default()
    };
    colors.cursor = match &p.cursor {
        Some(c) => parse_hex(c)?,
        None => colors.foreground,
    };
    let mut current = shared.colors.write().unwrap_or_else(|e| e.into_inner());
    colors.ansi = current.ansi;
    if let Some(palette) = &p.palette {
        if palette.len() != 16 {
            return Err((
                ErrorCode::InvalidParams,
                "palette needs exactly 16 colors".into(),
            ));
        }
        for (slot, c) in colors.ansi.iter_mut().zip(palette) {
            *slot = parse_hex(c)?;
        }
    }
    *current = colors;
    Ok(json!({}))
}

/// `terminal.create`: the terminal gets a tab of its own at the end of its space (named like
/// the terminal), so callers that know nothing of tabs keep working.
fn terminal_create(shared: &Arc<Shared>, p: TerminalCreate) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let terminal = spawn_terminal(shared, &mut inner, p)?;
    let events = inner.layout.add_tab(
        &terminal.space_id,
        shared.new_id("tab"),
        terminal.name.clone(),
        &terminal.id,
    )?;
    shared.apply(&inner, events);
    Ok(json!({ "terminal": terminal }))
}

/// Starts a terminal in its space and publishes `terminal.created`; the caller puts it into a
/// leaf under the same lock.
fn spawn_terminal(
    shared: &Arc<Shared>,
    inner: &mut Inner,
    p: TerminalCreate,
) -> Result<Terminal, (ErrorCode, String)> {
    let space = inner.layout.space(&p.space_id)?;
    let cwd = p.cwd.clone().unwrap_or_else(|| space.cwd.clone());
    let command = match p.command {
        Some(c) if c.is_empty() || c[0].is_empty() => {
            return Err((ErrorCode::InvalidParams, "command must not be empty".into()));
        }
        Some(c) => c,
        None => vec![pty::default_shell()],
    };
    let program = command[0].clone();
    let id = shared.new_id("t");
    let (cols, rows) = (p.cols.unwrap_or(80).max(1), p.rows.unwrap_or(24).max(1));
    let name = p
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| pty::program_name(&command[0]));
    let stream = Arc::new(TermStream::new(
        &id,
        cols,
        rows,
        shared.scrollback,
        Arc::clone(&shared.colors),
        {
            let weak = Arc::downgrade(shared);
            Arc::new(move |event| {
                if let Some(shared) = weak.upgrade() {
                    shared.publish(event);
                }
            })
        },
    ));
    let spawned = pty::spawn(
        &pty::SpawnSpec {
            terminal_id: &id,
            socket: &shared.socket,
            command: &command,
            cwd: &cwd,
            cols,
            rows,
        },
        {
            let stream = Arc::clone(&stream);
            move |bytes| stream.output(bytes)
        },
        {
            // The wait thread reports the exit through the shared state.
            let id = id.clone();
            let shared = Arc::clone(shared);
            move |code| shared.terminal_exited(&id, code)
        },
    )
    .map_err(|e| (ErrorCode::SpawnFailed, e))?;
    stream.set_writer(spawned.writer());
    let info = Terminal {
        id,
        space_id: p.space_id,
        name,
        cwd,
        cols,
        rows,
        status: TerminalStatus::Running,
        exit_code: None,
        program: Some(program),
        agent: None,
        tokens: Default::default(),
    };
    inner.terminals.push(TerminalEntry {
        info: info.clone(),
        pty: Some(spawned),
        stream,
        agent_seq: None,
        tokens: Vec::new(),
    });
    shared.publish(Event::TerminalCreated {
        terminal: info.clone(),
    });
    Ok(info)
}

fn terminal_mut<'a>(
    inner: &'a mut Inner,
    id: &str,
) -> Result<&'a mut TerminalEntry, (ErrorCode, String)> {
    inner
        .terminals
        .iter_mut()
        .find(|t| t.info.id == id)
        .ok_or_else(|| not_found("terminal", id))
}

fn terminal_rename(shared: &Shared, p: TerminalRename) -> ApiResult {
    let name = require_name(&p.name)?;
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    entry.info.name = name;
    let terminal = entry.info.clone();
    shared.publish(Event::TerminalRenamed {
        terminal: terminal.clone(),
    });
    Ok(json!({ "terminal": terminal }))
}

/// Applies an agent report unless it is stale (`seq` not above the last accepted one). The event
/// is published only when the agent state actually changed.
fn terminal_report_agent(shared: &Shared, p: TerminalReportAgent) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    if entry.agent_seq.is_some_and(|last| p.seq <= last) {
        return Ok(json!({ "accepted": false }));
    }
    entry.agent_seq = Some(p.seq);
    if entry.info.status == TerminalStatus::Exited {
        return Ok(json!({ "accepted": true }));
    }
    // The latest notification text: a new one replaces it, `working` clears it, else it stays.
    let (message, message_at) = match p
        .message
        .as_deref()
        .and_then(poimen_protocol::clean_message)
    {
        Some(text) => (Some(text), Some(now_millis())),
        None if p.status == AgentStatus::Working => (None, None),
        None => entry
            .info
            .agent
            .as_ref()
            .map_or((None, None), |a| (a.message.clone(), a.message_at)),
    };
    let next = (p.status != AgentStatus::Gone).then(|| AgentInfo {
        status: p.status,
        agent: p.agent.clone(),
        session_id: p.session_id.clone(),
        message: message.clone(),
        message_at,
    });
    if entry.info.agent != next {
        entry.info.agent = next;
        shared.publish(Event::AgentStatusChanged {
            terminal_id: p.terminal_id,
            status: p.status,
            agent: p.agent,
            session_id: p.session_id,
            message,
            message_at,
        });
    }
    Ok(json!({ "accepted": true }))
}

fn invalid(message: impl Into<String>) -> (ErrorCode, String) {
    (ErrorCode::InvalidParams, message.into())
}

fn tokens_changed(shared: &Shared, entry: &TerminalEntry) {
    shared.publish(Event::TerminalTokensChanged {
        terminal_id: entry.info.id.clone(),
        tokens: entry.info.tokens.clone(),
    });
}

/// Sets and removes the tokens of one source (herdr's `pane report-metadata` semantics: names of
/// [`proto::valid_token_name`], at most [`proto::MAX_TOKENS_PER_REPORT`] per report, a TTL per
/// report). The event is published only when the visible tokens changed.
fn terminal_report_tokens(shared: &Shared, p: TerminalReportTokens) -> ApiResult {
    if p.source.trim().is_empty() {
        return Err(invalid("source must not be empty"));
    }
    if p.tokens.len() > proto::MAX_TOKENS_PER_REPORT {
        return Err(invalid(format!(
            "at most {} tokens per report",
            proto::MAX_TOKENS_PER_REPORT
        )));
    }
    if let Some(name) = p.tokens.keys().find(|n| !proto::valid_token_name(n)) {
        return Err(invalid(format!(
            "invalid token name {name:?}: 1 to {} ASCII letters, digits, _ or -",
            proto::MAX_TOKEN_NAME
        )));
    }
    if p.ttl_s == Some(0) {
        return Err(invalid("ttl_s must be above 0"));
    }
    let now = Instant::now();
    let expires = p.ttl_s.map(|s| now + Duration::from_secs(s));
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    if entry.info.status == TerminalStatus::Exited {
        return Ok(json!({ "accepted": true }));
    }
    let before = entry.info.tokens.clone();
    entry.expire_tokens(now);
    for (name, value) in p.tokens {
        entry.tokens.retain(|t| t.name != name);
        match value {
            Some(value) => {
                entry.info.tokens.insert(name.clone(), value);
                entry.tokens.push(TokenEntry {
                    name,
                    source: p.source.clone(),
                    expires,
                });
            }
            None => {
                entry.info.tokens.remove(&name);
            }
        }
    }
    if entry.info.tokens != before {
        tokens_changed(shared, entry);
    }
    Ok(json!({ "accepted": true }))
}

/// Removes every token the source set on the terminal.
fn terminal_clear_tokens(shared: &Shared, p: TerminalClearTokens) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    let info = &mut entry.info;
    let before = entry.tokens.len();
    entry.tokens.retain(|t| {
        let keep = t.source != p.source;
        if !keep {
            info.tokens.remove(&t.name);
        }
        keep
    });
    if entry.tokens.len() != before {
        tokens_changed(shared, entry);
    }
    Ok(json!({ "cleared": before - entry.tokens.len() }))
}

fn terminal_close(shared: &Shared, p: TerminalClose) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let index = inner
        .terminals
        .iter()
        .position(|t| t.info.id == p.terminal_id)
        .ok_or_else(|| not_found("terminal", &p.terminal_id))?;
    let entry = inner.terminals.remove(index);
    close_entry(shared, entry);
    let events = inner.layout.remove_terminal(&p.terminal_id);
    shared.apply(&inner, events);
    Ok(json!({}))
}

/// Ends a terminal taken out of the list: its attachments, its process, `terminal.closed`. The
/// caller removes its leaf.
fn close_entry(shared: &Shared, entry: TerminalEntry) {
    entry.stream.closed();
    if let Some(pty) = entry.pty {
        pty.kill();
    }
    shared.publish(Event::TerminalClosed {
        terminal_id: entry.info.id,
        space_id: entry.info.space_id,
    });
}

/// The running terminal `terminal_id`: `not_found`, or `exited` (an exited terminal has no leaf).
fn running_terminal<'a>(
    inner: &'a Inner,
    terminal_id: &str,
) -> Result<&'a Terminal, (ErrorCode, String)> {
    let entry = inner
        .terminals
        .iter()
        .find(|t| t.info.id == terminal_id)
        .ok_or_else(|| not_found("terminal", terminal_id))?;
    if entry.info.status == TerminalStatus::Exited {
        return Err((
            ErrorCode::Exited,
            format!("terminal {terminal_id} has exited"),
        ));
    }
    Ok(&entry.info)
}

fn tab_json(inner: &Inner, tab_id: &str) -> Value {
    json!(inner.layout.tab(tab_id).ok())
}

/// `tab.create`: a new tab at the end of the space, with the given terminal (moved out of its
/// tab) or a new one.
fn tab_create(shared: &Arc<Shared>, p: TabCreate) -> ApiResult {
    let name = p.name.as_deref().map(require_name).transpose()?;
    let mut inner = lock(&shared.inner);
    inner.layout.space(&p.space_id)?;
    let mut events = Vec::new();
    let terminal = match &p.terminal_id {
        Some(id) => {
            let terminal = running_terminal(&inner, id)?.clone();
            if terminal.space_id != p.space_id {
                return Err(invalid(format!(
                    "terminal {id} belongs to space {}",
                    terminal.space_id
                )));
            }
            events.extend(inner.layout.remove_terminal(id));
            terminal
        }
        None => spawn_terminal(
            shared,
            &mut inner,
            TerminalCreate {
                space_id: p.space_id.clone(),
                ..Default::default()
            },
        )?,
    };
    let tab_id = shared.new_id("tab");
    let name = name.unwrap_or_else(|| terminal.name.clone());
    events.extend(
        inner
            .layout
            .add_tab(&p.space_id, tab_id.clone(), name, &terminal.id)?,
    );
    shared.apply(&inner, events);
    Ok(json!({ "tab": tab_json(&inner, &tab_id), "terminal": terminal }))
}

fn tab_rename(shared: &Shared, p: TabRename) -> ApiResult {
    let name = require_name(&p.name)?;
    let mut inner = lock(&shared.inner);
    let events = inner.layout.rename(&p.tab_id, name)?;
    shared.apply(&inner, events);
    Ok(json!({ "tab": tab_json(&inner, &p.tab_id) }))
}

/// `tab.close`: closes the terminals of the tab (`terminal.closed` each), then the tab.
fn tab_close(shared: &Shared, p: TabClose) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let terminals: Vec<String> = inner
        .layout
        .tab(&p.tab_id)?
        .root
        .terminal_ids()
        .into_iter()
        .map(str::to_string)
        .collect();
    let (closing, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut inner.terminals)
        .into_iter()
        .partition(|t| terminals.contains(&t.info.id));
    inner.terminals = keep;
    for entry in closing {
        close_entry(shared, entry);
    }
    let (_, events) = inner.layout.remove_tab(&p.tab_id)?;
    shared.apply(&inner, events);
    Ok(json!({}))
}

fn tab_move(shared: &Shared, p: TabMove) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let events = inner.layout.move_tab(&p.tab_id, p.index)?;
    shared.apply(&inner, events);
    let tab = inner.layout.tab(&p.tab_id)?;
    Ok(json!({ "tab": tab, "space": inner.layout.space(&tab.space_id)? }))
}

/// `tab.split`: a new terminal (cwd of the split one by default) as the second child of a split
/// that takes the place of the leaf.
fn tab_split(shared: &Arc<Shared>, p: TabSplit) -> ApiResult {
    let ratio = p.ratio.unwrap_or(proto::DEFAULT_SPLIT_RATIO);
    if !proto::valid_ratio(ratio) {
        return Err(invalid(format!(
            "ratio {ratio} must be above 0 and below 1"
        )));
    }
    let mut inner = lock(&shared.inner);
    let target = running_terminal(&inner, &p.terminal_id)?.clone();
    let tab_id = inner
        .layout
        .tab_of(&p.terminal_id)
        .map(|t| t.tab_id.clone())
        .ok_or_else(|| {
            (
                ErrorCode::Internal,
                format!("terminal {} has no leaf", p.terminal_id),
            )
        })?;
    let terminal = spawn_terminal(
        shared,
        &mut inner,
        TerminalCreate {
            space_id: target.space_id.clone(),
            cwd: Some(p.cwd.unwrap_or(target.cwd)),
            name: p.name,
            command: p.command,
            cols: None,
            rows: None,
        },
    )?;
    let split_id = shared.new_id("split");
    let events = inner
        .layout
        .split(&p.terminal_id, &terminal.id, p.direction, ratio, split_id)?;
    shared.apply(&inner, events);
    Ok(json!({ "tab": tab_json(&inner, &tab_id), "terminal": terminal }))
}

fn tab_resize_split(shared: &Shared, p: TabResizeSplit) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let events = inner.layout.resize_split(&p.tab_id, &p.split_id, p.ratio)?;
    shared.apply(&inner, events);
    Ok(json!({ "tab": tab_json(&inner, &p.tab_id) }))
}

/// `tab.focus`: shared by every client, the last one wins.
fn tab_focus(shared: &Shared, p: TabFocus) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let events = inner.layout.focus(&p.tab_id, p.terminal_id.as_deref())?;
    shared.apply(&inner, events);
    let tab = inner.layout.tab(&p.tab_id)?;
    Ok(json!({ "tab": tab, "space": inner.layout.space(&tab.space_id)? }))
}

fn running_pty<'a>(
    entry: &'a mut TerminalEntry,
    id: &str,
) -> Result<&'a mut pty::Pty, (ErrorCode, String)> {
    entry
        .pty
        .as_mut()
        .ok_or_else(|| (ErrorCode::Exited, format!("terminal {id} has exited")))
}

/// Input and resize belong to the controller. A connection attached to this terminal must be
/// its controller; a connection that is not attached may send input, and may resize only while
/// nobody controls.
fn authorize(
    stream: &TermStream,
    ctx: Option<&Attachment>,
    terminal_id: &str,
    resize: bool,
) -> Result<(), (ErrorCode, String)> {
    let denied = |why: &str| Err((ErrorCode::NotController, why.to_string()));
    match ctx.filter(|a| a.terminal_id == terminal_id) {
        Some(a) if a.mode == AttachMode::Observe => {
            denied("an observer can neither send input nor resize")
        }
        Some(a) if !stream.is_controller(a.client_id) => denied("the control was taken over"),
        Some(_) => Ok(()),
        None if resize && stream.has_controller() => {
            denied("only the controller resizes a terminal that has one")
        }
        None => Ok(()),
    }
}

fn terminal_input(shared: &Shared, p: TerminalInput, ctx: Option<&Attachment>) -> ApiResult {
    // The write may block on a full PTY; do it outside the global lock.
    let writer = {
        let mut inner = lock(&shared.inner);
        let entry = terminal_mut(&mut inner, &p.terminal_id)?;
        authorize(&entry.stream, ctx, &p.terminal_id, false)?;
        running_pty(entry, &p.terminal_id)?.writer()
    };
    writer
        .write(p.data.as_bytes())
        .map_err(|e| (ErrorCode::Internal, e))?;
    Ok(json!({}))
}

fn terminal_resize(shared: &Shared, p: TerminalResize, ctx: Option<&Attachment>) -> ApiResult {
    if p.cols == 0 || p.rows == 0 {
        return Err((
            ErrorCode::InvalidParams,
            "cols and rows must be at least 1".into(),
        ));
    }
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    authorize(&entry.stream, ctx, &p.terminal_id, true)?;
    running_pty(entry, &p.terminal_id)?
        .resize(p.cols, p.rows)
        .map_err(|e| (ErrorCode::Internal, e))?;
    entry.info.cols = p.cols;
    entry.info.rows = p.rows;
    entry.stream.resized(p.cols, p.rows);
    Ok(json!({}))
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
