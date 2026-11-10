//! `poimen-server stdio`: connects stdin and stdout to the socket of the user's running server, so
//! `ssh host poimen-server stdio` reaches it. The bridge is transparent: bytes pass unchanged in
//! both directions (the NDJSON lines of `docs/protocol.md` are never parsed or reframed).
//!
//! It starts the server detached when none answers (the rules of the app's `ensure_running`: a
//! server that answers is used as it is, never stopped or replaced). Closing stdin or a dropped
//! SSH session ends only the bridge: the server sees one connection close, like any client, and
//! keeps its terminals.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::transport::Conn;

/// How long a started server may take to answer.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// After stdin ended, how long the server may still answer before the bridge ends.
pub const DRAIN: Duration = Duration::from_secs(2);

/// Connects to the server on `socket`; when none answers, calls `start` once (detached, not
/// waited on) and polls until one does. Any other failure (a refused or unsafe socket) is
/// returned without starting anything.
pub fn connect_or_start(
    socket: &Path,
    start: impl FnOnce() -> io::Result<()>,
    timeout: Duration,
) -> io::Result<Conn> {
    let client = Client::new(socket);
    match client.ping() {
        Ok(_) => return Conn::connect(socket),
        Err(e) if e.is_not_running() => {}
        // A server that answers in another protocol is still bridged: the remote client decides.
        Err(poimen_client::ClientError::Protocol(_)) => return Conn::connect(socket),
        Err(e) => return Err(io::Error::other(e.to_string())),
    }
    start().map_err(|e| io::Error::other(format!("could not start poimen-server: {e}")))?;
    let begin = Instant::now();
    while begin.elapsed() < timeout {
        thread::sleep(POLL_INTERVAL);
        if client.ping().is_ok() {
            return Conn::connect(socket);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "poimen-server did not answer within {} s",
            timeout.as_secs()
        ),
    ))
}

/// Starts this executable as the server: own process group, no terminal, never waited on.
#[allow(clippy::disallowed_methods)] // CREATE_NO_WINDOW is set below; gui_path is the app's
pub fn spawn_server() -> io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(std::env::current_exe()?);
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

fn copy(mut from: impl Read, mut to: impl Write) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buf[..n])?;
        to.flush()?;
    }
}

/// Copies `input` to the connection and the connection to `output` until one side ends. When
/// `input` ends, the server gets end of file and may still answer for `drain`. Only the
/// connection is closed on return; the server and its terminals are not touched.
pub fn bridge<R, W>(conn: &Conn, input: R, output: W, drain: Duration) -> io::Result<()>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    enum Done {
        Up,
        Down(io::Result<()>),
    }
    let (tx, rx) = mpsc::channel();
    let up = {
        let (conn, tx) = (conn.clone(), tx.clone());
        move || {
            let mut writer = &conn;
            let _ = copy(input, &mut writer);
            let _ = conn.shutdown_write();
            let _ = tx.send(Done::Up);
        }
    };
    let down = {
        let (conn, tx) = (conn.clone(), tx);
        move || {
            let _ = tx.send(Done::Down(copy(&conn, output)));
        }
    };
    thread::spawn(up);
    thread::spawn(down);
    let result = match rx.recv() {
        Ok(Done::Down(r)) => r,
        Ok(Done::Up) => match rx.recv_timeout(drain) {
            Ok(Done::Down(r)) => r,
            _ => Ok(()),
        },
        Err(_) => Ok(()),
    };
    let _ = conn.shutdown();
    result
}

/// The command: connect (starting the server if needed) and bridge the process's stdin and stdout.
pub fn run(socket: &Path) -> io::Result<()> {
    let conn = connect_or_start(socket, spawn_server, START_TIMEOUT)?;
    bridge(&conn, io::stdin(), io::stdout(), DRAIN)
}
