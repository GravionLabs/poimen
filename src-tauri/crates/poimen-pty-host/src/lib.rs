//! `poimen-pty-host`: owns one terminal, its PTY and child, for the child's whole life (ADR 0009).
//!
//! The host forwards and buffers bytes and never interprets them: no emulation, spaces, agent
//! status or tokens. It serves one client at a time on its endpoint (`docs/protocol.md`, "Host
//! protocol"); a second connection replaces the first. It ends with its child, after a `kill`,
//! and after the orphan timeout without any client.
//!
//! Threads: the PTY reader (output into the ring and to the client's queue), the child waiter
//! (`exit`), the accept loop, a reader and a writer per client. [`Host::serve`] waits for the end
//! on a condition variable. The PTY is never blocked by a client: a client whose queue is full is
//! dropped and reconnects with `since`.

pub mod ring;

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::transport::{self, Conn, Listener};
use poimen_protocol::host::{
    ClientMessage, HOST_VERSION, HostMessage, HostStart, MAX_RING_RECORDS, Size, parse_client_line,
    to_line,
};
use poimen_protocol::{decode_bytes, encode_bytes};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use ring::Ring;

/// Records waiting for one client; a client this far behind is dropped.
const CLIENT_QUEUE: usize = 1024;
/// How long the exit report waits for the output of the dead child to be read.
const DRAIN_WAIT: Duration = Duration::from_secs(1);
/// How long the host waits for its child to end after a kill before it ends anyway.
const KILL_WAIT: Duration = Duration::from_secs(5);
/// A client that stops reading is cut off after this (Unix; pipes have no timeouts).
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// `poimen-pty-host <version> (host protocol <n>)`.
pub fn version_line() -> String {
    format!(
        "poimen-pty-host {} (host protocol {HOST_VERSION})",
        env!("CARGO_PKG_VERSION")
    )
}

enum Out {
    Line(Vec<u8>),
    /// Write, then end the connection.
    Close(Vec<u8>),
    /// The `exit` record: write, report it delivered, end the connection.
    Exit(Vec<u8>),
}

struct Client {
    id: u64,
    tx: SyncSender<Out>,
    conn: Conn,
    attached: bool,
}

struct State {
    ring: Ring,
    client: Option<Client>,
    next_client: u64,
    size: Size,
    /// The child's exit code once it ended (`Some(None)`: unknown code).
    exit: Option<Option<u32>>,
    exited_at: Option<Instant>,
    /// A client got the `exit` record.
    delivered: bool,
    /// Since when no client is connected; `None` while one is.
    alone_since: Option<Instant>,
    killed_at: Option<Instant>,
    orphaned: bool,
}

impl State {
    /// Queues `out` for the current client; a full queue drops the client.
    fn send(&mut self, out: Out) {
        let Some(client) = &self.client else { return };
        if let Err(TrySendError::Full(_)) = client.tx.try_send(out) {
            self.drop_client();
        }
    }

    fn drop_client(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = client.conn.shutdown();
            self.alone_since = Some(Instant::now());
        }
    }

    fn is_current(&self, id: u64) -> bool {
        self.client.as_ref().is_some_and(|c| c.id == id)
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    input: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: u32,
    child_pid: Option<u32>,
    endpoint: PathBuf,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn hello(&self, state: &State) -> HostMessage {
        HostMessage::Hello {
            host_version: HOST_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: self.pid,
            child_pid: self.child_pid,
            size: state.size,
            endpoint: self.endpoint.clone(),
            seq: state.ring.last_seq(),
        }
    }

    fn kill(&self) {
        let _ = self.killer.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

/// A started host: endpoint bound, child running.
pub struct Host {
    shared: Arc<Shared>,
    listener: Listener,
    orphan_timeout: Duration,
    exit_linger: Duration,
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn error(message: impl Into<String>) -> Vec<u8> {
    to_line(&HostMessage::Error {
        message: message.into(),
    })
}

/// Removes the endpoint's entry (Unix socket file, Windows marker file) when it is one.
fn remove_endpoint(path: &Path) {
    if transport::inspect(path).is_ok_and(|e| e == transport::Entry::Socket) {
        let _ = std::fs::remove_file(path);
    }
}

impl Host {
    /// Binds the endpoint and starts the child in a new PTY.
    pub fn start(start: &HostStart) -> Result<Host, String> {
        let spawn = &start.spawn;
        let Some(program) = spawn.argv.first() else {
            return Err("spawn.argv is empty".into());
        };
        if spawn.cols == 0 || spawn.rows == 0 {
            return Err("spawn size must be at least 1x1".into());
        }
        let listener = Listener::bind(&start.endpoint)
            .map_err(|e| format!("could not bind {}: {e}", start.endpoint.display()))?;
        let started = Self::spawn(start, program, listener);
        if started.is_err() {
            remove_endpoint(&start.endpoint);
        }
        started
    }

    fn spawn(start: &HostStart, program: &str, listener: Listener) -> Result<Host, String> {
        let spawn = &start.spawn;
        // ConPTY with `INHERIT_CURSOR` holds the child back until its cursor query is answered,
        // and the host answers nothing (it does not emulate; no client may be connected).
        portable_pty::conpty_flags::set_inherit_cursor(false);
        let pair = native_pty_system()
            .openpty(pty_size(spawn.cols, spawn.rows))
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(&spawn.argv[1..]);
        cmd.cwd(&spawn.cwd);
        for (key, value) in &spawn.env {
            cmd.env(key, value);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("could not start {program}: {e}"))?;
        // Only the child holds the slave, so the reader sees end of file when it ends.
        drop(pair.slave);
        let killer = child.clone_killer();
        let child_pid = child.process_id();
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let input = pair.master.take_writer().map_err(|e| e.to_string())?;

        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                ring: Ring::new(start.ring_bytes, MAX_RING_RECORDS),
                client: None,
                next_client: 1,
                size: Size {
                    cols: spawn.cols,
                    rows: spawn.rows,
                },
                exit: None,
                exited_at: None,
                delivered: false,
                alone_since: Some(Instant::now()),
                killed_at: None,
                orphaned: false,
            }),
            changed: Condvar::new(),
            input: Mutex::new(input),
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
            pid: std::process::id(),
            child_pid,
            endpoint: start.endpoint.clone(),
        });

        let (drained_tx, drained_rx) = mpsc::channel::<()>();
        let out = Arc::clone(&shared);
        thread::spawn(move || {
            read_output(&out, reader);
            let _ = drained_tx.send(());
        });
        let waiter = Arc::clone(&shared);
        thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            // A grandchild that keeps the PTY open must not delay the report for ever (and
            // ConPTY keeps its output pipe open until the pseudoconsole is closed).
            let _ = drained_rx.recv_timeout(DRAIN_WAIT);
            let mut state = waiter.lock();
            state.exit = Some(code);
            state.exited_at = Some(Instant::now());
            if state.client.as_ref().is_some_and(|c| c.attached) {
                state.send(Out::Exit(to_line(&HostMessage::Exit { code })));
            }
            waiter.changed.notify_all();
        });

        Ok(Host {
            shared,
            listener,
            orphan_timeout: Duration::from_millis(start.orphan_timeout_ms),
            exit_linger: Duration::from_millis(start.exit_linger_ms),
        })
    }

    /// The `hello` record as of now.
    pub fn hello(&self) -> HostMessage {
        self.shared.hello(&self.shared.lock())
    }

    /// Serves clients until the host ends, then removes the endpoint. The caller ends the
    /// process (threads blocked on the PTY or a client stay behind).
    pub fn serve(self) {
        let Host {
            shared,
            listener,
            orphan_timeout,
            exit_linger,
        } = self;
        let accepting = Arc::clone(&shared);
        thread::spawn(move || {
            loop {
                match listener.accept() {
                    Ok(conn) => connect(&accepting, conn),
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            }
        });

        let mut state = shared.lock();
        loop {
            let now = Instant::now();
            let deadline = if state.delivered {
                break;
            } else if let Some(at) = state.exited_at {
                if state.orphaned || now >= at + exit_linger {
                    break;
                }
                Some(at + exit_linger)
            } else if let Some(at) = state.killed_at {
                if now >= at + KILL_WAIT {
                    break;
                }
                Some(at + KILL_WAIT)
            } else if let Some(since) = state.alone_since {
                if now >= since + orphan_timeout {
                    shared.kill();
                    state.orphaned = true;
                    state.killed_at = Some(now);
                    continue;
                }
                Some(since + orphan_timeout)
            } else {
                None
            };
            let wait = deadline.map_or(Duration::from_secs(60), |d| {
                d.saturating_duration_since(now)
                    .min(Duration::from_secs(60))
            });
            state = shared
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        drop(state);
        remove_endpoint(&shared.endpoint);
    }
}

/// Drains the PTY into the ring and the attached client's queue until end of file.
fn read_output(shared: &Shared, mut reader: Box<dyn Read + Send>) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let data = &buf[..n];
        let mut state = shared.lock();
        let seq = state.ring.push(data.to_vec());
        if state.client.as_ref().is_some_and(|c| c.attached) {
            let line = to_line(&HostMessage::Output {
                seq,
                bytes: encode_bytes(data),
            });
            state.send(Out::Line(line));
        }
    }
}

/// A new connection: replaces the current client and greets the new one.
fn connect(shared: &Arc<Shared>, conn: Conn) {
    let _ = conn.set_write_timeout(Some(WRITE_TIMEOUT));
    let (tx, rx) = mpsc::sync_channel::<Out>(CLIENT_QUEUE);
    let id = {
        let mut state = shared.lock();
        if let Some(old) = state.client.take()
            && old
                .tx
                .try_send(Out::Close(to_line(&HostMessage::Replaced)))
                .is_err()
        {
            let _ = old.conn.shutdown();
        }
        let id = state.next_client;
        state.next_client += 1;
        let _ = tx.try_send(Out::Line(to_line(&shared.hello(&state))));
        state.client = Some(Client {
            id,
            tx,
            conn: conn.clone(),
            attached: false,
        });
        state.alone_since = None;
        shared.changed.notify_all();
        id
    };
    let writer = Arc::clone(shared);
    let write_conn = conn.clone();
    thread::spawn(move || write_client(&writer, id, &write_conn, rx));
    let reader = Arc::clone(shared);
    thread::spawn(move || read_client(&reader, id, &conn));
}

fn write_client(shared: &Shared, id: u64, conn: &Conn, rx: mpsc::Receiver<Out>) {
    for out in rx {
        match out {
            Out::Line(line) => {
                if (&*conn).write_all(&line).is_err() {
                    break;
                }
            }
            Out::Close(line) => {
                let _ = (&*conn).write_all(&line);
                break;
            }
            Out::Exit(line) => {
                if (&*conn).write_all(&line).is_ok() {
                    shared.lock().delivered = true;
                    shared.changed.notify_all();
                }
                break;
            }
        }
    }
    let _ = conn.shutdown();
    gone(shared, id);
}

/// The client `id` is gone; the host is alone unless another one replaced it.
fn gone(shared: &Shared, id: u64) {
    let mut state = shared.lock();
    if state.is_current(id) {
        state.drop_client();
        shared.changed.notify_all();
    }
}

fn read_client(shared: &Shared, id: u64, conn: &Conn) {
    let mut lines = BufReader::new(conn);
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        if !shared.lock().is_current(id) {
            break;
        }
        handle(shared, id, line.trim_end());
    }
    gone(shared, id);
}

fn handle(shared: &Shared, id: u64, line: &str) {
    let reply = match parse_client_line(line) {
        Err(e) => Some(error(format!("invalid line: {e}"))),
        Ok(ClientMessage::Attach { since }) => {
            attach(shared, id, since);
            None
        }
        Ok(ClientMessage::Input { bytes }) => match decode_bytes(&bytes) {
            Err(e) => Some(error(format!("invalid input bytes: {e}"))),
            Ok(data) => {
                let mut input = shared.input.lock().unwrap_or_else(|e| e.into_inner());
                input
                    .write_all(&data)
                    .and_then(|()| input.flush())
                    .err()
                    .map(|e| error(format!("input failed: {e}")))
            }
        },
        Ok(ClientMessage::Resize { cols, rows }) => resize(shared, cols, rows).err().map(error),
        Ok(ClientMessage::Kill) => {
            shared.kill();
            let mut state = shared.lock();
            state.killed_at.get_or_insert_with(Instant::now);
            shared.changed.notify_all();
            None
        }
        Ok(ClientMessage::Unknown) => Some(error("unknown message type")),
    };
    if let Some(reply) = reply {
        let mut state = shared.lock();
        if state.is_current(id) {
            state.send(Out::Line(reply));
        }
    }
}

fn resize(shared: &Shared, cols: u16, rows: u16) -> Result<(), String> {
    if cols == 0 || rows == 0 {
        return Err("size must be at least 1x1".into());
    }
    shared
        .master
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .resize(pty_size(cols, rows))
        .map_err(|e| format!("resize failed: {e}"))?;
    shared.lock().size = Size { cols, rows };
    Ok(())
}

/// Starts the stream of client `id` after record `since`: the gap, the ring, then live output.
fn attach(shared: &Shared, id: u64, since: u64) {
    let mut state = shared.lock();
    let Some(client) = &state.client else { return };
    if client.id != id {
        return;
    }
    if client.attached {
        state.send(Out::Line(error("already attached")));
        return;
    }
    let replay = match state.ring.replay(since) {
        Ok(replay) => replay,
        Err(ahead) => {
            let message = format!(
                "since {since} is ahead of this host (last seq {})",
                ahead.last_seq
            );
            state.send(Out::Line(error(message)));
            return;
        }
    };
    let mut lines = Vec::new();
    if let Some((from, to)) = replay.gap {
        lines.extend(to_line(&HostMessage::Gap { from, to }));
    }
    for (seq, data) in replay.records {
        lines.extend(to_line(&HostMessage::Output {
            seq,
            bytes: encode_bytes(data),
        }));
    }
    if let Some(client) = state.client.as_mut() {
        client.attached = true;
    }
    if !lines.is_empty() {
        state.send(Out::Line(lines));
    }
    if let Some(code) = state.exit {
        state.send(Out::Exit(to_line(&HostMessage::Exit { code })));
    }
}

/// Starts a host in this process and serves it on a thread: what the binary does, without a
/// process of its own. For clients' tests (`poimen-server`'s `PtyHost::Launch`); `Ok` once the
/// endpoint is bound and the child runs.
pub fn spawn_in_thread(start: &HostStart) -> Result<(), String> {
    let host = Host::start(start)?;
    thread::spawn(move || host.serve());
    Ok(())
}

/// Reads the [`HostStart`] line (without waiting for end of file).
pub fn read_start(input: impl Read) -> Result<HostStart, String> {
    let mut line = String::new();
    BufReader::new(input)
        .read_line(&mut line)
        .map_err(|e| format!("could not read the start line: {e}"))?;
    if line.trim().is_empty() {
        return Err("no start line on stdin".into());
    }
    serde_json::from_str(&line).map_err(|e| format!("invalid start line: {e}"))
}

/// Writes one host message as a line and flushes.
pub fn write_message(out: &mut impl Write, message: &HostMessage) -> io::Result<()> {
    out.write_all(&to_line(message))?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #455: on Windows the ConPTY is created with pass-through (conhost must not re-encode VT
    /// mouse input), falling back to the flags of upstream portable-pty when the OS rejects it.
    /// (Read before any host starts here, so `INHERIT_CURSOR` is still in both sets.)
    #[test]
    fn conpty_tries_passthrough_then_falls_back() {
        use portable_pty::conpty_flags::{PASSTHROUGH_MODE, UPSTREAM, attempts};
        let a = attempts();
        assert_eq!(a[0] & PASSTHROUGH_MODE, 0x8);
        assert_eq!(a[0], 0xF);
        assert_eq!(a[1], UPSTREAM);
        assert_eq!(a[1] & PASSTHROUGH_MODE, 0);
    }

    #[test]
    fn the_start_line_is_read_without_waiting_for_more() {
        let input: &[u8] = b"{\"endpoint\":\"/x\",\"spawn\":{\"argv\":[\"sh\"],\"cwd\":\"/\",\"cols\":80,\"rows\":24}}\nrest";
        let start = read_start(input).unwrap();
        assert_eq!(start.endpoint, PathBuf::from("/x"));
        assert_eq!(start.spawn.argv, vec!["sh"]);
    }

    #[test]
    fn a_missing_or_invalid_start_line_is_an_error() {
        assert_eq!(read_start(&b""[..]).unwrap_err(), "no start line on stdin");
        assert!(
            read_start(&b"{}\n"[..])
                .unwrap_err()
                .starts_with("invalid start line")
        );
    }

    #[test]
    fn the_version_line_names_the_host_protocol() {
        assert_eq!(
            version_line(),
            format!(
                "poimen-pty-host {} (host protocol 1)",
                env!("CARGO_PKG_VERSION")
            )
        );
    }
}
