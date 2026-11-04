//! `poimen-server`: a headless daemon that owns the PTYs, so shells and agents keep running when
//! the app closes. One server per user listens on a Unix socket (`docs/protocol.md`).
//!
//! The server keeps a flat model: spaces (name, cwd) with terminals. Spaces persist across
//! restarts, terminals do not. Layout (groups, splits, tabs) is the client's business.

mod pty;
mod store;

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use poimen_protocol::{
    self as proto, ErrorCode, Event, PROTOCOL_VERSION, PingResult, Request, Response, Space,
    SpaceClose, SpaceCreate, SpaceRename, Terminal, TerminalClose, TerminalCreate, TerminalInput,
    TerminalList, TerminalRename, TerminalResize, TerminalStatus,
};
use serde_json::{Value, json};

pub use store::state_file_path;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

/// Where the server listens and keeps its spaces.
#[derive(Debug, Clone)]
pub struct Config {
    pub socket: PathBuf,
    pub state_file: PathBuf,
}

impl Config {
    /// The paths of this user (`POIMEN_SOCKET`, `POIMEN_STATE_DIR` override the defaults).
    pub fn from_env() -> Self {
        Self {
            socket: proto::socket_path(),
            state_file: state_file_path(),
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
}

#[derive(Default)]
struct Inner {
    spaces: Vec<Space>,
    terminals: Vec<TerminalEntry>,
}

pub(crate) struct Shared {
    inner: Mutex<Inner>,
    subscribers: Mutex<Vec<Sender<Event>>>,
    next_id: AtomicU64,
    socket: PathBuf,
    state_file: PathBuf,
}

impl Shared {
    /// Sends the event to every subscriber, dropping the ones that went away. Called with the
    /// `inner` lock held, so events arrive in the order the changes happened.
    fn publish(&self, event: Event) {
        lock(&self.subscribers).retain(|tx| tx.send(event.clone()).is_ok());
    }

    fn new_id(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn persist(&self, inner: &Inner) {
        if let Err(e) = store::save(&self.state_file, &inner.spaces) {
            eprintln!(
                "poimen-server: could not save {}: {e}",
                self.state_file.display()
            );
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
        self.publish(Event::TerminalExited {
            terminal_id: terminal_id.to_string(),
            exit_code,
        });
    }
}

type ApiResult = Result<Value, (ErrorCode, String)>;

fn not_found(what: &str, id: &str) -> (ErrorCode, String) {
    (ErrorCode::NotFound, format!("no {what} {id}"))
}

/// A bound server, ready to [`serve`](Server::serve).
pub struct Server {
    #[cfg(unix)]
    listener: UnixListener,
    /// Keeps the lock next to the socket for as long as the server lives.
    #[cfg(unix)]
    _lock: Option<std::fs::File>,
    shared: Arc<Shared>,
}

impl Server {
    /// Binds the socket (directory mode 0700, socket 0600), replacing a stale socket file, and
    /// loads the persisted spaces. Fails with [`StartError::AlreadyRunning`] when a server answers.
    #[cfg(unix)]
    pub fn bind(config: Config) -> Result<Self, StartError> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

        let mut lock_file = None;
        if let Some(dir) = config.socket.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            // A directory that already existed may be someone else's or too open: tighten what
            // is ours, refuse the rest.
            proto::check_private_dir(dir, true)?;
            // Held for the life of the server: whoever holds it is the only one to judge the
            // socket file stale.
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600)
                .open(dir.join("server.lock"))?;
            if file.try_lock().is_err() {
                return Err(StartError::AlreadyRunning(config.socket));
            }
            lock_file = Some(file);
        }
        if let Ok(meta) = std::fs::symlink_metadata(&config.socket) {
            if !std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) {
                return Err(StartError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a socket", config.socket.display()),
                )));
            }
            if UnixStream::connect(&config.socket).is_ok() {
                return Err(StartError::AlreadyRunning(config.socket));
            }
            std::fs::remove_file(&config.socket)?;
        }
        let listener = UnixListener::bind(&config.socket).map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse {
                StartError::AlreadyRunning(config.socket.clone())
            } else {
                StartError::Io(e)
            }
        })?;
        std::fs::set_permissions(&config.socket, std::fs::Permissions::from_mode(0o600))?;

        let spaces = store::load(&config.state_file);
        let next = store::next_id(&spaces);
        let shared = Shared {
            inner: Mutex::new(Inner {
                spaces,
                terminals: Vec::new(),
            }),
            subscribers: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(next),
            socket: config.socket,
            state_file: config.state_file,
        };
        Ok(Self {
            listener,
            _lock: lock_file,
            shared: Arc::new(shared),
        })
    }

    #[cfg(not(unix))]
    pub fn bind(_: Config) -> Result<Self, StartError> {
        Err(StartError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "poimen-server needs Unix sockets",
        )))
    }

    pub fn socket(&self) -> &Path {
        &self.shared.socket
    }

    /// Accepts connections forever, one thread each.
    #[cfg(unix)]
    pub fn serve(self) {
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

    #[cfg(not(unix))]
    pub fn serve(self) {}
}

#[cfg(unix)]
fn handle_connection(shared: &Arc<Shared>, stream: UnixStream) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut writer = stream;
    for line in BufReader::new(read_half).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let request = match proto::parse_request(&line) {
            Ok(r) => r,
            Err(response) => {
                if send(&mut writer, &response).is_err() {
                    return;
                }
                continue;
            }
        };
        if request.method == "events.subscribe" {
            // The subscription is registered before the response is sent, so an event caused by
            // the next request on another connection is never missed.
            let (tx, rx) = mpsc::channel();
            lock(&shared.subscribers).push(tx);
            let ok = Response::ok(&request.id, json!({ "subscribed": true }));
            if send(&mut writer, &ok).is_err() {
                return;
            }
            // From here the connection only carries events.
            for event in rx {
                if send(&mut writer, &event).is_err() {
                    return;
                }
            }
            return;
        }
        let response = match dispatch(shared, &request) {
            Ok(result) => Response::ok(&request.id, result),
            Err((code, message)) => Response::err(Some(&request.id), code, message),
        };
        if send(&mut writer, &response).is_err() {
            return;
        }
    }
}

#[cfg(unix)]
fn send<T: serde::Serialize>(writer: &mut UnixStream, value: &T) -> io::Result<()> {
    let mut line = serde_json::to_string(value).map_err(io::Error::other)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

fn parsed<T: for<'de> serde::Deserialize<'de>>(
    request: &Request,
) -> Result<T, (ErrorCode, String)> {
    proto::params(request).map_err(|r| {
        let e = r.error.expect("params errors carry an error");
        (e.code, e.message)
    })
}

fn dispatch(shared: &Arc<Shared>, request: &Request) -> ApiResult {
    match request.method.as_str() {
        "ping" => Ok(json!(PingResult {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: PROTOCOL_VERSION,
            pid: std::process::id(),
        })),
        "space.list" => Ok(json!({ "spaces": lock(&shared.inner).spaces })),
        "space.create" => space_create(shared, parsed(request)?),
        "space.rename" => space_rename(shared, parsed(request)?),
        "space.close" => space_close(shared, parsed(request)?),
        "terminal.list" => terminal_list(shared, parsed(request)?),
        "terminal.create" => terminal_create(shared, parsed(request)?),
        "terminal.rename" => terminal_rename(shared, parsed(request)?),
        "terminal.close" => terminal_close(shared, parsed(request)?),
        "terminal.input" => terminal_input(shared, parsed(request)?),
        "terminal.resize" => terminal_resize(shared, parsed(request)?),
        other => Err((ErrorCode::UnknownMethod, format!("unknown method {other}"))),
    }
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
    };
    inner.spaces.push(space.clone());
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
    let index = inner
        .spaces
        .iter()
        .position(|s| s.id == p.space_id)
        .ok_or_else(|| not_found("space", &p.space_id))?;
    let (closing, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut inner.terminals)
        .into_iter()
        .partition(|t| t.info.space_id == p.space_id);
    inner.terminals = keep;
    for entry in closing {
        if let Some(pty) = entry.pty {
            pty.kill();
        }
        shared.publish(Event::TerminalClosed {
            terminal_id: entry.info.id,
            space_id: p.space_id.clone(),
        });
    }
    inner.spaces.remove(index);
    shared.persist(&inner);
    shared.publish(Event::SpaceClosed {
        space_id: p.space_id,
    });
    Ok(json!({}))
}

fn terminal_list(shared: &Shared, p: TerminalList) -> ApiResult {
    let inner = lock(&shared.inner);
    let terminals: Vec<&Terminal> = inner
        .terminals
        .iter()
        .map(|t| &t.info)
        .filter(|t| p.space_id.as_ref().is_none_or(|id| &t.space_id == id))
        .collect();
    Ok(json!({ "terminals": terminals }))
}

fn terminal_create(shared: &Arc<Shared>, p: TerminalCreate) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let space = inner
        .spaces
        .iter()
        .find(|s| s.id == p.space_id)
        .ok_or_else(|| not_found("space", &p.space_id))?;
    let cwd = p.cwd.clone().unwrap_or_else(|| space.cwd.clone());
    let command = match p.command {
        Some(c) if c.is_empty() || c[0].is_empty() => {
            return Err((ErrorCode::InvalidParams, "command must not be empty".into()));
        }
        Some(c) => c,
        None => vec![pty::default_shell()],
    };
    let id = shared.new_id("t");
    let (cols, rows) = (p.cols.unwrap_or(80).max(1), p.rows.unwrap_or(24).max(1));
    let name = p
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| pty::program_name(&command[0]));
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
            // The wait thread reports the exit through the shared state.
            let id = id.clone();
            let shared = Arc::clone(shared);
            move |code| shared.terminal_exited(&id, code)
        },
    )
    .map_err(|e| (ErrorCode::SpawnFailed, e))?;
    let info = Terminal {
        id,
        space_id: p.space_id,
        name,
        cwd,
        cols,
        rows,
        status: TerminalStatus::Running,
        exit_code: None,
    };
    inner.terminals.push(TerminalEntry {
        info: info.clone(),
        pty: Some(spawned),
    });
    shared.publish(Event::TerminalCreated {
        terminal: info.clone(),
    });
    Ok(json!({ "terminal": info }))
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

fn terminal_close(shared: &Shared, p: TerminalClose) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let index = inner
        .terminals
        .iter()
        .position(|t| t.info.id == p.terminal_id)
        .ok_or_else(|| not_found("terminal", &p.terminal_id))?;
    let entry = inner.terminals.remove(index);
    if let Some(pty) = entry.pty {
        pty.kill();
    }
    shared.publish(Event::TerminalClosed {
        terminal_id: entry.info.id,
        space_id: entry.info.space_id,
    });
    Ok(json!({}))
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

fn terminal_input(shared: &Shared, p: TerminalInput) -> ApiResult {
    // The write may block on a full PTY; do it outside the global lock.
    let writer = {
        let mut inner = lock(&shared.inner);
        let entry = terminal_mut(&mut inner, &p.terminal_id)?;
        running_pty(entry, &p.terminal_id)?.writer()
    };
    writer
        .write(p.data.as_bytes())
        .map_err(|e| (ErrorCode::Internal, e))?;
    Ok(json!({}))
}

fn terminal_resize(shared: &Shared, p: TerminalResize) -> ApiResult {
    if p.cols == 0 || p.rows == 0 {
        return Err((
            ErrorCode::InvalidParams,
            "cols and rows must be at least 1".into(),
        ));
    }
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    running_pty(entry, &p.terminal_id)?
        .resize(p.cols, p.rows)
        .map_err(|e| (ErrorCode::Internal, e))?;
    entry.info.cols = p.cols;
    entry.info.rows = p.rows;
    Ok(json!({}))
}
