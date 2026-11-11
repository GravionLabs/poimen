//! The server as a client of `poimen-pty-host` (ADR 0009, `docs/protocol.md`, "Host protocol").
//!
//! Every terminal runs in a host of its own: [`start`] launches it with the [`HostStart`] line,
//! connects to its endpoint and attaches. A reader thread takes the output records strictly by
//! `seq` (a record at or below the last one read is dropped, so none is fed twice) and hands them
//! to [`pty::coalesce`], which feeds the terminal's stream. A connection the host dropped (the
//! server read too slowly) is attached again after the last `seq` read. `exit`, or a host that
//! is gone, ends the terminal through `on_exit`. Input and resize are lines on the same
//! connection, written whole by a thread of their own ([`Outbox`]); `kill` goes on a connection
//! of its own.
//!
//! A restarted server adopts the hosts of the terminal list in `server.json` (#604): [`reach`]
//! connects to a host that already runs and [`adopt`] reads it from the start of its ring, the
//! records up to its `hello` as a replay ([`Chunk::Replay`]). A `gap` becomes a marker line in
//! the grid ([`gap_marker`]).

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use poimen_client::transport::{self, Conn, Entry};
use poimen_protocol::host::{ClientMessage, HostMessage, HostStart, Size, SpawnSpec, to_line};
use poimen_protocol::{CHANNEL, Channel, ENV_SOCKET, ENV_TERMINAL_ID, decode_bytes, encode_bytes};

use crate::pty::{self, Chunk, PtyWriter};

/// How long a started host may take to answer with `hello`.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// Input queued for a host and not yet written; a writer past it waits (outside the global lock).
const INPUT_QUEUE_BYTES: usize = 1024 * 1024;
/// A write or read on the connection of `kill` gives up after this (Unix; pipes have no
/// timeouts). Longer than the host's wait for a killed child.
const KILL_IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Bytes read from a host and not yet fed to the stream. The host never waits for its client (it
/// drops one that falls behind), so the reader keeps reading into this queue while the emulator
/// catches up; only past it the reader waits, the host drops the connection, and it is attached
/// again after the last `seq` read (with a `gap` when the host's ring moved on meanwhile).
const QUEUE_BYTES: usize = 64 * 1024 * 1024;
/// How long a host the server adopts may take to answer with `hello`.
pub(crate) const ADOPT_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a reader at [`QUEUE_BYTES`] looks again.
const QUEUE_POLL: Duration = Duration::from_millis(2);
/// Reconnects in a row that bring no new record before the host counts as gone.
const MAX_IDLE_RECONNECTS: u32 = 3;

/// Starts a host for a [`HostStart`] in this process (tests); `Ok` once its endpoint is bound.
pub type Launch = Arc<dyn Fn(&HostStart) -> Result<(), String> + Send + Sync>;

/// How the server starts the host of a new terminal.
#[derive(Clone)]
pub enum PtyHost {
    /// Run this `poimen-pty-host` binary (the one next to the server, [`PtyHost::beside_server`]).
    Binary(PathBuf),
    /// Start the host through the function, without a process of its own (tests: an in-process
    /// host speaking the same protocol).
    Launch(Launch),
}

impl fmt::Debug for PtyHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtyHost::Binary(path) => f.debug_tuple("Binary").field(path).finish(),
            PtyHost::Launch(_) => f.write_str("Launch(..)"),
        }
    }
}

impl PtyHost {
    /// The host binary next to the running server ([`host_beside`]); a server whose own path is
    /// unknown gets a bare file name, which `terminal.create` reports as missing.
    pub fn beside_server() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        PtyHost::Binary(host_beside(&exe, cfg!(windows), CHANNEL))
    }

    /// A [`PtyHost::Launch`] from a function.
    pub fn launch(f: impl Fn(&HostStart) -> Result<(), String> + Send + Sync + 'static) -> Self {
        PtyHost::Launch(Arc::new(f))
    }
}

/// `poimen-pty-host` (`poimen-pty-host-rc` on rc, `.exe` on Windows) in the directory of the
/// server binary `exe`.
pub fn host_beside(exe: &Path, windows: bool, channel: Channel) -> PathBuf {
    let name = channel.host_program(windows);
    match exe.parent() {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

/// What the server starts in a host.
pub(crate) struct Spec<'a> {
    pub terminal_id: &'a str,
    /// The server's socket, for `POIMEN_SOCKET` of the child; the host endpoints live next to it.
    pub socket: &'a Path,
    pub command: &'a [String],
    pub cwd: &'a str,
    pub cols: u16,
    pub rows: u16,
    /// The server's `TERM`, which the child inherits through the host.
    pub term: Option<&'a str>,
}

/// The endpoint of the host of `terminal_id`: `hosts/<id>` next to the server's socket (shorter
/// than the socket's own name for the usual ids, so it fits where the socket fits).
pub(crate) fn endpoint_for(socket: &Path, terminal_id: &str) -> PathBuf {
    socket
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("hosts")
        .join(terminal_id)
}

/// The start line of a host: the command, its directory and size, and the environment poimen
/// adds (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID`, `TERM=xterm-256color` when the server has none
/// or a dumb one).
pub(crate) fn start_line(spec: &Spec<'_>, endpoint: PathBuf) -> HostStart {
    let mut env = std::collections::BTreeMap::new();
    if spec.term.is_none_or(|t| t.is_empty() || t == "dumb") {
        env.insert("TERM".to_string(), "xterm-256color".to_string());
    }
    env.insert(
        ENV_SOCKET.to_string(),
        spec.socket.to_string_lossy().into_owned(),
    );
    env.insert(ENV_TERMINAL_ID.to_string(), spec.terminal_id.to_string());
    HostStart {
        endpoint,
        spawn: SpawnSpec {
            argv: spec.command.to_vec(),
            cwd: spec.cwd.to_string(),
            env,
            cols: spec.cols,
            rows: spec.rows,
        },
        orphan_timeout_ms: poimen_protocol::host::DEFAULT_ORPHAN_TIMEOUT_MS,
        ring_bytes: poimen_protocol::host::DEFAULT_RING_BYTES,
        exit_linger_ms: poimen_protocol::host::DEFAULT_EXIT_LINGER_MS,
    }
}

/// The connection to a host, shared by the reader (which replaces it on a reconnect) and the
/// [`Outbox`]'s writer thread.
#[derive(Clone)]
struct HostConn(Arc<Mutex<Conn>>);

impl HostConn {
    fn current(&self) -> Conn {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn replace(&self, conn: Conn) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = conn;
    }
}

/// Ends the child of the host at `endpoint` over a connection of its own, so a `kill` never
/// waits behind input the host has not taken yet. The host takes the new connection as its
/// client (the reader's gets `replaced`) and handles `kill` on it. The connection attaches after
/// `since` too, and a thread reads it up to `exit`, so the host can end at once instead of
/// keeping the exit for a client.
fn send_kill(endpoint: &Path, since: u64) {
    let Ok(conn) = Conn::connect(endpoint) else {
        return;
    };
    // Two short lines on a fresh connection; the timeouts only guard against a wedged host.
    let _ = conn.set_write_timeout(Some(KILL_IO_TIMEOUT));
    let _ = conn.set_read_timeout(Some(KILL_IO_TIMEOUT));
    let lines = [
        to_line(&ClientMessage::Attach { since }),
        to_line(&ClientMessage::Kill),
    ]
    .concat();
    if (&conn).write_all(&lines).is_err() {
        return;
    }
    thread::spawn(move || {
        let mut lines = BufReader::new(conn);
        let mut line = String::new();
        loop {
            line.clear();
            match lines.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if matches!(
                poimen_protocol::host::parse_host_line(&line),
                Ok(HostMessage::Exit { .. } | HostMessage::Replaced)
            ) {
                return;
            }
        }
    });
}

#[derive(Default)]
struct Pending {
    input: VecDeque<Vec<u8>>,
    /// Bytes in `input` ([`INPUT_QUEUE_BYTES`]).
    queued: usize,
    /// The latest size asked for and not yet written; an older one is superseded.
    resize: Option<(u16, u16)>,
    closed: bool,
}

/// What the server sends a host over its connection, written by one thread per terminal
/// ([`Outbox::run`]): every line is written whole, without a timeout, so the connection never
/// holds half a line. Callers never wait for the host: a resize only replaces the pending size,
/// input waits only while [`INPUT_QUEUE_BYTES`] are queued (outside the global lock, like a
/// blocking PTY write).
struct Outbox {
    pending: Mutex<Pending>,
    changed: Condvar,
}

impl Outbox {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(Pending::default()),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn input(&self, data: &[u8]) -> io::Result<()> {
        let mut pending = self.lock();
        while !pending.closed
            && pending.queued > 0
            && pending.queued + data.len() > INPUT_QUEUE_BYTES
        {
            pending = self
                .changed
                .wait(pending)
                .unwrap_or_else(|e| e.into_inner());
        }
        if pending.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the terminal has ended",
            ));
        }
        pending.queued += data.len();
        pending.input.push_back(data.to_vec());
        self.changed.notify_all();
        Ok(())
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let mut pending = self.lock();
        if pending.closed {
            return Err("the terminal has ended".into());
        }
        pending.resize = Some((cols, rows));
        self.changed.notify_all();
        Ok(())
    }

    /// Nothing more is written; queued input is dropped and waiting writers return an error.
    fn close(&self) {
        let mut pending = self.lock();
        pending.closed = true;
        pending.input.clear();
        pending.queued = 0;
        pending.resize = None;
        self.changed.notify_all();
    }

    /// The next line to write (a pending resize first), `None` once closed.
    fn next(&self) -> Option<ClientMessage> {
        let mut pending = self.lock();
        loop {
            if pending.closed {
                return None;
            }
            if let Some((cols, rows)) = pending.resize.take() {
                return Some(ClientMessage::Resize { cols, rows });
            }
            if let Some(data) = pending.input.pop_front() {
                pending.queued -= data.len();
                self.changed.notify_all();
                return Some(ClientMessage::Input {
                    bytes: encode_bytes(&data),
                });
            }
            pending = self
                .changed
                .wait(pending)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Writes the lines to the current connection until closed. A failed write means the
    /// connection is gone (the reader attaches again); its line is dropped.
    fn run(&self, conn: &HostConn) {
        while let Some(message) = self.next() {
            let _ = (&conn.current()).write_all(&to_line(&message));
        }
    }
}

/// The input side for [`PtyWriter`]: every write is one `input` line.
struct InputLines(Arc<Outbox>);

impl Write for InputLines {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.input(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A terminal's host as the server keeps it: the endpoint, the last `seq` read, the connection
/// and the outbox for input and resize. Dropping it stops the writing.
pub(crate) struct Handle {
    endpoint: PathBuf,
    read: Arc<AtomicU64>,
    outbox: Arc<Outbox>,
    closing: Arc<AtomicBool>,
    conn: HostConn,
}

impl Handle {
    /// Where the host listens.
    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }

    /// The `seq` of the last output record read from the host (0: none yet).
    pub fn seq(&self) -> u64 {
        self.read.load(Ordering::SeqCst)
    }

    pub fn writer(&self) -> PtyWriter {
        PtyWriter::new(Box::new(InputLines(Arc::clone(&self.outbox))))
    }

    /// Queues the size for the host; never waits for it.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.outbox
            .resize(cols, rows)
            .map_err(|e| format!("resize did not reach the host: {e}"))
    }

    /// Ends the child over a connection of its own ([`send_kill`]), so neither a blocked input
    /// write nor the global lock waits on the input stream; the host ends. The reader stops
    /// reconnecting.
    pub fn kill(self) {
        self.closing.store(true, Ordering::SeqCst);
        self.outbox.close();
        send_kill(&self.endpoint, self.read.load(Ordering::SeqCst));
    }

    /// Lets the host go without ending it (`server.shutdown` with `keep_terminals`): the server
    /// stops writing and reading, the host keeps the child and buffers its output for the next
    /// server. Nothing is reported as an exit.
    pub fn release(self) {
        self.closing.store(true, Ordering::SeqCst);
        self.outbox.close();
        let _ = self.conn.current().shutdown();
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.outbox.close();
    }
}

/// Starts the host of a terminal and reads it: `on_output` gets the output in order (`live`
/// false for a replay, see [`Chunk::Replay`]), `on_exit` the exit code (`None`: unknown, or the
/// host is gone) once all output before it was handed on. `Err` with a message for
/// `terminal.create` when the host cannot be started or reached.
pub(crate) fn start(
    host: &PtyHost,
    spec: &Spec<'_>,
    on_output: impl FnMut(&[u8], bool) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
) -> Result<Handle, String> {
    let endpoint = endpoint_for(spec.socket, spec.terminal_id);
    prepare_endpoint(&endpoint)?;
    let start = start_line(spec, endpoint.clone());
    let process = launch(host, &start)?;
    let attached = attach(&endpoint, 0);
    if let Some(mut child) = process {
        if attached.is_err() {
            let _ = child.kill();
        }
        // Reaped when it ends; the host outlives a connection, never its own child.
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let (conn, lines) = attached.map_err(|e| format!("could not reach the host: {e}"))?;
    Ok(follow(endpoint, conn, lines, 0, on_output, on_exit))
}

/// A host that already ran when the server started, connected and attached from the start of
/// its ring ([`reach`]), ready to [`adopt`].
pub(crate) struct Reached {
    endpoint: PathBuf,
    conn: Conn,
    lines: BufReader<Conn>,
    /// The host's `seq` in its `hello`: the records up to it ran before this server.
    pub seq: u64,
    /// The PTY size in its `hello`.
    pub size: Size,
}

/// Connects to the running host at `endpoint`, reads its `hello` (within `timeout`) and attaches
/// with `since: 0`, so the whole ring is replayed. `min_seq` is the last `seq` the previous
/// server read from it: a host behind it is another one. `Err` with the reason when the host does
/// not answer.
pub(crate) fn reach(endpoint: &Path, min_seq: u64, timeout: Duration) -> Result<Reached, String> {
    let (tx, rx) = mpsc::channel();
    let target = endpoint.to_path_buf();
    // On a thread: a host that accepts but never answers (and a pipe has no read timeout) must
    // not hold up the start of the server.
    thread::spawn(move || {
        let _ = tx.send(hello_of(&target));
    });
    let (conn, lines, seq, size) = match rx.recv_timeout(timeout) {
        Ok(result) => result.map_err(|e| e.to_string())?,
        Err(_) => {
            return Err(format!("no hello within {} ms", timeout.as_millis()));
        }
    };
    if seq < min_seq {
        return Err(format!(
            "the host is at seq {seq}, behind the {min_seq} read from it before (another host)"
        ));
    }
    (&conn)
        .write_all(&to_line(&ClientMessage::Attach { since: 0 }))
        .map_err(|e| format!("could not attach: {e}"))?;
    Ok(Reached {
        endpoint: endpoint.to_path_buf(),
        conn,
        lines,
        seq,
        size,
    })
}

impl Reached {
    /// Ends the child of a host the server does not adopt (its terminal has no space any more).
    pub fn kill(self) {
        let _ = (&self.conn).write_all(&to_line(&ClientMessage::Kill));
    }
}

type HelloOf = io::Result<(Conn, BufReader<Conn>, u64, Size)>;

/// Connects and reads the `hello` line.
fn hello_of(endpoint: &Path) -> HelloOf {
    let conn = Conn::connect(endpoint)?;
    let mut lines = BufReader::new(conn.clone());
    let mut line = String::new();
    lines.read_line(&mut line)?;
    match poimen_protocol::host::parse_host_line(&line) {
        Ok(HostMessage::Hello { seq, size, .. }) => Ok((conn, lines, seq, size)),
        _ => Err(io::Error::other(format!("no hello: {:?}", line.trim_end()))),
    }
}

/// Reads a [`reach`]ed host as [`start`] reads a new one; the records up to its `hello` go to
/// `on_output` as a replay (`live` false), so their queries are not answered a second time.
pub(crate) fn adopt(
    reached: Reached,
    on_output: impl FnMut(&[u8], bool) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
) -> Handle {
    let Reached {
        endpoint,
        conn,
        lines,
        seq,
        ..
    } = reached;
    follow(endpoint, conn, lines, seq, on_output, on_exit)
}

/// Starts the threads that read and write an attached host connection.
fn follow(
    endpoint: PathBuf,
    conn: Conn,
    lines: BufReader<Conn>,
    replay_until: u64,
    on_output: impl FnMut(&[u8], bool) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
) -> Handle {
    let conn = HostConn(Arc::new(Mutex::new(conn)));
    let handle = Handle {
        endpoint,
        read: Arc::new(AtomicU64::new(0)),
        outbox: Outbox::new(),
        closing: Arc::new(AtomicBool::new(false)),
        conn: conn.clone(),
    };
    let outbox = Arc::clone(&handle.outbox);
    let writing = conn.clone();
    thread::spawn(move || outbox.run(&writing));
    let (tx, rx) = mpsc::channel::<Chunk>();
    let queued = Arc::new(AtomicUsize::new(0));
    let reader = Reader {
        feed: Feed {
            endpoint: handle.endpoint.clone(),
            read: Arc::clone(&handle.read),
            tx,
            queued: Arc::clone(&queued),
            replay_until,
        },
        conn,
        closing: Arc::clone(&handle.closing),
    };
    thread::spawn(move || reader.run(lines));
    let mut on_output = on_output;
    thread::spawn(move || {
        let code = pty::coalesce(&rx, |batch, live| {
            on_output(batch, live);
            queued.fetch_sub(batch.len(), Ordering::SeqCst);
        });
        on_exit(code);
    });
    handle
}

/// The line a `gap` (records `from..=to` left the host's ring before this server read them)
/// leaves in the grid, so the loss is shown and not hidden. After earlier output it starts on a
/// line of its own.
pub(crate) fn gap_marker(from: u64, to: u64, after_output: bool) -> Vec<u8> {
    let lead = if after_output { "\r\n" } else { "" };
    let records = to.saturating_sub(from).saturating_add(1);
    let what = if records == 1 { "record" } else { "records" };
    format!("{lead}[poimen: earlier output lost, {records} {what} left the host's buffer]\r\n")
        .into_bytes()
}

/// Creates the private `hosts` directory and removes a stale entry at the endpoint (a host of an
/// earlier server that ended); a host that still answers there is an error.
fn prepare_endpoint(endpoint: &Path) -> Result<(), String> {
    if let Some(dir) = endpoint.parent().filter(|d| !d.as_os_str().is_empty()) {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    match transport::inspect(endpoint) {
        Ok(Entry::Socket) if transport::is_listening(endpoint) => {
            Err(format!("a host already listens on {}", endpoint.display()))
        }
        Ok(Entry::Socket) => std::fs::remove_file(endpoint)
            .map_err(|e| format!("could not remove {}: {e}", endpoint.display())),
        _ => Ok(()),
    }
}

/// Starts the host; for a binary also the process, to reap it (or kill it when it cannot be
/// reached).
fn launch(host: &PtyHost, start: &HostStart) -> Result<Option<std::process::Child>, String> {
    match host {
        PtyHost::Launch(f) => f(start).map(|()| None),
        PtyHost::Binary(program) => {
            if !program.is_file() {
                return Err(format!(
                    "poimen-pty-host is missing: {} (it ships next to poimen-server)",
                    program.display()
                ));
            }
            run_binary(program, start).map(Some)
        }
    }
}

/// Runs the host binary in its own process group without a terminal, writes the start line and
/// waits for `hello` (or the host's `error`) on its stdout.
#[allow(clippy::disallowed_methods)] // CREATE_NO_WINDOW is set below; gui_path is the app's
fn run_binary(program: &Path, start: &HostStart) -> Result<std::process::Child, String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Signals to the server's group (a Ctrl+C in the terminal that started it) do not reach
        // the host, which is meant to outlive the server.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x0000_0200 | 0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    let fail = |child: &mut std::process::Child, message: String| {
        let _ = child.kill();
        let _ = child.wait();
        Err(message)
    };
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(&to_line(start)));
    if let Some(Err(e)) = written {
        return fail(&mut child, format!("could not start the host: {e}"));
    }
    let Some(stdout) = child.stdout.take() else {
        return fail(&mut child, "the host has no stdout".into());
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(first_line(stdout));
    });
    let line = match rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => return fail(&mut child, format!("the host did not answer: {e}")),
        Err(_) => {
            let message = format!(
                "the host did not answer within {} s",
                START_TIMEOUT.as_secs()
            );
            return fail(&mut child, message);
        }
    };
    match poimen_protocol::host::parse_host_line(&line) {
        Ok(HostMessage::Hello { .. }) => Ok(child),
        Ok(HostMessage::Error { message }) => fail(&mut child, message),
        _ => fail(
            &mut child,
            format!("the host answered {:?}", line.trim_end()),
        ),
    }
}

fn first_line(input: impl Read) -> io::Result<String> {
    let mut line = String::new();
    BufReader::new(input).read_line(&mut line)?;
    if line.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "it ended without a line",
        ));
    }
    Ok(line)
}

/// Connects to the host, reads its `hello` and attaches after record `since`.
fn attach(endpoint: &Path, since: u64) -> io::Result<(Conn, BufReader<Conn>)> {
    let conn = Conn::connect(endpoint)?;
    let mut lines = BufReader::new(conn.clone());
    let mut line = String::new();
    lines.read_line(&mut line)?;
    match poimen_protocol::host::parse_host_line(&line) {
        // A host behind `since` is another one than the one read so far.
        Ok(HostMessage::Hello { seq, .. }) if seq >= since => {}
        Ok(HostMessage::Hello { seq, .. }) => {
            return Err(io::Error::other(format!(
                "the host at {} is at seq {seq}, behind {since}",
                endpoint.display()
            )));
        }
        _ => {
            return Err(io::Error::other(format!(
                "no hello from the host at {}: {:?}",
                endpoint.display(),
                line.trim_end()
            )));
        }
    }
    (&conn).write_all(&to_line(&ClientMessage::Attach { since }))?;
    Ok((conn, lines))
}

/// How a connection to a host ended.
#[derive(Debug, PartialEq, Eq)]
enum Ended {
    /// `exit` with the child's code.
    Exit(Option<u32>),
    /// The connection ended without `exit` (dropped, or the host died).
    Lost,
    /// Another client took the host over.
    Replaced,
    /// The stream side is gone; nobody reads any more.
    Stopped,
}

/// Where the records of a host go: the last `seq` read and the queue to the stream.
struct Feed {
    endpoint: PathBuf,
    read: Arc<AtomicU64>,
    tx: mpsc::Sender<Chunk>,
    /// Bytes in `tx` not yet fed ([`QUEUE_BYTES`]).
    queued: Arc<AtomicUsize>,
    /// Records up to this `seq` ran before this server read the host: a replay.
    replay_until: u64,
}

struct Reader {
    feed: Feed,
    conn: HostConn,
    closing: Arc<AtomicBool>,
}

impl Reader {
    /// Reads the host until the terminal ends, attaching again after a dropped connection.
    fn run(self, mut lines: BufReader<Conn>) {
        let feed = &self.feed;
        let mut idle = 0;
        let code = loop {
            let before = feed.read.load(Ordering::SeqCst);
            match feed.pump(&mut lines) {
                Ended::Exit(code) => break code,
                Ended::Stopped => return,
                Ended::Replaced => break None,
                Ended::Lost if self.closing.load(Ordering::SeqCst) => break None,
                Ended::Lost => {}
            }
            idle = if feed.read.load(Ordering::SeqCst) == before {
                idle + 1
            } else {
                0
            };
            if idle > MAX_IDLE_RECONNECTS {
                break None;
            }
            match attach(&feed.endpoint, feed.read.load(Ordering::SeqCst)) {
                Ok((conn, next)) => {
                    self.conn.replace(conn);
                    lines = next;
                    // A close while attaching: this connection may have replaced the one `kill`
                    // went on before the host read it.
                    if self.closing.load(Ordering::SeqCst) {
                        send_kill(&feed.endpoint, feed.read.load(Ordering::SeqCst));
                        break None;
                    }
                }
                Err(_) => break None,
            }
        };
        let _ = feed.tx.send(Chunk::End(code));
    }
}

impl Feed {
    /// Reads one connection: output records after the last `seq` read go on, older ones are
    /// dropped.
    fn pump(&self, lines: &mut impl BufRead) -> Ended {
        let mut line = String::new();
        loop {
            line.clear();
            match lines.read_line(&mut line) {
                Ok(0) | Err(_) => return Ended::Lost,
                Ok(_) => {}
            }
            let Ok(message) = poimen_protocol::host::parse_host_line(&line) else {
                continue;
            };
            match message {
                HostMessage::Output { seq, bytes } => {
                    if seq <= self.read.load(Ordering::SeqCst) {
                        continue;
                    }
                    let Ok(data) = decode_bytes(&bytes) else {
                        continue;
                    };
                    while self.queued.load(Ordering::SeqCst) >= QUEUE_BYTES {
                        thread::sleep(QUEUE_POLL);
                    }
                    self.read.store(seq, Ordering::SeqCst);
                    self.queued.fetch_add(data.len(), Ordering::SeqCst);
                    if self.tx.send(self.chunk(seq, data)).is_err() {
                        return Ended::Stopped;
                    }
                }
                HostMessage::Gap { from, to } => {
                    // A gap at or before the last record read is no news (a reattach repeats it).
                    if to <= self.read.load(Ordering::SeqCst) {
                        continue;
                    }
                    let after_output = self.read.load(Ordering::SeqCst) > 0;
                    let marker = gap_marker(from, to, after_output);
                    self.queued.fetch_add(marker.len(), Ordering::SeqCst);
                    if self.tx.send(self.chunk(to, marker)).is_err() {
                        return Ended::Stopped;
                    }
                }
                HostMessage::Exit { code } => return Ended::Exit(code),
                HostMessage::Replaced => return Ended::Replaced,
                HostMessage::Error { message } => {
                    eprintln!("poimen-server: host {}: {message}", self.endpoint.display());
                }
                // Not for here.
                HostMessage::Hello { .. } | HostMessage::Unknown => {}
            }
        }
    }

    /// Output of record `seq`: a replay when it ran before this server read the host.
    fn chunk(&self, seq: u64, data: Vec<u8>) -> Chunk {
        if seq <= self.replay_until {
            Chunk::Replay(data)
        } else {
            Chunk::Data(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_binary_is_looked_for_next_to_the_server() {
        let exe = Path::new("/opt/poimen/poimen-server");
        assert_eq!(
            host_beside(exe, false, Channel::Stable),
            PathBuf::from("/opt/poimen/poimen-pty-host")
        );
        assert_eq!(
            host_beside(exe, false, Channel::Rc),
            PathBuf::from("/opt/poimen/poimen-pty-host-rc")
        );
        assert_eq!(
            host_beside(Path::new(r"C:\p\poimen-server.exe"), true, Channel::Stable)
                .file_name()
                .unwrap(),
            "poimen-pty-host.exe"
        );
        assert_eq!(
            host_beside(Path::new(""), false, Channel::Stable),
            PathBuf::from("poimen-pty-host")
        );
    }

    #[test]
    fn the_endpoint_is_in_the_hosts_directory_next_to_the_socket() {
        assert_eq!(
            endpoint_for(Path::new("/run/u/poimen/server.sock"), "t12"),
            PathBuf::from("/run/u/poimen/hosts/t12")
        );
    }

    fn spec<'a>(command: &'a [String], term: Option<&'a str>) -> Spec<'a> {
        Spec {
            terminal_id: "t3",
            socket: Path::new("/run/s.sock"),
            command,
            cwd: "/w",
            cols: 100,
            rows: 30,
            term,
        }
    }

    #[test]
    fn the_start_line_carries_the_command_and_poimens_environment() {
        let command = vec!["sh".to_string(), "-l".to_string()];
        let start = start_line(&spec(&command, Some("xterm-kitty")), PathBuf::from("/e"));
        assert_eq!(start.endpoint, PathBuf::from("/e"));
        assert_eq!(start.spawn.argv, command);
        assert_eq!(start.spawn.cwd, "/w");
        assert_eq!((start.spawn.cols, start.spawn.rows), (100, 30));
        assert_eq!(
            start.spawn.env,
            std::collections::BTreeMap::from([
                ("POIMEN_SOCKET".to_string(), "/run/s.sock".to_string()),
                ("POIMEN_TERMINAL_ID".to_string(), "t3".to_string()),
            ])
        );
        assert_eq!(start.orphan_timeout_ms, 86_400_000);
    }

    #[test]
    fn a_missing_or_dumb_term_becomes_xterm_256color() {
        let command = vec!["sh".to_string()];
        for term in [None, Some(""), Some("dumb")] {
            let start = start_line(&spec(&command, term), PathBuf::from("/e"));
            assert_eq!(
                start.spawn.env.get("TERM").map(String::as_str),
                Some("xterm-256color"),
                "{term:?}"
            );
        }
    }

    #[test]
    fn a_missing_host_binary_is_a_clear_error() {
        let host = PtyHost::Binary(PathBuf::from("/nonexistent/poimen-pty-host"));
        let start = start_line(&spec(&["sh".into()], None), PathBuf::from("/e"));
        let err = launch(&host, &start).unwrap_err();
        assert_eq!(
            err,
            "poimen-pty-host is missing: /nonexistent/poimen-pty-host (it ships next to poimen-server)"
        );
    }

    fn reader(read: u64) -> (Feed, mpsc::Receiver<Chunk>) {
        let (tx, rx) = mpsc::channel();
        let feed = Feed {
            endpoint: PathBuf::from("/e"),
            read: Arc::new(AtomicU64::new(read)),
            tx,
            queued: Arc::default(),
            replay_until: 0,
        };
        (feed, rx)
    }

    fn output(seq: u64, text: &str) -> String {
        String::from_utf8(to_line(&HostMessage::Output {
            seq,
            bytes: encode_bytes(text.as_bytes()),
        }))
        .unwrap()
    }

    fn data(rx: &mpsc::Receiver<Chunk>) -> Vec<String> {
        rx.try_iter()
            .map(|c| match c {
                Chunk::Data(d) => String::from_utf8(d).unwrap(),
                Chunk::Replay(d) => format!("replay {}", String::from_utf8(d).unwrap()),
                Chunk::End(code) => format!("end {code:?}"),
            })
            .collect()
    }

    #[test]
    fn records_are_read_strictly_by_seq_and_none_twice() {
        let (reader, rx) = reader(0);
        let lines = [
            output(1, "a"),
            output(2, "b"),
            output(2, "b"),
            output(1, "a"),
            output(3, "c"),
        ]
        .concat();
        let ended = reader.pump(&mut lines.as_bytes());
        assert_eq!(ended, Ended::Lost);
        assert_eq!(data(&rx), ["a", "b", "c"]);
        assert_eq!(reader.read.load(Ordering::SeqCst), 3);
    }

    fn gap(from: u64, to: u64) -> String {
        String::from_utf8(to_line(&HostMessage::Gap { from, to })).unwrap()
    }

    #[test]
    fn a_reattached_connection_goes_on_after_the_last_seq_read_and_marks_the_gap() {
        let (reader, rx) = reader(5);
        let lines = [
            output(4, "old"),
            output(5, "old"),
            gap(6, 7),
            output(8, "new"),
        ]
        .concat();
        reader.pump(&mut lines.as_bytes());
        assert_eq!(
            data(&rx),
            [
                "\r\n[poimen: earlier output lost, 2 records left the host's buffer]\r\n",
                "new"
            ]
        );
        assert_eq!(reader.read.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn a_gap_already_behind_the_last_record_read_leaves_no_marker() {
        let (reader, rx) = reader(9);
        let lines = [gap(1, 7), output(10, "x")].concat();
        reader.pump(&mut lines.as_bytes());
        assert_eq!(data(&rx), ["x"]);
    }

    #[test]
    fn the_ring_of_an_adopted_host_is_a_replay_up_to_its_hello_and_live_after() {
        let (mut reader, rx) = reader(0);
        reader.replay_until = 3;
        let lines = [gap(1, 1), output(2, "b"), output(3, "c"), output(4, "d")].concat();
        reader.pump(&mut lines.as_bytes());
        assert_eq!(
            data(&rx),
            [
                "replay [poimen: earlier output lost, 1 record left the host's buffer]\r\n",
                "replay b",
                "replay c",
                "d"
            ]
        );
    }

    #[test]
    fn exit_replaced_and_noise_end_or_pass_as_documented() {
        let (reader, rx) = reader(0);
        let exit = String::from_utf8(to_line(&HostMessage::Exit { code: Some(7) })).unwrap();
        let lines = [
            "not json\n".to_string(),
            "{\"type\":\"title\",\"text\":\"x\"}\n".to_string(),
            output(1, "x"),
            exit,
            output(2, "after"),
        ]
        .concat();
        assert_eq!(reader.pump(&mut lines.as_bytes()), Ended::Exit(Some(7)));
        assert_eq!(data(&rx), ["x"]);
        let replaced = String::from_utf8(to_line(&HostMessage::Replaced)).unwrap();
        assert_eq!(reader.pump(&mut replaced.as_bytes()), Ended::Replaced);
    }
}
