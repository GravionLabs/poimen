//! Starting `poimen-server` for the console (#1171): the rules of `poimen-server stdio` and the
//! app's `ensure_running`. A server that answers is used as it is; when none runs, the one next to
//! the console binary, else on the `PATH`, is started detached (own process group, no terminal,
//! never waited on) and polled until it answers. Inside a poimen terminal (`POIMEN_SOCKET` set)
//! nothing is started: the terminal's server runs by definition.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_core::gui_path;

/// How long a started server may take to answer.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum Error {
    /// No server program next to the console or on the `PATH`; holds its file name.
    NotFound(String),
    Spawn(io::Error),
    Timeout(Duration),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(name) => write!(
                f,
                "no poimen server is running and `{name}` was not found next to poimen or on PATH; install poimen (the app or the `{name}` package) or start the server by hand"
            ),
            Error::Spawn(e) => write!(f, "could not start poimen-server: {e}"),
            Error::Timeout(t) => write!(f, "poimen-server did not answer within {} s", t.as_secs()),
        }
    }
}

impl std::error::Error for Error {}

/// The server program of this build: next to the console binary (`bin_dir`), else on `path`.
pub fn find_server(bin_dir: Option<&Path>, path: &str) -> Option<PathBuf> {
    let name = server_name();
    bin_dir
        .map(|d| d.join(&name))
        .filter(|p| p.is_file())
        .or_else(|| gui_path::resolve_program(&name, path))
}

pub fn server_name() -> String {
    poimen_protocol::CHANNEL.server_program(cfg!(windows))
}

/// Returns at once when a server answers. When none does (`is_not_running`), `find` locates the
/// program, `spawn` starts it and the socket is polled for `timeout`. Any other failure of the
/// ping (a refused or unsafe socket, another protocol) starts nothing: the command reports it.
pub fn ensure_with(
    client: &Client,
    find: impl FnOnce() -> Option<PathBuf>,
    spawn: impl FnOnce(&Path) -> io::Result<()>,
    timeout: Duration,
) -> Result<(), Error> {
    match client.ping() {
        Err(e) if e.is_not_running() => {}
        _ => return Ok(()),
    }
    let program = find().ok_or_else(|| Error::NotFound(server_name()))?;
    spawn(&program).map_err(Error::Spawn)?;
    let begin = Instant::now();
    while begin.elapsed() < timeout {
        thread::sleep(POLL_INTERVAL);
        if client.ping().is_ok() {
            return Ok(());
        }
    }
    Err(Error::Timeout(timeout))
}

/// What `poimen` does before a command that needs a server, with this process's environment.
pub fn ensure_for_console() -> Result<(), Error> {
    if std::env::var_os(poimen_protocol::ENV_SOCKET).is_some_and(|v| !v.is_empty()) {
        return Ok(());
    }
    let client = Client::new(poimen_protocol::terminal_socket_path());
    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    ensure_with(
        &client,
        || find_server(bin_dir.as_deref(), &gui_path::child_path()),
        spawn_detached,
        START_TIMEOUT,
    )
}

/// Starts `program` so it outlives this process: own process group, no terminal, never waited on.
#[allow(clippy::disallowed_methods)] // CREATE_NO_WINDOW is set below; gui_path is the app's
pub fn spawn_detached(program: &Path) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x0000_0200 | 0x0800_0000);
    }
    let mut child = command.spawn()?;
    // Reaped if it exits (a second server exits at once); never killed.
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
