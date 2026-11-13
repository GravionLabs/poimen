//! `poimen-server`: a headless daemon that runs the terminals, so shells and agents keep running
//! when the app closes. Each terminal's PTY and child live in a `poimen-pty-host` of its own,
//! which the server reads as a client (`host.rs`, ADR 0009). One server per user listens on a local socket (a Unix socket, a named pipe on
//! Windows; `docs/protocol.md`).
//!
//! The server keeps spaces (name, cwd) with terminals, and per space the tabs in order with a
//! split tree of terminals each (`tabs.rs`). Spaces, tabs and the list of running terminals
//! persist in `server.json` (`store.rs`); a new server adopts the hosts of that list (#604) and
//! drops the leaves whose terminal did not come back.

pub mod check;
pub mod claude;
pub mod copilot;
mod create;
mod emulator;
pub mod hook;
mod host;
mod pty;
pub mod report;
mod runs;
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
use poimen_core::{repos, worktree};
use poimen_protocol::{
    self as proto, AgentInfo, AgentStatus, AttachMode, ClientKind, DetachReason, ErrorCode, Event,
    EventsSubscribe, PROTOCOL_VERSION, PingResult, RepoEnsure, RepoEnsureResult, Request, Response,
    ServerShutdown, SetColors, Space, SpaceApplyLayout, SpaceApplyLayoutResult, SpaceClose,
    SpaceCreate, SpaceCreateResult, SpaceCreated, SpaceList, SpaceRename, TabClose, TabCreate,
    TabFocus, TabMove, TabRename, TabResizeSplit, TabSplit, Terminal, TerminalAttach,
    TerminalClearTokens, TerminalClose, TerminalCreate, TerminalDetach, TerminalInput,
    TerminalList, TerminalRename, TerminalReportAgent, TerminalReportTokens, TerminalResize,
    TerminalStatus, WorktreeCreate, WorktreeCreateResult,
};
use serde_json::{Value, json};

pub use emulator::DEFAULT_SCROLLBACK;
pub use host::{Launch, PtyHost, host_beside};
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
    /// The user's home, whose `~/.config/poimen/config.json` gives `trustedProjects`,
    /// `spaceTemplates`, `spaces.layout` and `harnesses` to `space.create` (which also writes
    /// `trustedProjects`); `None`: no user config.
    pub home: Option<PathBuf>,
    /// How a new terminal's `poimen-pty-host` is started (ADR 0009): the binary next to the
    /// server, or an in-process host in tests.
    pub pty_host: PtyHost,
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
            home: std::env::home_dir().filter(|h| !h.as_os_str().is_empty()),
            pty_host: PtyHost::beside_server(),
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
    /// The terminal's host (endpoint, last `seq` read, connection); `None` once it has exited.
    host: Option<host::Handle>,
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
    /// The entry as the terminal list keeps it; `None` once the terminal has exited (no host).
    fn stored(&self, now: Instant, now_ms: u64) -> Option<store::StoredTerminal> {
        let host = self.host.as_ref()?;
        if self.info.status == TerminalStatus::Exited {
            return None;
        }
        let tokens = self
            .tokens
            .iter()
            .filter_map(|t| {
                Some(store::StoredToken {
                    name: t.name.clone(),
                    value: self.info.tokens.get(&t.name)?.clone(),
                    source: t.source.clone(),
                    expires_at_ms: t.expires.map(|at| {
                        let left = u64::try_from(at.saturating_duration_since(now).as_millis())
                            .unwrap_or(u64::MAX);
                        now_ms.saturating_add(left)
                    }),
                })
            })
            .collect();
        Some(store::StoredTerminal {
            id: self.info.id.clone(),
            space_id: self.info.space_id.clone(),
            name: self.info.name.clone(),
            cwd: self.info.cwd.clone(),
            program: self.info.program.clone(),
            args: self.info.args.clone(),
            cols: self.info.cols,
            rows: self.info.rows,
            endpoint: host.endpoint().to_path_buf(),
            seq: host.seq(),
            agent: self.info.agent.clone(),
            agent_seq: self.agent_seq,
            tokens,
        })
    }

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

static NEXT_SUBSCRIBER: AtomicU64 = AtomicU64::new(1);

/// An open event subscription and what its client said it is.
struct Subscriber {
    id: u64,
    kind: ClientKind,
    /// The client said what it is. Only these get `server.clients_changed`: an older client
    /// cannot parse the event and would resync in a loop.
    declared: bool,
    /// It asked for `run_events` ([`Event::RunEvents`], #692).
    runs: bool,
    tx: Sender<Event>,
}

/// Sends the event to the subscribers that declared a client, dropping the ones that went away.
fn notify_declared(subs: &mut Vec<Subscriber>, event: &Event) {
    subs.retain(|s| !s.declared || s.tx.send(event.clone()).is_ok());
}

pub(crate) struct Shared {
    inner: Mutex<Inner>,
    subscribers: Mutex<Vec<Subscriber>>,
    /// The run files followed for the subscriptions with `run_events`. Taken before
    /// `subscribers`, so a new subscription gets the replay and then every later event once.
    runs: Mutex<runs::Runs>,
    /// The repositories of the space and terminal directories, for `runs`.
    run_repos: Mutex<runs::Repos>,
    next_id: AtomicU64,
    socket: PathBuf,
    state_file: PathBuf,
    scrollback: usize,
    home: Option<PathBuf>,
    pty_host: PtyHost,
    /// What `repo.ensure` clones with: the real `gh`, or a fake set by tests ([`Server::set_gh`]).
    gh: RwLock<Arc<dyn repos::Gh>>,
    /// The scheme colors for terminal queries, shared with every terminal's stream.
    colors: Arc<RwLock<Colors>>,
    /// Run once the terminals are ended after `server.shutdown`; the binary exits the process.
    on_shutdown: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Set by `server.shutdown` with `keep_terminals` once the terminal list is written: this
    /// server let its hosts go to the next one and never writes `server.json` again.
    stopped: AtomicBool,
    /// The lock next to the socket, held until `server.shutdown` is done: whoever holds it is the
    /// only one to judge the socket entry stale.
    server_lock: Mutex<Option<std::fs::File>>,
}

impl Shared {
    /// Sends the event to every subscriber, dropping the ones that went away. Called with the
    /// `inner` lock held, so events arrive in the order the changes happened.
    fn publish(&self, event: Event) {
        lock(&self.subscribers).retain(|s| s.tx.send(event.clone()).is_ok());
    }

    /// The kinds of the subscribed clients, sorted.
    fn client_kinds(&self) -> Vec<ClientKind> {
        let mut kinds: Vec<_> = lock(&self.subscribers).iter().map(|s| s.kind).collect();
        kinds.sort();
        kinds
    }

    /// Registers a subscription and tells the others; the new one learns from `ping`. One that
    /// asks for `runs` first gets the events of the followed runs from the start.
    fn add_subscriber(
        &self,
        kind: ClientKind,
        declared: bool,
        runs: bool,
        tx: Sender<Event>,
    ) -> u64 {
        let id = NEXT_SUBSCRIBER.fetch_add(1, Ordering::Relaxed);
        // Held until the subscription is listed, so no run event falls between the two.
        let followed = runs.then(|| lock(&self.runs));
        if let Some(followed) = &followed {
            for event in followed.replay() {
                let _ = tx.send(event);
            }
        }
        let mut subs = lock(&self.subscribers);
        let event = Event::ServerClientsChanged {
            clients: {
                let mut k: Vec<_> = subs.iter().map(|s| s.kind).collect();
                k.push(kind);
                k.sort();
                k
            },
        };
        notify_declared(&mut subs, &event);
        subs.push(Subscriber {
            id,
            kind,
            declared,
            runs,
            tx,
        });
        id
    }

    /// Reads what the run files of the spaces' repositories gained and sends it to the
    /// subscriptions with `run_events`; without one, the followed files are forgotten (#692).
    fn follow_runs(&self) {
        let wanted = || lock(&self.subscribers).iter().any(|s| s.runs);
        {
            // Under the lock, so a subscription that comes meanwhile is never cleared away.
            let mut followed = lock(&self.runs);
            if !wanted() {
                followed.clear();
                return;
            }
        }
        let dirs: Vec<String> = {
            let inner = lock(&self.inner);
            let mut dirs: Vec<String> = inner.layout.spaces.iter().map(|s| s.cwd.clone()).collect();
            for t in &inner.terminals {
                if t.info.status == TerminalStatus::Running && !dirs.contains(&t.info.cwd) {
                    dirs.push(t.info.cwd.clone());
                }
            }
            dirs
        };
        let repos = lock(&self.run_repos).resolve(&dirs, Instant::now(), poimen_core::git::roots);
        let mut followed = lock(&self.runs);
        // Checked again under the lock: a subscription that came meanwhile got the replay of what
        // is kept, so it is only dropped while there is none.
        if !wanted() {
            followed.clear();
            return;
        }
        let events = followed.scan(&repos, std::time::SystemTime::now());
        let mut subs = lock(&self.subscribers);
        for event in events {
            subs.retain(|s| !s.runs || s.tx.send(event.clone()).is_ok());
        }
    }

    /// Drops a subscription (ending its event loop) and tells the rest.
    fn remove_subscriber(&self, id: u64) {
        let mut subs = lock(&self.subscribers);
        let before = subs.len();
        subs.retain(|s| s.id != id);
        if subs.len() == before {
            return;
        }
        let mut clients: Vec<_> = subs.iter().map(|s| s.kind).collect();
        clients.sort();
        let event = Event::ServerClientsChanged { clients };
        notify_declared(&mut subs, &event);
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

    /// Writes spaces, tabs, the id counter and the running terminals. Called on every change of
    /// them (the `seq` read from each host goes along, it is not a change of its own).
    fn persist(&self, inner: &Inner) {
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }
        let next_id = self.next_id.load(Ordering::Relaxed);
        let (now, now_ms) = (Instant::now(), now_millis());
        let terminals = inner
            .terminals
            .iter()
            .filter_map(|t| t.stored(now, now_ms))
            .collect();
        if let Err(e) = store::save(&self.state_file, &inner.layout, next_id, terminals) {
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
        let mut changed = false;
        for entry in &mut inner.terminals {
            if entry.expire_tokens(now) {
                changed = true;
                self.publish(Event::TerminalTokensChanged {
                    terminal_id: entry.info.id.clone(),
                    tokens: entry.info.tokens.clone(),
                });
            }
        }
        if changed {
            self.persist(&inner);
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
        // Nothing of it is left for a restart to keep.
        entry.info.survives_restart = false;
        entry.info.host_version = None;
        entry.info.host_bundled_version = None;
        entry.host = None;
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
        // An exited terminal is no longer in the tree, nor in the terminal list.
        let events = inner.layout.remove_terminal(terminal_id);
        publish_all(self, events);
        self.persist(&inner);
    }
}

type ApiResult = Result<Value, (ErrorCode, String)>;

fn not_found(what: &str, id: &str) -> (ErrorCode, String) {
    (ErrorCode::NotFound, format!("no {what} {id}"))
}

/// A bound server, ready to [`serve`](Server::serve).
pub struct Server {
    listener: Listener,
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
            // Held until `server.shutdown` is done: whoever holds it is the only one to judge the
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
            layout,
            next_id,
            terminals,
        } = store::load(&config.state_file);
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                layout,
                terminals: Vec::new(),
            }),
            subscribers: Mutex::new(Vec::new()),
            runs: Mutex::default(),
            run_repos: Mutex::default(),
            next_id: AtomicU64::new(next_id),
            socket: config.socket,
            state_file: config.state_file,
            scrollback: config.scrollback,
            home: config.home,
            pty_host: config.pty_host,
            gh: RwLock::new(Arc::new(repos::SystemGh)),
            colors: Arc::default(),
            on_shutdown: Mutex::new(None),
            stopped: AtomicBool::new(false),
            server_lock: Mutex::new(lock_file),
        });
        {
            // Under the lock, so an adopted host whose child has ended meanwhile reports its exit
            // only once its terminal is in the list.
            let mut guard = lock(&shared.inner);
            let inner = &mut *guard;
            adopt_terminals(&shared, inner, terminals);
            // The leaves of terminals that did not come back, the splits they leave behind and
            // the empty tabs go (#510).
            let live: Vec<tabs::Live<'_>> = inner
                .terminals
                .iter()
                .map(|t| tabs::Live {
                    id: &t.info.id,
                    space_id: &t.info.space_id,
                    name: &t.info.name,
                })
                .collect();
            inner
                .layout
                .reconcile(&live, &mut |prefix| shared.new_id(prefix));
            shared.persist(inner);
        }
        Ok(Self { listener, shared })
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

    /// Replaces the `gh` that `repo.ensure` clones with (tests map `gh repo clone` onto a local
    /// origin, so they need no network).
    pub fn set_gh(&self, gh: Arc<dyn repos::Gh>) {
        *self.shared.gh.write().unwrap_or_else(|e| e.into_inner()) = gh;
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
        let runs = Arc::clone(&self.shared);
        thread::spawn(move || {
            loop {
                thread::sleep(runs::POLL);
                runs.follow_runs();
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
            let info: EventsSubscribe = match parsed(&request) {
                Ok(p) => p,
                Err((code, message)) => {
                    let response = Response::err(Some(&request.id), code, message);
                    if out.send(&response).is_err() {
                        return;
                    }
                    continue;
                }
            };
            let declared = info.client.is_some();
            let kind = info.client.map_or(ClientKind::Unknown, |c| c.kind);
            let (tx, rx) = mpsc::channel();
            let id = shared.add_subscriber(kind, declared, info.run_events, tx);
            // The client sends nothing more; the end of its side of the connection removes
            // the subscription, which also ends the loop below.
            let reader = stream.clone();
            let watcher = Arc::clone(shared);
            thread::spawn(move || {
                let mut sink = [0u8; 256];
                let mut reader = reader;
                while matches!(std::io::Read::read(&mut reader, &mut sink), Ok(n) if n > 0) {}
                watcher.remove_subscriber(id);
            });
            let ok = Response::ok(&request.id, json!({ "subscribed": true }));
            if out.send(&ok).is_err() {
                return;
            }
            // From here the connection only carries events.
            let mut writer = stream;
            for event in rx {
                if send(&mut writer, &event).is_err() {
                    break;
                }
            }
            shared.remove_subscriber(id);
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
    let link = Arc::new(Link::new());
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
        // The server let the terminal go to the next one: the connection just ends.
        if link.released.load(std::sync::atomic::Ordering::SeqCst) {
            break;
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
            clients: shared.client_kinds(),
        })),
        "machine.check" => Ok(json!(check::run(
            &check::System,
            env!("CARGO_PKG_VERSION"),
            PROTOCOL_VERSION,
            proto::CHANNEL,
        ))),
        "repo.ensure" => repo_ensure(shared, parsed(request)?),
        "worktree.create" => worktree_create(shared, parsed(request)?),
        "space.list" => {
            let inner = lock(&shared.inner);
            Ok(json!(SpaceList {
                spaces: inner.layout.spaces.clone(),
                tabs: inner.layout.tabs.clone(),
            }))
        }
        "space.create" => space_create(shared, parsed(request)?),
        "space.apply_layout" => space_apply_layout(shared, parsed(request)?),
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
        "server.shutdown" => server_shutdown(shared, parsed(request)?),
        "server.set_colors" => server_set_colors(shared, parsed(request)?),
        "terminal.report_agent" => terminal_report_agent(shared, parsed(request)?),
        "terminal.report_tokens" => terminal_report_tokens(shared, parsed(request)?),
        "terminal.clear_tokens" => terminal_clear_tokens(shared, parsed(request)?),
        "terminal.input" => terminal_input(shared, parsed(request)?, ctx),
        "terminal.resize" => terminal_resize(shared, parsed(request)?, ctx),
        other => Err((ErrorCode::UnknownMethod, format!("unknown method {other}"))),
    }
}

/// Answers, then (after the answer had time to leave) ends every terminal, or with
/// `keep_terminals` writes the terminal list and lets the hosts run on for the next server to
/// adopt (ADR 0009); then removes the socket and runs the hook of [`Server::on_shutdown`]. The
/// app asks only after the user confirmed.
fn server_shutdown(shared: &Arc<Shared>, p: ServerShutdown) -> ApiResult {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        thread::sleep(SHUTDOWN_DELAY);
        {
            let mut inner = lock(&shared.inner);
            if p.keep_terminals {
                // The list with the last `seq` read from every host, then no write any more: a
                // host this server lets go is never reported as an exit here.
                shared.persist(&inner);
                shared.stopped.store(true, Ordering::SeqCst);
                for entry in std::mem::take(&mut inner.terminals) {
                    entry.stream.release();
                    if let Some(host) = entry.host {
                        host.release();
                    }
                }
            } else {
                let entries = std::mem::take(&mut inner.terminals);
                let mut events = Vec::new();
                for entry in entries {
                    let id = entry.info.id.clone();
                    close_entry(&shared, entry);
                    events.extend(inner.layout.remove_terminal(&id));
                }
                shared.apply(&inner, events);
            }
        }
        // Not on Windows: a named pipe has no file, and it goes with the process.
        if cfg!(unix) {
            let _ = std::fs::remove_file(&shared.socket);
        }
        // The next server may start (in the binary the process ends right after).
        lock(&shared.server_lock).take();
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

/// Whether a program is on the `PATH` of the server's children, for a template's agent.
fn program_available() -> impl Fn(&str) -> bool {
    let path = std::sync::OnceLock::new();
    move |program: &str| {
        let path = path.get_or_init(poimen_core::gui_path::child_path);
        poimen_core::gui_path::resolve_program(program, path).is_some()
    }
}

/// `space.create`: an empty space (`layout: false`), or the space with the resolved layout built
/// in it, or the trust question with nothing created (see `create.rs`).
fn space_create(shared: &Arc<Shared>, p: SpaceCreate) -> ApiResult {
    let name = require_name(&p.name)?;
    let cwd = p.cwd.trim();
    if cwd.is_empty() {
        return Err((ErrorCode::InvalidParams, "cwd must not be empty".into()));
    }
    if !p.layout {
        let mut inner = lock(&shared.inner);
        let space = Space {
            id: shared.new_id("s"),
            name,
            cwd: p.cwd.clone(),
            ..Default::default()
        };
        inner.layout.spaces.push(space.clone());
        shared.persist(&inner);
        shared.publish(Event::SpaceCreated {
            space: space.clone(),
        });
        return Ok(json!({ "space": space }));
    }
    let build = match create::prepare(
        shared.home.as_deref(),
        cwd,
        p.trust.as_ref(),
        create::Wanted::Resolved {
            root_command: p.command.as_deref(),
        },
        &program_available(),
    )? {
        create::Prepared::Ask(trust_required) => {
            return Ok(json!(SpaceCreateResult::TrustRequired { trust_required }));
        }
        create::Prepared::Build(build) => build,
    };
    let (mut created, typing) = build_space(shared, name, build)?;
    type_commands(typing, &mut created);
    Ok(json!(SpaceCreateResult::Created(created)))
}

/// `repo.ensure` (#282): the clone of `owner/repo` under `reposDir` of the user config, cloned
/// with `gh repo clone` when absent and fetched when present; anything else at its path is
/// `conflict` and stays untouched. Runs on the connection's thread, without the server lock.
fn repo_ensure(shared: &Arc<Shared>, p: RepoEnsure) -> ApiResult {
    let home = shared.home.as_deref();
    let template = repos::user_repos_dir(home);
    let gh = Arc::clone(&shared.gh.read().unwrap_or_else(|e| e.into_inner()));
    match repos::ensure(gh.as_ref(), home, &template, p.owner.trim(), p.repo.trim()) {
        Ok(done) => Ok(json!(RepoEnsureResult {
            path: done.path,
            cloned: done.cloned,
        })),
        Err(repos::EnsureError::Invalid(m)) => Err((ErrorCode::InvalidParams, m)),
        Err(repos::EnsureError::Occupied(m)) => Err((ErrorCode::Conflict, m)),
        Err(repos::EnsureError::Failed(m)) => Err((ErrorCode::Internal, m)),
    }
}

/// `worktree.create` (#282): `task_worktree` of #190 through `poimen_core::worktree`, with the
/// task modes of the user config and the trusted project file.
fn worktree_create(shared: &Arc<Shared>, p: WorktreeCreate) -> ApiResult {
    if !Path::new(&p.repo_dir).is_dir() {
        return Err((
            ErrorCode::InvalidParams,
            format!("{} is not a directory", p.repo_dir),
        ));
    }
    let issue = worktree::Issue {
        nr: p.issue.nr,
        title: p.issue.title,
        url: p.issue.url,
    };
    match worktree::task_worktree(shared.home.as_deref(), &p.repo_dir, &issue, &p.mode) {
        Ok(wt) => Ok(json!(WorktreeCreateResult {
            path: wt.path,
            branch: wt.branch,
            created: wt.created,
        })),
        Err(worktree::TaskWorktreeError::UnknownMode(m)) => Err((ErrorCode::InvalidParams, m)),
        Err(worktree::TaskWorktreeError::Failed(m)) => Err((ErrorCode::Internal, m)),
    }
}

/// `space.apply_layout`: the resolved layout of the space (or a preset of its project file)
/// built into the existing space as new tabs, or the trust question with nothing created.
fn space_apply_layout(shared: &Arc<Shared>, p: SpaceApplyLayout) -> ApiResult {
    let space_cwd = lock(&shared.inner).layout.space(&p.space_id)?.cwd.clone();
    let cwd = p
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(space_cwd.trim())
        .to_owned();
    if cwd.is_empty() {
        return Err((ErrorCode::InvalidParams, "the space has no cwd".into()));
    }
    let wanted = match p.preset.as_deref() {
        Some(name) => create::Wanted::Preset(name),
        None => create::Wanted::Resolved { root_command: None },
    };
    let build = match create::prepare(
        shared.home.as_deref(),
        &cwd,
        p.trust.as_ref(),
        wanted,
        &program_available(),
    )? {
        create::Prepared::Ask(trust_required) => {
            return Ok(json!(SpaceApplyLayoutResult::TrustRequired {
                trust_required
            }));
        }
        create::Prepared::Build(build) => build,
    };
    let (mut applied, typing) = {
        let mut inner = lock(&shared.inner);
        // The space may have gone between the lookup and the build.
        inner.layout.space(&p.space_id)?;
        let built = build_tabs(shared, &mut inner, &p.space_id, build);
        shared.persist(&inner);
        let built = built?;
        let space = inner.layout.space(&p.space_id)?.clone();
        (
            SpaceCreated {
                space,
                tabs: built.tabs(&inner),
                warnings: built.warnings,
                setup_started: built.setup_started,
            },
            built.typing,
        )
    };
    type_commands(typing, &mut applied);
    Ok(json!(SpaceApplyLayoutResult::Applied(applied)))
}

/// Types the commands of a built layout once the lock is released (a write may block on a full
/// PTY); one that cannot be typed is a warning.
fn type_commands(typing: Vec<Typing>, created: &mut SpaceCreated) {
    for typed in typing {
        if let Err(e) = typed
            .writer
            .write(format!("{}\r", typed.command).as_bytes())
        {
            created.warnings.push(format!(
                "pane \"{}\": the command was not typed: {e}",
                typed.pane
            ));
            if typed.setup {
                created.setup_started = false;
            }
        }
    }
}

/// A command to type into a pane of a new space.
struct Typing {
    pane: String,
    writer: pty::PtyWriter,
    command: String,
    setup: bool,
}

/// What [`build_tabs`] built.
struct Built {
    tab_ids: Vec<String>,
    warnings: Vec<String>,
    setup_started: bool,
    typing: Vec<Typing>,
}

impl Built {
    fn tabs(&self, inner: &Inner) -> Vec<proto::Tab> {
        self.tab_ids
            .iter()
            .filter_map(|id| inner.layout.tab(id).ok().cloned())
            .collect()
    }
}

/// Creates the space and builds the panes of `build` in it under one lock (see [`build_tabs`]).
/// When not even the root pane can be made, nothing of the space is left behind.
fn build_space(
    shared: &Arc<Shared>,
    name: String,
    build: create::Build,
) -> Result<(SpaceCreated, Vec<Typing>), (ErrorCode, String)> {
    let mut inner = lock(&shared.inner);
    let space_id = shared.new_id("s");
    let space = Space {
        id: space_id.clone(),
        name,
        cwd: build.dir.clone(),
        ..Default::default()
    };
    inner.layout.spaces.push(space.clone());
    shared.publish(Event::SpaceCreated { space });
    let built = match build_tabs(shared, &mut inner, &space_id, build) {
        Ok(built) => built,
        Err(e) => {
            inner.layout.spaces.retain(|s| s.id != space_id);
            shared.persist(&inner);
            shared.publish(Event::SpaceClosed { space_id });
            return Err(e);
        }
    };
    shared.persist(&inner);
    let space = inner.layout.space(&space_id)?.clone();
    let tabs = space
        .tabs
        .iter()
        .filter_map(|id| inner.layout.tab(id).ok().cloned())
        .collect();
    Ok((
        SpaceCreated {
            space,
            tabs,
            warnings: built.warnings,
            setup_started: built.setup_started,
        },
        built.typing,
    ))
}

/// Builds the panes of `build` into the space `space_id`, under the caller's lock: the root pane
/// in a new tab at the end of the space, every later pane as a split of its `from`, then the
/// `setup` tab. A pane after the root that cannot be made stops the layout with a warning; the
/// space keeps what was built. `Err` only when the root pane (or its tab) cannot be made; nothing
/// was built then. The caller persists.
fn build_tabs(
    shared: &Arc<Shared>,
    inner: &mut Inner,
    space_id: &str,
    build: create::Build,
) -> Result<Built, (ErrorCode, String)> {
    let shell = |name: &str| TerminalCreate {
        space_id: space_id.to_owned(),
        cwd: Some(build.dir.clone()),
        name: Some(name.to_string()).filter(|n| !n.is_empty()),
        ..Default::default()
    };
    let mut warnings = build.warnings;
    let mut typing = Vec::new();
    let mut tab_ids = Vec::new();
    let writer = |inner: &Inner, id: &str| {
        inner
            .terminals
            .iter()
            .find(|t| t.info.id == id)
            .and_then(|t| t.host.as_ref())
            .map(host::Handle::writer)
    };
    let mut queue = |inner: &Inner, pane: &str, id: &str, command: Option<&String>, setup| {
        if let (Some(command), Some(writer)) = (command, writer(inner, id)) {
            typing.push(Typing {
                pane: pane.to_string(),
                writer,
                command: command.clone(),
                setup,
            });
        }
    };
    let panes = build.layout.map(|l| l.panes).unwrap_or_default();
    let root_name = panes.first().map(|p| p.name.clone()).unwrap_or_default();
    let root = spawn_terminal(shared, inner, shell(&root_name))?;
    let root_tab = shared.new_id("tab");
    let events = match inner
        .layout
        .add_tab(space_id, root_tab.clone(), root.name.clone(), &root.id)
    {
        Ok(events) => events,
        Err(e) => {
            // A terminal is never left without a leaf.
            drop_terminal(shared, inner, &root.id);
            return Err(e);
        }
    };
    publish_all(shared, events);
    tab_ids.push(root_tab);
    queue(
        inner,
        &root.name,
        &root.id,
        panes.first().and_then(|p| p.command.as_ref()),
        false,
    );
    let mut ids = vec![(root_name, root.id)];
    for spec in panes.iter().skip(1) {
        let Some(split) = &spec.split else { continue };
        let Some(from) = ids
            .iter()
            .find(|(n, _)| *n == split.from)
            .map(|(_, id)| id.clone())
        else {
            warnings.push(format!(
                "layout not fully applied: pane \"{}\": unknown split.from \"{}\"",
                spec.name, split.from
            ));
            break;
        };
        let terminal = match spawn_terminal(shared, inner, shell(&spec.name)) {
            Ok(t) => t,
            Err((_, e)) => {
                warnings.push(format!(
                    "layout not fully applied: pane \"{}\": {e}",
                    spec.name
                ));
                break;
            }
        };
        let split_id = shared.new_id("split");
        match inner.layout.split(
            &from,
            &terminal.id,
            create::direction(split.direction),
            split.ratio,
            split_id,
        ) {
            Ok(events) => publish_all(shared, events),
            Err((_, e)) => {
                // A terminal is never left without a leaf.
                drop_terminal(shared, inner, &terminal.id);
                warnings.push(format!(
                    "layout not fully applied: pane \"{}\": {e}",
                    spec.name
                ));
                break;
            }
        }
        queue(
            inner,
            &spec.name,
            &terminal.id,
            spec.command.as_ref(),
            false,
        );
        ids.push((spec.name.clone(), terminal.id));
    }
    let mut setup_started = false;
    if let Some(setup) = &build.setup {
        let label = poimen_core::project::SETUP_LABEL;
        match spawn_terminal(shared, inner, shell(label)) {
            Ok(t) => {
                let tab_id = shared.new_id("tab");
                match inner
                    .layout
                    .add_tab(space_id, tab_id.clone(), label.to_string(), &t.id)
                {
                    Ok(events) => {
                        publish_all(shared, events);
                        tab_ids.push(tab_id);
                        queue(inner, label, &t.id, Some(setup), true);
                        setup_started = true;
                    }
                    Err((_, e)) => {
                        drop_terminal(shared, inner, &t.id);
                        warnings.push(format!("setup not started: {e}"));
                    }
                }
            }
            Err((_, e)) => warnings.push(format!("setup not started: {e}")),
        }
    }
    Ok(Built {
        tab_ids,
        warnings,
        setup_started,
        typing,
    })
}

fn publish_all(shared: &Shared, events: Vec<Event>) {
    for event in events {
        shared.publish(event);
    }
}

/// Closes a terminal that is not in any leaf yet (`terminal.closed`).
fn drop_terminal(shared: &Shared, inner: &mut Inner, terminal_id: &str) {
    if let Some(i) = inner
        .terminals
        .iter()
        .position(|t| t.info.id == terminal_id)
    {
        let entry = inner.terminals.remove(i);
        close_entry(shared, entry);
    }
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
    let stream = new_stream(shared, &id, cols, rows);
    let term = std::env::var("TERM").ok();
    let spawned = host::start(
        &shared.pty_host,
        &host::Spec {
            terminal_id: &id,
            socket: &shared.socket,
            command: &command,
            cwd: &cwd,
            cols,
            rows,
            term: term.as_deref(),
        },
        output_to(&stream),
        exit_to(shared, &id),
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
        args: command[1..].to_vec(),
        agent: None,
        tokens: Default::default(),
        survives_restart: true,
        host_version: Some(spawned.version().to_string()),
        host_bundled_version: Some(host_bundled_version()),
    };
    inner.terminals.push(TerminalEntry {
        info: info.clone(),
        host: Some(spawned),
        stream,
        agent_seq: None,
        tokens: Vec::new(),
    });
    shared.publish(Event::TerminalCreated {
        terminal: info.clone(),
    });
    Ok(info)
}

/// The screen and attachments of a new or adopted terminal.
fn new_stream(shared: &Arc<Shared>, id: &str, cols: u16, rows: u16) -> Arc<TermStream> {
    Arc::new(TermStream::new(
        id,
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
    ))
}

/// Where a host's output goes: the grid and the clients; a replay answers no query.
fn output_to(stream: &Arc<TermStream>) -> impl FnMut(&[u8], bool) + Send + 'static {
    let stream = Arc::clone(stream);
    move |bytes, live| {
        if live {
            stream.output(bytes);
        } else {
            stream.replay(bytes);
        }
    }
}

/// The host's reader reports the exit through the shared state.
fn exit_to(shared: &Arc<Shared>, id: &str) -> impl FnOnce(Option<u32>) + Send + 'static {
    let id = id.to_string();
    let shared = Arc::clone(shared);
    move |code| shared.terminal_exited(&id, code)
}

/// Adopts the hosts of the terminal list a previous server wrote (ADR 0009, "Server update"):
/// connects to every one at once, then rebuilds each grid from its ring and restores name, size,
/// agent status and tokens. A terminal keeps its id, so tabs and splits still point to it. A host
/// that does not answer (or a terminal whose space is gone) is dropped with a line on stderr.
fn adopt_terminals(shared: &Arc<Shared>, inner: &mut Inner, stored: Vec<store::StoredTerminal>) {
    let reached: Vec<_> = thread::scope(|scope| {
        let pending: Vec<_> = stored
            .into_iter()
            .map(|t| {
                let (endpoint, seq) = (t.endpoint.clone(), t.seq);
                let reach = scope.spawn(move || host::reach(&endpoint, seq, host::ADOPT_TIMEOUT));
                (t, reach)
            })
            .collect();
        pending
            .into_iter()
            .map(|(t, reach)| {
                let reached = reach
                    .join()
                    .unwrap_or_else(|_| Err("the connect failed".into()));
                (t, reached)
            })
            .collect()
    });
    for (t, reached) in reached {
        let dropped = |why: &str| {
            eprintln!(
                "poimen-server: dropping terminal {} ({}): {why}",
                t.id,
                t.endpoint.display()
            );
        };
        if inner.terminals.iter().any(|e| e.info.id == t.id) {
            dropped("listed twice");
            continue;
        }
        let reached = match reached {
            Ok(reached) => reached,
            Err(e) => {
                dropped(&format!("its host does not answer: {e}"));
                continue;
            }
        };
        if inner.layout.space(&t.space_id).is_err() {
            dropped(&format!("its space {} is gone", t.space_id));
            reached.kill();
            continue;
        }
        inner.terminals.push(adopted(shared, t, reached));
    }
}

/// The entry of an adopted terminal: the stored state over a grid fed from the host's ring.
fn adopted(
    shared: &Arc<Shared>,
    t: store::StoredTerminal,
    reached: host::Reached,
) -> TerminalEntry {
    let (cols, rows) = (reached.size.cols.max(1), reached.size.rows.max(1));
    let stream = new_stream(shared, &t.id, cols, rows);
    let handle = host::adopt(reached, output_to(&stream), exit_to(shared, &t.id));
    stream.set_writer(handle.writer());
    let (now, now_ms) = (Instant::now(), now_millis());
    let mut visible = std::collections::BTreeMap::new();
    let mut tokens = Vec::new();
    for token in t.tokens {
        let expires = match token.expires_at_ms {
            None => None,
            Some(at) if at > now_ms => Some(now + Duration::from_millis(at - now_ms)),
            // Its time ran out while no server was there.
            Some(_) => continue,
        };
        visible.insert(token.name.clone(), token.value);
        tokens.push(TokenEntry {
            name: token.name,
            source: token.source,
            expires,
        });
    }
    TerminalEntry {
        info: Terminal {
            id: t.id,
            space_id: t.space_id,
            name: t.name,
            cwd: t.cwd,
            cols,
            rows,
            status: TerminalStatus::Running,
            exit_code: None,
            program: t.program,
            args: t.args,
            agent: t.agent,
            tokens: visible,
            survives_restart: true,
            host_version: Some(handle.version().to_string()),
            host_bundled_version: Some(host_bundled_version()),
        },
        host: Some(handle),
        stream,
        agent_seq: t.agent_seq,
        tokens,
    }
}

/// The version of the host this server ships with. While the server and the host ship together
/// it is the version of this crate.
fn host_bundled_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
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
    shared.persist(&inner);
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
    // The status and its `seq` go into the terminal list, so a stale report stays dropped and
    // the status comes back after a restart.
    shared.persist(&inner);
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
    // Also an unchanged value may have a new owner or TTL.
    shared.persist(&inner);
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
    let cleared = before - entry.tokens.len();
    if cleared > 0 {
        tokens_changed(shared, entry);
        shared.persist(&inner);
    }
    Ok(json!({ "cleared": cleared }))
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
    publish_all(shared, events);
    shared.persist(&inner);
    Ok(json!({}))
}

/// Ends a terminal taken out of the list: its attachments, its process, `terminal.closed`. The
/// caller removes its leaf.
fn close_entry(shared: &Shared, entry: TerminalEntry) {
    entry.stream.closed();
    if let Some(host) = entry.host {
        host.kill();
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

fn running_host<'a>(
    entry: &'a mut TerminalEntry,
    id: &str,
) -> Result<&'a mut host::Handle, (ErrorCode, String)> {
    entry
        .host
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
        running_host(entry, &p.terminal_id)?.writer()
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
    running_host(entry, &p.terminal_id)?
        .resize(p.cols, p.rows)
        .map_err(|e| (ErrorCode::Internal, e))?;
    entry.info.cols = p.cols;
    entry.info.rows = p.rows;
    entry.stream.resized(p.cols, p.rows);
    shared.persist(&inner);
    Ok(json!({}))
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
