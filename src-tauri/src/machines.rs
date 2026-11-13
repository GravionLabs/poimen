//! Remote machines (ADR 0008, #279): the `machines[]` of the user config and one connection per
//! machine to its `poimen-server`, reached with `ssh -T -o BatchMode=yes <target> poimen-server
//! stdio`.
//!
//! Each machine has a supervisor thread that holds one long-lived connection (`ping`, then
//! `events.subscribe` on the same connection) and reports its state: `connecting`, `connected`,
//! `offline` (with the ssh error) or `incompatible` (other protocol; no further requests). A
//! dropped connection is tried again with a backoff of 1 s doubling up to 60 s, or at once on
//! demand ([`Machine::reconnect`]); an incompatible server only on demand.
//!
//! Requests of the machine go through a [`PoimenBackend`] whose connections are gated: none is
//! opened while the machine is not `connected`, and a connection of an earlier session fails its
//! reads and writes, so nothing is sent to a terminal while the machine is not connected and no
//! request of before a reconnect is answered or replayed. A lost connection only ends poimen's own
//! ssh processes; the terminals on the remote keep running and are attached again from the
//! snapshot once the machine is back.
//!
//! BatchMode only: poimen never prompts for, stores or logs a password or a key.

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use poimen_client::Client;
use poimen_client::pipe::{Connector, Pipe, WeakPipe};
use poimen_client::protocol::{self as wire, Event, PROTOCOL_VERSION, PingResult};
use poimen_core::gui_path;
use serde::Serialize;
use serde_json::Value;

use crate::backend::{Backend, BackendEvent};
use crate::poimen_backend::{PoimenBackend, backend_event};

/// `reposDir` when a machine sets none.
pub const DEFAULT_REPOS_DIR: &str = "~/Source/{owner}/{repo}";

/// First wait after a failed or dropped connection; it doubles up to [`MAX_BACKOFF`].
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// How much of ssh's stderr is kept for the error of a failed connection.
const MAX_SSH_ERROR: usize = 4096;

/// One entry of `machines[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineConfig {
    /// Unique among the machines.
    pub name: String,
    /// The ssh target as in `~/.ssh/config` (a `Host` alias or `user@host`).
    pub ssh: String,
    /// Where repositories live on the machine (`{owner}`, `{repo}`).
    pub repos_dir: String,
}

/// The machines of `machines[]`; an invalid entry is a warning and is left out, the rest loads.
pub fn parse(value: &Value, warnings: &mut Vec<String>) -> Vec<MachineConfig> {
    let Some(entries) = value.as_array() else {
        warnings.push("machines must be a list; no machines".into());
        return Vec::new();
    };
    let mut names = HashSet::new();
    let mut machines = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match parse_entry(entry) {
            Ok(m) if !names.insert(m.name.clone()) => warnings.push(format!(
                "machines[{i}]: the name {:?} is already used; ignored",
                m.name
            )),
            Ok(m) => machines.push(m),
            Err(e) => warnings.push(format!("machines[{i}]: {e}; ignored")),
        }
    }
    machines
}

fn parse_entry(entry: &Value) -> Result<MachineConfig, String> {
    let Some(obj) = entry.as_object() else {
        return Err("must be an object".into());
    };
    let text = |key: &str| match obj.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_owned())),
        None | Some(Value::Null) => Ok(None),
        Some(_) => Err(format!("{key} must be a non-empty string")),
    };
    let name = text("name")?.ok_or("name is missing")?;
    let ssh = text("ssh")?.ok_or("ssh is missing")?;
    check_target(&ssh)?;
    let repos_dir = text("reposDir")?.unwrap_or_else(|| DEFAULT_REPOS_DIR.to_owned());
    check_repos_dir(&repos_dir)?;
    Ok(MachineConfig {
        name,
        ssh,
        repos_dir,
    })
}

/// A target is one ssh argument: no option (`-...`), no whitespace or control characters.
fn check_target(ssh: &str) -> Result<(), String> {
    if ssh.starts_with('-') {
        return Err(format!("ssh {ssh:?} must be a host, not an option"));
    }
    if ssh.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("ssh {ssh:?} must be one host without spaces"));
    }
    Ok(())
}

/// `reposDir` may only use the placeholders `{owner}` and `{repo}`.
fn check_repos_dir(dir: &str) -> Result<(), String> {
    let mut rest = dir;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(format!("reposDir {dir:?} has an unclosed {{"));
        };
        let name = &after[..close];
        if !matches!(name, "owner" | "repo") {
            return Err(format!(
                "reposDir {dir:?} uses {{{name}}}; only {{owner}} and {{repo}} are known"
            ));
        }
        rest = &after[close + 1..];
    }
    Ok(())
}

/// The connection state of a machine (`state` of [`MachineStatus`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum State {
    Connecting,
    Connected {
        server_version: String,
    },
    /// Not reachable; `error` is what ssh (or the connection) said.
    Offline {
        error: String,
    },
    /// The server speaks another protocol; it gets no further requests.
    Incompatible {
        server_version: String,
        server_protocol: u32,
        app_version: String,
        app_protocol: u32,
    },
}

/// One machine with its state: an entry of `list_machines` and the payload of
/// `machine:state_changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineStatus {
    #[serde(flatten)]
    pub config: MachineConfig,
    #[serde(flatten)]
    pub state: State,
}

/// Waits after a failed connection: 1 s, doubling up to 60 s; reset by a connection.
#[derive(Debug, Default)]
struct Backoff {
    next: Option<Duration>,
}

impl Backoff {
    fn next(&mut self) -> Duration {
        let delay = self.next.unwrap_or(FIRST_BACKOFF);
        self.next = Some((delay * 2).min(MAX_BACKOFF));
        delay
    }

    fn reset(&mut self) {
        self.next = None;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The state, the session counter and the open connections of one machine.
struct Shared {
    /// The state and the number of the session it belongs to (raised by every connection).
    status: Mutex<(State, u64)>,
    stopped: AtomicBool,
    woken: Mutex<bool>,
    wake: Condvar,
    /// Every connection opened to the machine, so a drop or a stop can end them.
    pipes: Mutex<Vec<WeakPipe>>,
    on_change: Box<dyn Fn(&State) + Send + Sync>,
    /// Gets every event of the machine's subscription, as the frontend's backend event.
    on_event: Box<dyn Fn(&BackendEvent) + Send + Sync>,
}

impl Shared {
    #[cfg(test)]
    fn new(on_change: impl Fn(&State) + Send + Sync + 'static) -> Self {
        Self::with_events(on_change, |_| {})
    }

    fn with_events(
        on_change: impl Fn(&State) + Send + Sync + 'static,
        on_event: impl Fn(&BackendEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            on_event: Box::new(on_event),
            status: Mutex::new((State::Connecting, 0)),
            stopped: AtomicBool::new(false),
            woken: Mutex::new(false),
            wake: Condvar::new(),
            pipes: Mutex::default(),
            on_change: Box::new(on_change),
        }
    }

    /// Hands one event of the subscription on, unless the machine was stopped.
    fn forward(&self, event: &Event) {
        if !self.stopped() {
            (self.on_event)(&backend_event(event));
        }
    }

    fn state(&self) -> State {
        lock(&self.status).0.clone()
    }

    /// The session number while the machine is connected.
    fn session(&self) -> Option<u64> {
        match &*lock(&self.status) {
            (State::Connected { .. }, n) => Some(*n),
            _ => None,
        }
    }

    fn set(&self, state: State) {
        {
            let mut status = lock(&self.status);
            if matches!(state, State::Connected { .. }) {
                status.1 += 1;
            }
            status.0 = state.clone();
        }
        if !self.stopped() {
            (self.on_change)(&state);
        }
    }

    /// Registers `pipe` so a drop or a stop ends it; a stopped machine closes and refuses it.
    fn track(&self, pipe: &Pipe) -> io::Result<()> {
        let mut pipes = lock(&self.pipes);
        // Checked under the lock: `stop` sets the flag before it takes the list, so a pipe pushed
        // here while not stopped is closed by that stop.
        if self.stopped() {
            drop(pipes);
            pipe.close();
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the machine is stopped",
            ));
        }
        pipes.retain(|p| p.upgrade().is_some());
        pipes.push(pipe.downgrade());
        Ok(())
    }

    /// Ends every connection of the machine (poimen's ssh processes, never a terminal).
    fn close_pipes(&self) {
        let pipes = std::mem::take(&mut *lock(&self.pipes));
        for pipe in pipes.iter().filter_map(WeakPipe::upgrade) {
            pipe.close();
        }
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.close_pipes();
        self.wake_up();
    }

    fn wake_up(&self) {
        *lock(&self.woken) = true;
        self.wake.notify_all();
    }

    /// Waits `delay` (`None`: until woken); true when woken (on demand, or a stop).
    fn wait(&self, delay: Option<Duration>) -> bool {
        let guard = lock(&self.woken);
        let mut guard = match delay {
            Some(d) => {
                self.wake
                    .wait_timeout_while(guard, d, |woken| !*woken)
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            }
            None => self
                .wake
                .wait_while(guard, |woken| !*woken)
                .unwrap_or_else(|e| e.into_inner()),
        };
        std::mem::replace(&mut *guard, false)
    }
}

/// The event stream of a connected session; it ends (or yields an error) with the connection.
type Session = Box<dyn Iterator<Item = Result<Event, String>> + Send>;

/// Opens the supervisor's own connection: `ping`, then the event stream when the protocol fits.
trait Link: Send + Sync {
    fn connect(&self) -> Result<(PingResult, Option<Session>), String>;
}

impl Link for Client {
    fn connect(&self) -> Result<(PingResult, Option<Session>), String> {
        let me = wire::ClientInfo {
            kind: wire::ClientKind::App,
            pid: std::process::id(),
        };
        let (ping, stream) = self
            .ping_and_subscribe(Some(me))
            .map_err(|e| e.to_string())?;
        let session =
            stream.map(|s| -> Session { Box::new(s.map(|r| r.map_err(|e| e.to_string()))) });
        Ok((ping, session))
    }
}

/// How the supervisor waits between attempts (a test pauses without sleeping).
trait Pause: Send + Sync {
    /// Waits `delay` (`None`: until woken); true when woken on demand.
    fn pause(&self, shared: &Shared, delay: Option<Duration>) -> bool;
}

struct RealPause;

impl Pause for RealPause {
    fn pause(&self, shared: &Shared, delay: Option<Duration>) -> bool {
        shared.wait(delay)
    }
}

/// The supervisor loop: connect, follow the session until it drops, wait, again; until stopped.
fn supervise(link: &dyn Link, shared: &Shared, pause: &dyn Pause) {
    let mut backoff = Backoff::default();
    while !shared.stopped() {
        shared.set(State::Connecting);
        let connected = link.connect();
        // Stopped while connecting (a connect can take ssh's ConnectTimeout plus the ping): never
        // connected; its connection was refused by `track`, and the session is dropped unread.
        if shared.stopped() {
            break;
        }
        let delay = match connected {
            Err(error) => {
                shared.set(State::Offline { error });
                Some(backoff.next())
            }
            Ok((ping, Some(session))) if ping.protocol == PROTOCOL_VERSION => {
                shared.set(State::Connected {
                    server_version: ping.version,
                });
                backoff.reset();
                let mut error = "the connection ended".to_owned();
                for item in session {
                    match item {
                        Ok(event) => shared.forward(&event),
                        Err(e) => {
                            error = e;
                            break;
                        }
                    }
                }
                // Offline first, so the gate refuses before the connections end.
                shared.set(State::Offline { error });
                shared.close_pipes();
                Some(backoff.next())
            }
            Ok((ping, _)) => {
                shared.set(State::Incompatible {
                    server_version: ping.version,
                    server_protocol: ping.protocol,
                    app_version: env!("CARGO_PKG_VERSION").into(),
                    app_protocol: PROTOCOL_VERSION,
                });
                None
            }
        };
        if shared.stopped() {
            break;
        }
        if pause.pause(shared, delay) {
            backoff.reset();
        }
    }
}

/// Registers every connection of `inner` with the machine, so a drop or a stop ends it.
struct Tracked {
    inner: Arc<dyn Connector>,
    shared: Arc<Shared>,
}

impl Connector for Tracked {
    fn connect(&self) -> io::Result<Pipe> {
        let pipe = self.inner.connect()?;
        self.shared.track(&pipe)?;
        Ok(pipe)
    }
}

fn not_connected() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "the machine is not connected")
}

/// The connections of the machine's backend: none while the machine is not connected, and one of
/// an earlier session fails every read and write.
struct Gated {
    inner: Tracked,
}

struct GatedReader {
    pipe: Pipe,
    shared: Arc<Shared>,
    session: u64,
}

impl Read for GatedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (&self.pipe).read(buf)?;
        if self.shared.session() != Some(self.session) {
            return Err(not_connected());
        }
        Ok(n)
    }
}

struct GatedWriter {
    pipe: Pipe,
    shared: Arc<Shared>,
    session: u64,
}

impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.shared.session() != Some(self.session) {
            return Err(not_connected());
        }
        (&self.pipe).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.pipe).flush()
    }
}

impl Connector for Gated {
    fn connect(&self) -> io::Result<Pipe> {
        let shared = &self.inner.shared;
        let session = shared.session().ok_or_else(not_connected)?;
        let pipe = self.inner.connect()?;
        let closing = pipe.clone();
        Ok(Pipe::new(
            GatedReader {
                pipe: pipe.clone(),
                shared: shared.clone(),
                session,
            },
            GatedWriter {
                pipe,
                shared: shared.clone(),
                session,
            },
            move || closing.close(),
        ))
    }
}

/// One configured machine: its supervisor and its backend.
pub struct Machine {
    config: MachineConfig,
    shared: Arc<Shared>,
    backend: Arc<PoimenBackend>,
    thread: Option<JoinHandle<()>>,
}

impl Machine {
    /// Starts the supervisor of `config` over `connector` (one ssh process per connection);
    /// `on_change` gets every new state.
    pub fn start(
        config: MachineConfig,
        connector: Arc<dyn Connector>,
        on_change: impl Fn(&MachineStatus) + Send + Sync + 'static,
        on_event: impl Fn(&BackendEvent) + Send + Sync + 'static,
    ) -> Self {
        Self::start_with(
            config,
            connector,
            Arc::new(RealPause),
            on_change,
            on_event,
            |c| Box::new(c),
        )
    }

    fn start_with(
        config: MachineConfig,
        connector: Arc<dyn Connector>,
        pause: Arc<dyn Pause>,
        on_change: impl Fn(&MachineStatus) + Send + Sync + 'static,
        on_event: impl Fn(&BackendEvent) + Send + Sync + 'static,
        link: impl FnOnce(Client) -> Box<dyn Link>,
    ) -> Self {
        let named = config.clone();
        let shared = Arc::new(Shared::with_events(
            move |state| {
                on_change(&MachineStatus {
                    config: named.clone(),
                    state: state.clone(),
                });
            },
            on_event,
        ));
        let label = format!("ssh:{}", config.ssh);
        let tracked = || Tracked {
            inner: connector.clone(),
            shared: shared.clone(),
        };
        let backend = Arc::new(PoimenBackend::remote(Client::with_connector(
            &label,
            Arc::new(Gated { inner: tracked() }),
        )));
        let link = link(Client::with_connector(&label, Arc::new(tracked())));
        let thread = {
            let shared = shared.clone();
            thread::spawn(move || supervise(link.as_ref(), &shared, pause.as_ref()))
        };
        Self {
            config,
            shared,
            backend,
            thread: Some(thread),
        }
    }

    pub fn config(&self) -> &MachineConfig {
        &self.config
    }

    pub fn status(&self) -> MachineStatus {
        MachineStatus {
            config: self.config.clone(),
            state: self.shared.state(),
        }
    }

    /// The backend of the machine's server; every request fails while it is not connected.
    pub fn backend(&self) -> Arc<PoimenBackend> {
        self.backend.clone()
    }

    /// Tries again at once when the machine is offline or incompatible.
    pub fn reconnect(&self) {
        if matches!(
            self.shared.state(),
            State::Offline { .. } | State::Incompatible { .. }
        ) {
            self.shared.wake_up();
        }
    }
}

impl Drop for Machine {
    /// Ends the supervisor and poimen's connections; the remote server and its terminals stay.
    fn drop(&mut self) {
        self.shared.stop();
        // The supervisor may sit in a connect (ssh's own timeouts bound it); it is not waited for.
        drop(self.thread.take());
    }
}

/// The machines of the settings, in their order.
#[derive(Default)]
pub struct Machines {
    machines: Mutex<Vec<Machine>>,
}

impl Machines {
    /// Follows the configured machines: an unchanged one keeps its connection, a new or changed
    /// one is started by `start`, a removed one is stopped.
    pub fn sync(&self, configs: &[MachineConfig], start: impl Fn(&MachineConfig) -> Machine) {
        let mut machines = lock(&self.machines);
        let mut old: Vec<Machine> = std::mem::take(&mut *machines);
        for config in configs {
            match old.iter().position(|m| m.config() == config) {
                Some(i) => machines.push(old.remove(i)),
                None => machines.push(start(config)),
            }
        }
        // `old` holds the removed and changed machines; dropping stops them.
    }

    pub fn list(&self) -> Vec<MachineStatus> {
        lock(&self.machines).iter().map(Machine::status).collect()
    }

    pub fn reconnect(&self, name: &str) -> Result<(), String> {
        self.with(name, Machine::reconnect)
    }

    pub fn backend(&self, name: &str) -> Result<Arc<PoimenBackend>, String> {
        self.with(name, Machine::backend)
    }

    fn with<T>(&self, name: &str, f: impl FnOnce(&Machine) -> T) -> Result<T, String> {
        lock(&self.machines)
            .iter()
            .find(|m| m.config().name == name)
            .map(f)
            .ok_or_else(|| format!("no machine named {name:?}"))
    }
}

/// First id of a stream on a machine's backend: local streams count up from 1, so the two never
/// meet and the stream commands route by the id alone.
pub const REMOTE_STREAM_BASE: u32 = 1 << 31;

type StreamMap = std::collections::HashMap<u32, (Arc<dyn Backend>, u32)>;

/// The terminal streams open on machines: an id the webview uses, mapped to the machine's backend
/// and its own stream id.
#[derive(Default)]
pub struct RemoteStreams {
    streams: Mutex<(u32, StreamMap)>,
}

impl RemoteStreams {
    /// Registers stream `inner` of `backend`; returns the id for the webview.
    pub fn add(&self, backend: Arc<dyn Backend>, inner: u32) -> u32 {
        let mut streams = lock(&self.streams);
        streams.0 += 1;
        let id = REMOTE_STREAM_BASE + streams.0;
        streams.1.insert(id, (backend, inner));
        id
    }

    /// The backend and stream id behind `id`; `None` for a local stream or a closed one.
    pub fn get(&self, id: u32) -> Option<(Arc<dyn Backend>, u32)> {
        lock(&self.streams).1.get(&id).cloned()
    }

    pub fn remove(&self, id: u32) -> Option<(Arc<dyn Backend>, u32)> {
        lock(&self.streams).1.remove(&id)
    }
}

/// `ssh -T -o BatchMode=yes ... <target> poimen-server stdio`: one ssh process per connection.
pub struct SshConnector {
    target: String,
    /// The ssh program; `None` resolves `ssh` on the login shell's `PATH` at every connect.
    program: Option<PathBuf>,
}

impl SshConnector {
    pub fn new(target: &str) -> Self {
        Self {
            target: target.to_owned(),
            program: None,
        }
    }
}

/// The arguments of ssh for `target`: no terminal, never a prompt, bounded connect, a dead link
/// noticed within 45 s, and the server of this build's channel in stdio mode.
pub fn ssh_args(target: &str) -> Vec<String> {
    let server = wire::CHANNEL.named("poimen-server");
    [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "--",
        target,
        &server,
        "stdio",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

impl Connector for SshConnector {
    fn connect(&self) -> io::Result<Pipe> {
        // A target from an older config would still be refused here, never passed as an option.
        check_target(&self.target).map_err(io::Error::other)?;
        let program = match &self.program {
            Some(p) => p.clone(),
            None => gui_path::resolve_program("ssh", &gui_path::child_path())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ssh is not on the PATH"))?,
        };
        let mut child = gui_path::command(&program)
            .args(ssh_args(&self.target))
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("ssh started without its pipes"));
        };
        let message = Arc::new(Mutex::new(String::new()));
        let collect = {
            let message = message.clone();
            thread::spawn(move || collect_stderr(stderr, &message))
        };
        let child = Arc::new(Mutex::new(child));
        let reader = SshOutput {
            stdout,
            stderr: Some(collect),
            message,
        };
        Ok(Pipe::new(reader, stdin, move || end(&child)))
    }
}

/// Keeps the first [`MAX_SSH_ERROR`] bytes of ssh's stderr, one line at a time.
fn collect_stderr(stderr: impl Read, message: &Mutex<String>) {
    for line in BufReader::new(stderr).lines() {
        let Ok(line) = line else { break };
        let mut message = lock(message);
        if message.len() + line.len() < MAX_SSH_ERROR {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(line.trim_end());
        }
    }
}

fn end(child: &Mutex<Child>) {
    let mut child = lock(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// ssh's stdout; its end of file is the ssh error when ssh wrote one (`Permission denied`,
/// `Could not resolve hostname`, `poimen-server: command not found`).
struct SshOutput<R> {
    stdout: R,
    stderr: Option<JoinHandle<()>>,
    message: Arc<Mutex<String>>,
}

impl<R: Read> Read for SshOutput<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.stdout.read(buf)?;
        if n > 0 || buf.is_empty() {
            return Ok(n);
        }
        // ssh has closed its stdout, so it is ending: wait for the rest of its stderr.
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
        let message = lock(&self.message).trim().to_owned();
        if message.is_empty() {
            Ok(0)
        } else {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                format!("ssh: {message}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicU32;

    fn parsed(value: Value) -> (Vec<MachineConfig>, Vec<String>) {
        let mut warnings = Vec::new();
        (parse(&value, &mut warnings), warnings)
    }

    fn config(name: &str) -> MachineConfig {
        MachineConfig {
            name: name.into(),
            ssh: format!("{name}.lan"),
            repos_dir: DEFAULT_REPOS_DIR.into(),
        }
    }

    #[test]
    fn machines_parse_with_the_default_repos_dir() {
        let (machines, warnings) = parsed(json!([
            {"name": "box", "ssh": "box"},
            {"name": "gpu", "ssh": "me@gpu.lan", "reposDir": "~/code/{owner}-{repo}"}
        ]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            machines,
            vec![
                MachineConfig {
                    name: "box".into(),
                    ssh: "box".into(),
                    repos_dir: "~/Source/{owner}/{repo}".into()
                },
                MachineConfig {
                    name: "gpu".into(),
                    ssh: "me@gpu.lan".into(),
                    repos_dir: "~/code/{owner}-{repo}".into()
                },
            ]
        );
    }

    #[test]
    fn an_empty_list_is_no_machines_without_a_warning() {
        assert_eq!(parsed(json!([])), (vec![], vec![]));
    }

    #[test]
    fn invalid_entries_warn_and_the_rest_loads() {
        let (machines, warnings) = parsed(json!([
            {"name": "box", "ssh": "box"},
            {"name": "box", "ssh": "other"},
            {"ssh": "nameless"},
            {"name": "x"},
            {"name": "opt", "ssh": "-oProxyCommand=evil"},
            {"name": "two", "ssh": "a b"},
            {"name": "num", "ssh": 3},
            {"name": "dir", "ssh": "d", "reposDir": "~/{org}/{repo}"},
            {"name": "open", "ssh": "d", "reposDir": "~/{owner"},
            "box",
            {"name": "ok", "ssh": "ok"}
        ]));
        let names: Vec<&str> = machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["box", "ok"]);
        assert_eq!(warnings.len(), 9, "{warnings:#?}");
        assert!(warnings[0].contains("already used"), "{}", warnings[0]);
        assert!(warnings[1].contains("name is missing"));
        assert!(warnings[2].contains("ssh is missing"));
        assert!(warnings[3].contains("not an option"));
        assert!(warnings[4].contains("without spaces"));
        assert!(warnings[5].contains("ssh must be a non-empty string"));
        assert!(warnings[6].contains("{org}"));
        assert!(warnings[7].contains("unclosed"));
        assert!(warnings[8].contains("must be an object"));
    }

    #[test]
    fn machines_that_are_not_a_list_warn() {
        let (machines, warnings) = parsed(json!({"name": "box"}));
        assert!(machines.is_empty());
        assert_eq!(warnings, ["machines must be a list; no machines"]);
    }

    #[test]
    fn ssh_runs_in_batch_mode_without_a_terminal_and_the_target_after_the_options() {
        let args = ssh_args("box");
        assert_eq!(&args[..3], ["-T", "-o", "BatchMode=yes"]);
        let target = args.iter().position(|a| a == "box").unwrap();
        assert_eq!(args[target - 1], "--");
        assert_eq!(
            &args[target + 1..],
            [wire::CHANNEL.named("poimen-server"), "stdio".into()]
        );
    }

    #[test]
    fn backoff_doubles_from_one_second_up_to_a_minute_and_resets() {
        let mut b = Backoff::default();
        let secs: Vec<u64> = (0..8).map(|_| b.next().as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 32, 60, 60]);
        b.reset();
        assert_eq!(b.next(), Duration::from_secs(1));
    }

    #[test]
    fn the_status_serializes_with_its_config_and_state() {
        let status = MachineStatus {
            config: config("box"),
            state: State::Incompatible {
                server_version: "0.1.0".into(),
                server_protocol: 1,
                app_version: "0.17.0".into(),
                app_protocol: 6,
            },
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({
                "name": "box", "ssh": "box.lan", "reposDir": "~/Source/{owner}/{repo}",
                "state": "incompatible", "serverVersion": "0.1.0", "serverProtocol": 1,
                "appVersion": "0.17.0", "appProtocol": 6
            })
        );
        let offline = MachineStatus {
            config: config("box"),
            state: State::Offline {
                error: "ssh: Permission denied (publickey).".into(),
            },
        };
        let v = serde_json::to_value(&offline).unwrap();
        assert_eq!(
            (v["state"].as_str(), v["error"].as_str()),
            (Some("offline"), Some("ssh: Permission denied (publickey)."))
        );
    }

    // --- The supervisor with a fake transport --------------------------------------------------

    fn ping(protocol: u32) -> PingResult {
        PingResult {
            version: "0.9.0".into(),
            protocol,
            pid: 7,
            clients: vec![],
        }
    }

    fn event() -> Event {
        Event::TerminalClosed {
            terminal_id: "t1".into(),
            space_id: "s1".into(),
        }
    }

    enum Step {
        Fail(&'static str),
        /// Connected; the session yields its events, then ends with the error.
        Session(u32, Vec<Event>, &'static str),
        Incompatible,
    }

    /// Answers each connect with the next step; `None` once the script is done.
    struct FakeLink {
        steps: Mutex<VecDeque<Step>>,
    }

    impl Link for FakeLink {
        fn connect(&self) -> Result<(PingResult, Option<Session>), String> {
            match lock(&self.steps).pop_front() {
                None => Err("script done".into()),
                Some(Step::Fail(e)) => Err(e.into()),
                Some(Step::Incompatible) => Ok((ping(1), None)),
                Some(Step::Session(protocol, events, end)) => {
                    let items: Vec<Result<Event, String>> = events
                        .into_iter()
                        .map(Ok)
                        .chain(std::iter::once(Err(end.to_owned())))
                        .collect();
                    Ok((ping(protocol), Some(Box::new(items.into_iter()))))
                }
            }
        }
    }

    /// Records each pause; stops the machine after `stop_after` pauses; woken on the pauses
    /// listed in `woken`.
    struct FakePause {
        delays: Mutex<Vec<Option<Duration>>>,
        stop_after: usize,
        woken: Vec<usize>,
    }

    impl Pause for FakePause {
        fn pause(&self, shared: &Shared, delay: Option<Duration>) -> bool {
            let mut delays = lock(&self.delays);
            delays.push(delay);
            if delays.len() >= self.stop_after {
                shared.stop();
            }
            self.woken.contains(&delays.len())
        }
    }

    fn run_script(
        steps: Vec<Step>,
        stop_after: usize,
        woken: Vec<usize>,
    ) -> (Vec<State>, Vec<Option<Duration>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let shared = Shared::new(move |s| lock(&record).push(s.clone()));
        let link = FakeLink {
            steps: Mutex::new(steps.into()),
        };
        let pause = FakePause {
            delays: Mutex::default(),
            stop_after,
            woken,
        };
        supervise(&link, &shared, &pause);
        let states = lock(&seen).clone();
        (states, pause.delays.into_inner().unwrap())
    }

    fn secs(s: u64) -> Option<Duration> {
        Some(Duration::from_secs(s))
    }

    fn connected() -> State {
        State::Connected {
            server_version: "0.9.0".into(),
        }
    }

    fn offline(e: &str) -> State {
        State::Offline { error: e.into() }
    }

    #[test]
    fn connect_drop_and_reconnect_go_through_the_states_with_backoff() {
        let (states, delays) = run_script(
            vec![
                Step::Session(PROTOCOL_VERSION, vec![event()], "connection reset"),
                Step::Fail("ssh: connect to host box port 22: Connection refused"),
                Step::Session(PROTOCOL_VERSION, vec![], "broken pipe"),
            ],
            3,
            vec![],
        );
        assert_eq!(
            states,
            vec![
                State::Connecting,
                connected(),
                offline("connection reset"),
                State::Connecting,
                offline("ssh: connect to host box port 22: Connection refused"),
                State::Connecting,
                connected(),
                offline("broken pipe"),
            ]
        );
        // After the drop 1 s, then 2 s; the next connection resets the backoff to 1 s.
        assert_eq!(delays, vec![secs(1), secs(2), secs(1)]);
    }

    #[test]
    fn an_auth_failure_is_offline_with_the_ssh_error_and_backs_off_doubling() {
        let denied = "ssh: me@box: Permission denied (publickey).";
        let (states, delays) = run_script(
            vec![Step::Fail(denied), Step::Fail(denied), Step::Fail(denied)],
            3,
            vec![],
        );
        assert_eq!(
            states,
            vec![
                State::Connecting,
                offline(denied),
                State::Connecting,
                offline(denied),
                State::Connecting,
                offline(denied),
            ]
        );
        assert_eq!(delays, vec![secs(1), secs(2), secs(4)]);
    }

    #[test]
    fn a_version_mismatch_is_incompatible_and_waits_for_a_reconnect_on_demand() {
        let (states, delays) = run_script(vec![Step::Incompatible], 1, vec![]);
        assert_eq!(
            states,
            vec![
                State::Connecting,
                State::Incompatible {
                    server_version: "0.9.0".into(),
                    server_protocol: 1,
                    app_version: env!("CARGO_PKG_VERSION").into(),
                    app_protocol: PROTOCOL_VERSION,
                },
            ]
        );
        assert_eq!(delays, vec![None], "no automatic retry");
    }

    #[test]
    fn a_ping_of_another_protocol_with_a_stream_is_still_incompatible() {
        let (states, _) = run_script(vec![Step::Session(1, vec![], "x")], 1, vec![]);
        assert!(matches!(
            states[1],
            State::Incompatible {
                server_protocol: 1,
                ..
            }
        ));
    }

    #[test]
    fn a_reconnect_on_demand_resets_the_backoff() {
        let (_, delays) = run_script(
            vec![
                Step::Fail("a"),
                Step::Fail("b"),
                Step::Fail("c"),
                Step::Fail("d"),
            ],
            4,
            vec![2],
        );
        assert_eq!(delays, vec![secs(1), secs(2), secs(1), secs(2)]);
    }

    // --- The gate -----------------------------------------------------------------------------

    /// Connections that record what is written to them; reads end at once.
    #[derive(Default)]
    struct Recorder {
        connects: AtomicU32,
        written: Arc<Mutex<Vec<u8>>>,
    }

    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            lock(&self.0).extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Connector for Recorder {
        fn connect(&self) -> io::Result<Pipe> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Ok(Pipe::new(io::empty(), Sink(self.written.clone()), || {}))
        }
    }

    struct ArcConnector(Arc<Recorder>);

    impl Connector for ArcConnector {
        fn connect(&self) -> io::Result<Pipe> {
            self.0.connect()
        }
    }

    fn gate(recorder: &Arc<Recorder>) -> (Arc<Shared>, Gated) {
        let shared = Arc::new(Shared::new(|_| {}));
        let gated = Gated {
            inner: Tracked {
                inner: Arc::new(ArcConnector(recorder.clone())),
                shared: shared.clone(),
            },
        };
        (shared, gated)
    }

    #[test]
    fn nothing_is_sent_to_a_remote_terminal_while_the_machine_is_not_connected() {
        let recorder = Arc::new(Recorder::default());
        let (shared, gated) = gate(&recorder);
        let backend = PoimenBackend::remote(Client::with_connector("ssh:box", Arc::new(gated)));
        use crate::backend::Backend;
        for state in [
            State::Connecting,
            offline("down"),
            State::Incompatible {
                server_version: "0.1.0".into(),
                server_protocol: 1,
                app_version: "x".into(),
                app_protocol: PROTOCOL_VERSION,
            },
        ] {
            shared.set(state);
            assert!(backend.send_input("t1", "rm -rf build").is_err());
        }
        assert_eq!(recorder.connects.load(Ordering::SeqCst), 0);
        assert!(lock(&recorder.written).is_empty());
    }

    #[test]
    fn a_connection_of_an_earlier_session_fails_and_is_never_replayed() {
        let recorder = Arc::new(Recorder::default());
        let (shared, gated) = gate(&recorder);
        shared.set(connected());
        let old = gated.connect().unwrap();
        (&old).write_all(b"first\n").unwrap();
        // Dropped, then connected again: a new session.
        shared.set(offline("reset"));
        assert_eq!(
            (&old).write_all(b"late\n").unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        shared.set(connected());
        assert_eq!(
            (&old).write_all(b"again\n").unwrap_err().kind(),
            io::ErrorKind::NotConnected,
            "a request of before the reconnect fails"
        );
        let new = gated.connect().unwrap();
        (&new).write_all(b"second\n").unwrap();
        assert_eq!(&*lock(&recorder.written), b"first\nsecond\n");
    }

    #[test]
    fn a_drop_ends_poimens_connections_and_sends_nothing() {
        let recorder = Arc::new(Recorder::default());
        let (shared, gated) = gate(&recorder);
        let closed = Arc::new(AtomicU32::new(0));
        shared.set(connected());
        let attached = gated.connect().unwrap();
        let link = FakeLink {
            steps: Mutex::new(vec![Step::Session(PROTOCOL_VERSION, vec![], "reset")].into()),
        };
        let counter = closed.clone();
        let watcher = Pipe::new(io::empty(), io::sink(), move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        shared.track(&watcher).unwrap();
        let pause = FakePause {
            delays: Mutex::default(),
            stop_after: 1,
            woken: vec![],
        };
        let written_before = lock(&recorder.written).len();
        supervise(&link, &shared, &pause);
        assert_eq!(
            closed.load(Ordering::SeqCst),
            1,
            "the tracked connection ended"
        );
        assert!((&attached).write_all(b"x").is_err());
        assert_eq!(
            lock(&recorder.written).len(),
            written_before,
            "no close request"
        );
    }

    /// Stops the machine while its connect is under way, then answers with a live session.
    struct StoppedWhileConnecting {
        shared: Arc<Shared>,
        read: Arc<AtomicBool>,
    }

    impl Link for StoppedWhileConnecting {
        fn connect(&self) -> Result<(PingResult, Option<Session>), String> {
            self.shared.stop();
            let read = self.read.clone();
            let session = std::iter::once(Ok(event())).inspect(move |_| {
                read.store(true, Ordering::SeqCst);
            });
            Ok((ping(PROTOCOL_VERSION), Some(Box::new(session))))
        }
    }

    #[test]
    fn a_machine_stopped_while_connecting_never_connects_or_follows_the_session() {
        let shared = Arc::new(Shared::new(|_| {}));
        let read = Arc::new(AtomicBool::new(false));
        let link = StoppedWhileConnecting {
            shared: shared.clone(),
            read: read.clone(),
        };
        let pause = FakePause {
            delays: Mutex::default(),
            stop_after: 1,
            woken: vec![],
        };
        supervise(&link, &shared, &pause);
        assert_eq!(shared.state(), State::Connecting);
        assert_eq!(shared.session(), None);
        assert!(!read.load(Ordering::SeqCst), "the session is not followed");
        assert!(lock(&pause.delays).is_empty(), "no retry after the stop");
    }

    /// A connection that counts its closes.
    struct Closing(Arc<AtomicU32>);

    impl Connector for Closing {
        fn connect(&self) -> io::Result<Pipe> {
            let closed = self.0.clone();
            Ok(Pipe::new(io::empty(), io::sink(), move || {
                closed.fetch_add(1, Ordering::SeqCst);
            }))
        }
    }

    #[test]
    fn a_connection_that_opens_after_the_stop_is_closed_and_refused() {
        let shared = Arc::new(Shared::new(|_| {}));
        let closed = Arc::new(AtomicU32::new(0));
        let tracked = Tracked {
            inner: Arc::new(Closing(closed.clone())),
            shared: shared.clone(),
        };
        let before = tracked.connect().unwrap();
        shared.stop();
        assert_eq!(closed.load(Ordering::SeqCst), 1, "the open one ended");
        let late = tracked.connect().unwrap_err();
        assert_eq!(late.kind(), io::ErrorKind::NotConnected);
        assert_eq!(
            closed.load(Ordering::SeqCst),
            2,
            "the late ssh process ended"
        );
        assert!(lock(&shared.pipes).is_empty(), "nothing left to keep alive");
        drop(before);
    }

    #[test]
    fn a_machine_reports_its_states_and_its_backend_waits_for_the_connection() {
        let recorder = Arc::new(Recorder::default());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let pause = Arc::new(FakePause {
            delays: Mutex::default(),
            stop_after: 1,
            woken: vec![],
        });
        let machine = Machine::start_with(
            config("box"),
            Arc::new(ArcConnector(recorder.clone())),
            pause,
            move |s| lock(&record).push(s.clone()),
            |_| {},
            |_| {
                Box::new(FakeLink {
                    steps: Mutex::new(
                        vec![Step::Fail("ssh: Could not resolve hostname box.lan")].into(),
                    ),
                })
            },
        );
        for _ in 0..200 {
            if lock(&seen).len() >= 2 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let seen = lock(&seen).clone();
        assert_eq!(seen[0].config, config("box"));
        assert_eq!(
            seen[1].state,
            offline("ssh: Could not resolve hostname box.lan")
        );
        assert_eq!(machine.status().state, seen[1].state);
        use crate::backend::Backend;
        assert!(machine.backend().snapshot().is_err());
        assert_eq!(recorder.connects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sync_keeps_unchanged_machines_and_starts_new_and_changed_ones() {
        let machines = Machines::default();
        let started = Arc::new(Mutex::new(Vec::new()));
        let start = |c: &MachineConfig| {
            lock(&started).push(c.name.clone());
            Machine::start_with(
                c.clone(),
                Arc::new(Recorder::default()),
                Arc::new(FakePause {
                    delays: Mutex::default(),
                    stop_after: 1,
                    woken: vec![],
                }),
                |_| {},
                |_| {},
                |_| {
                    Box::new(FakeLink {
                        steps: Mutex::default(),
                    })
                },
            )
        };
        machines.sync(&[config("a"), config("b")], start);
        let mut b = config("b");
        b.ssh = "elsewhere".into();
        machines.sync(&[config("a"), b, config("c")], start);
        assert_eq!(*lock(&started), ["a", "b", "b", "c"]);
        let names: Vec<String> = machines.list().into_iter().map(|s| s.config.name).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert!(machines.reconnect("c").is_ok());
        assert!(machines.reconnect("zz").unwrap_err().contains("zz"));
        machines.sync(&[], start);
        assert!(machines.list().is_empty());
    }

    #[test]
    fn remote_streams_get_ids_beside_the_local_ones_and_route_to_their_backend() {
        use crate::backend_fake::FakeBackend;
        let streams = RemoteStreams::default();
        let backend: Arc<dyn Backend> = Arc::new(FakeBackend::default());
        let a = streams.add(backend.clone(), 1);
        let b = streams.add(backend, 1);
        assert!(a >= REMOTE_STREAM_BASE && b > a);
        assert_eq!(streams.get(a).map(|(_, inner)| inner), Some(1));
        assert!(streams.get(7).is_none());
        assert!(streams.remove(a).is_some());
        assert!(streams.get(a).is_none());
        assert!(streams.get(b).is_some());
    }

    #[test]
    fn the_events_of_a_session_are_forwarded_as_backend_events() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let shared = Shared::with_events(|_| {}, move |e| lock(&record).push(e.clone()));
        let link = FakeLink {
            steps: Mutex::new(vec![Step::Session(PROTOCOL_VERSION, vec![event()], "reset")].into()),
        };
        let pause = FakePause {
            delays: Mutex::default(),
            stop_after: 1,
            woken: vec![],
        };
        supervise(&link, &shared, &pause);
        let seen = lock(&seen).clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].kind, "terminal.closed");
        assert_eq!(seen[0].data["terminal_id"], "t1");
    }

    // --- ssh's error on the end of its output -------------------------------------------------

    #[test]
    fn the_end_of_ssh_output_is_its_error_message() {
        let message = Arc::new(Mutex::new(String::new()));
        let collect = {
            let message = message.clone();
            thread::spawn(move || {
                collect_stderr(&b"me@box: Permission denied (publickey).\n"[..], &message)
            })
        };
        let mut out = SshOutput {
            stdout: &b"{}\n"[..],
            stderr: Some(collect),
            message,
        };
        let mut text = String::new();
        let err = out.read_to_string(&mut text).unwrap_err();
        assert_eq!(text, "{}\n");
        assert_eq!(
            err.to_string(),
            "ssh: me@box: Permission denied (publickey)."
        );
    }

    #[test]
    fn a_quiet_end_of_ssh_output_is_end_of_file() {
        let mut out = SshOutput {
            stdout: &b""[..],
            stderr: None,
            message: Arc::default(),
        };
        assert_eq!(out.read(&mut [0; 8]).unwrap(), 0);
    }
}
