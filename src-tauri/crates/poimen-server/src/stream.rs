//! The attachments of one terminal: the headless emulator, the sequence number, and the clients
//! that receive the output.
//!
//! Everything that changes the terminal's visible state happens under one lock and bumps `seq`:
//! output, resize. An attach builds its snapshot and registers its client under that same lock,
//! so the snapshot covers exactly the records up to its `seq` and the stream carries every
//! record after it: nothing is lost or repeated between the two.
//!
//! The PTY is never blocked by a client: records go into a bounded channel per client with
//! `try_send`. The controller whose channel is full is detached (it must attach again for a
//! fresh snapshot). An observer whose channel is full is not (#976): it is marked lagging, its
//! output and resizes are dropped (the grid still saw them, so queries are answered as ever),
//! and once its queue has room again it gets the current size and a snapshot of the grid as one
//! output record, a resync that replaces everything it missed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::Duration;

use poimen_protocol::{
    AttachMode, AttachResult, DetachReason, ErrorCode, Event, Response, encode_bytes,
};
use serde_json::json;

use crate::emulator::{Colors, Emulator, Signal};
use crate::history::{Checkpoint, History};
use crate::pty::{MAX_BATCH, PtyWriter};
use crate::{WRITE_TIMEOUT, send, write_line};
use poimen_client::transport::Conn;

/// Output bytes between two checkpoints of the history log (#1194).
pub(crate) const CHECKPOINT_INTERVAL: usize = 1024 * 1024;

/// Records a client may have queued before it counts as too slow.
pub(crate) const CLIENT_QUEUE: usize = 128;

/// One NDJSON line for a client's writer thread.
#[derive(Debug)]
pub(crate) struct Line {
    pub text: Arc<str>,
    /// The writer closes the connection after this line.
    pub last: bool,
}

/// What a client's writer thread shares with the stream.
pub(crate) struct Link {
    /// Set when the stream could not queue a record: the writer then ends the attachment with
    /// the `overflow` reason, after what it already holds.
    pub overflowed: AtomicBool,
    /// Set when the server stops and leaves the terminal to its host (`server.shutdown` with
    /// `keep_terminals`): the writer closes the connection without a record, so the client sees
    /// the server go away and attaches again to the next one.
    pub released: AtomicBool,
    /// Set while the stream drops output for this observer (its queue was full); its writer
    /// asks for the resync when the queue has drained.
    pub lagging: AtomicBool,
}

impl Link {
    pub fn new() -> Self {
        Self {
            overflowed: AtomicBool::new(false),
            released: AtomicBool::new(false),
            lagging: AtomicBool::new(false),
        }
    }
}

struct Client {
    id: u64,
    tx: SyncSender<Line>,
    link: Arc<Link>,
    /// The queue was full and records were dropped; a resync is due.
    lagging: bool,
}

struct State {
    emulator: Emulator,
    seq: u64,
    clients: Vec<Client>,
    next_client: u64,
    controller: Option<u64>,
    /// `Some(code)` once the process has exited.
    exited: Option<Option<u32>>,
    /// The terminal's history from the log after its process was gone (#1195): attaching is
    /// read-only.
    read_only: bool,
    /// The host `seq` of the last output record the grid has seen (0: unknown).
    host_seq: u64,
    /// Output bytes since the last checkpoint of the history log.
    since_checkpoint: usize,
    /// A batch is being fed chunk by chunk: the grid is between two records.
    mid_batch: bool,
    /// The attached clients that hold an unsent prompt draft (#1163); a client leaves it when it
    /// detaches.
    drafts: Vec<u64>,
}

pub(crate) struct TermStream {
    terminal_id: String,
    state: Mutex<State>,
    publish: Arc<dyn Fn(Event) + Send + Sync>,
    /// The scheme colors for color queries, shared by all terminals.
    colors: Arc<RwLock<Colors>>,
    /// Where answers to terminal queries go while no controller is attached.
    writer: Mutex<Option<PtyWriter>>,
    /// Where the agent status signals of the output go (OSC 7501, new prompts, RIS), called
    /// outside the stream lock.
    signals: OnceLock<SignalSink>,
    /// Where the live output and the resizes are logged to disk (#973).
    history: OnceLock<Arc<History>>,
    /// Set by [`closed`](Self::closed): nothing is logged afterwards.
    closed: AtomicBool,
    /// Output bytes between two checkpoints of the history log.
    checkpoint_every: AtomicUsize,
}

/// Receives the signals of one output batch, in order.
pub(crate) type SignalSink = Arc<dyn Fn(Vec<Signal>) + Send + Sync>;

/// An attachment: the id the connection uses to detach and to prove it is the controller.
#[derive(Debug)]
pub(crate) struct Attached {
    pub client_id: u64,
    pub mode: AttachMode,
}

pub(crate) fn line_of<T: serde::Serialize>(value: &T) -> Arc<str> {
    // The protocol types always serialize.
    let mut text = serde_json::to_string(value).unwrap_or_default();
    text.push('\n');
    text.into()
}

impl TermStream {
    pub fn new(
        terminal_id: &str,
        cols: u16,
        rows: u16,
        scrollback: usize,
        colors: Arc<RwLock<Colors>>,
        publish: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> Self {
        Self {
            terminal_id: terminal_id.to_string(),
            state: Mutex::new(State {
                emulator: Emulator::new(cols, rows, scrollback),
                seq: 0,
                clients: Vec::new(),
                next_client: 1,
                controller: None,
                drafts: Vec::new(),
                exited: None,
                read_only: false,
                host_seq: 0,
                since_checkpoint: 0,
                mid_batch: false,
            }),
            publish,
            colors,
            writer: Mutex::new(None),
            signals: OnceLock::new(),
            history: OnceLock::new(),
            closed: AtomicBool::new(false),
            checkpoint_every: AtomicUsize::new(CHECKPOINT_INTERVAL),
        }
    }

    /// Output bytes between two checkpoints of the history log (tests).
    #[cfg(test)]
    pub fn set_checkpoint_every(&self, bytes: usize) {
        self.checkpoint_every.store(bytes, Ordering::Relaxed);
    }

    /// Starts a new history log: the old one is removed and the empty grid is its first
    /// checkpoint, so the log always has a valid starting point.
    pub fn start_log(&self) {
        if let Some(history) = self.history() {
            history.reset(&self.terminal_id);
        }
        self.checkpoint();
    }

    /// Puts a grid rebuilt from the history log in place of the empty one (#1194): `seq` is the
    /// host record it covers. A size other than the terminal's is kept in the log.
    pub fn restore(&self, mut emulator: Emulator, seq: u64) {
        let mut st = self.lock();
        let (cols, rows) = st.emulator.size();
        if emulator.size() != (cols, rows) {
            emulator.resize(cols, rows);
            if let Some(history) = self.history() {
                history.resize(&self.terminal_id, cols, rows);
            }
        }
        st.emulator = emulator;
        st.host_seq = seq;
        st.since_checkpoint = 0;
    }

    /// Makes the stream the read-only history of an ended terminal (#1195): the grid rebuilt
    /// from the log, no process, no controller ever. Nothing is written to the log.
    pub fn ended_from_history(&self, emulator: Emulator) {
        let mut st = self.lock();
        st.emulator = emulator;
        st.exited = Some(None);
        st.read_only = true;
    }

    /// A new process runs in the ended terminal (#1232): attaching is no longer read-only. The
    /// log goes on; the grid so far is a checkpoint that covers no host record, because the new
    /// host counts its records from 1 again.
    pub fn revive(&self) {
        let mut st = self.lock();
        st.read_only = false;
        st.exited = None;
        st.host_seq = 0;
        self.checkpoint_locked(&mut st);
    }

    /// Queues a checkpoint of the grid as it is now to the history log (#1194).
    pub fn checkpoint(&self) {
        let mut st = self.lock();
        self.checkpoint_locked(&mut st);
    }

    fn checkpoint_locked(&self, st: &mut State) {
        if st.mid_batch {
            // The log replays this batch from its own records.
            return;
        }
        st.since_checkpoint = 0;
        let Some(history) = self.history() else {
            return;
        };
        let (cols, rows) = st.emulator.size();
        history.checkpoint(
            &self.terminal_id,
            &Checkpoint {
                seq: st.host_seq,
                cols,
                rows,
                grid: st.emulator.snapshot(),
            },
        );
    }

    /// Where output and resizes are written to the history log; set once.
    pub fn set_history(&self, history: Arc<History>) {
        let _ = self.history.set(history);
    }

    /// The history writer, `None` once the terminal is closed: the output that still arrives
    /// must not bring its log back (#1196).
    fn history(&self) -> Option<&Arc<History>> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        self.history.get()
    }

    /// Where the agent status signals of the output go; set once.
    pub fn set_signal_sink(&self, sink: SignalSink) {
        let _ = self.signals.set(sink);
    }

    /// The PTY the server answers terminal queries on while no controller is attached.
    pub fn set_writer(&self, writer: PtyWriter) {
        *self.writer.lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a connection is attached as controller.
    pub fn has_controller(&self) -> bool {
        self.lock().controller.is_some()
    }

    /// Whether the emulator shows the alternate screen (a full-screen program).
    pub fn alt_screen(&self) -> bool {
        self.lock().emulator.alt_screen()
    }

    /// Whether an attached client holds an unsent prompt draft for this terminal.
    pub fn has_draft(&self) -> bool {
        !self.lock().drafts.is_empty()
    }

    /// Sets or clears the draft of an attached client; false when it is not attached.
    pub fn set_draft(&self, client_id: u64, draft: bool) -> bool {
        let mut st = self.lock();
        if !st.clients.iter().any(|c| c.id == client_id) {
            return false;
        }
        st.drafts.retain(|d| *d != client_id);
        if draft {
            st.drafts.push(client_id);
        }
        true
    }

    pub fn is_controller(&self, client_id: u64) -> bool {
        self.lock().controller == Some(client_id)
    }

    /// Feeds PTY output to the emulator and passes it on to every client. The queries in it
    /// (colors, cursor position, device attributes) are answered here while no controller is
    /// attached; a controller's xterm.js answers them from the same bytes, and then the
    /// answers of the grid are dropped so a query never gets two replies. The hand-over is at
    /// attach and detach of the controller.
    #[cfg(test)]
    pub fn output(&self, bytes: &[u8]) {
        self.output_at(bytes, 0);
    }

    /// [`output`](Self::output) for the records up to host record `seq` (0: unknown).
    pub fn output_at(&self, bytes: &[u8], seq: u64) {
        if let Some(history) = self.history() {
            history.output_at(&self.terminal_id, bytes, seq);
        }
        self.feed(bytes, true, seq);
    }

    /// Feeds output that ran before this server read the host (the replay of its ring when the
    /// server adopts it): grid and clients as [`output`](Self::output), but its queries are not
    /// answered again.
    #[cfg(test)]
    pub fn replay(&self, bytes: &[u8]) {
        self.replay_at(bytes, 0);
    }

    /// [`replay`](Self::replay) for the records up to host record `seq` (0: unknown). The log
    /// does not hold them yet (the host's records it has were dropped before), so they go in.
    pub fn replay_at(&self, bytes: &[u8], seq: u64) {
        if let Some(history) = self.history() {
            history.output_at(&self.terminal_id, bytes, seq);
        }
        self.feed(bytes, false, seq);
    }

    fn feed(&self, bytes: &[u8], live: bool, seq: u64) {
        let mut signals = Vec::new();
        for chunk in bytes.chunks(MAX_BATCH) {
            let mut st = self.lock();
            st.mid_batch = true;
            st.emulator.feed(chunk);
            signals.extend(st.emulator.take_signals());
            let answers = {
                let colors = self.colors.read().unwrap_or_else(|e| e.into_inner());
                st.emulator.take_answers(&colors)
            };
            // With a controller, xterm.js answers the standard queries; only the answer to
            // OSC 7501 (which it does not know) is the server's.
            let replies = if st.controller.is_none() {
                answers.all
            } else {
                answers.own
            };
            let answer = live && !replies.is_empty();
            st.seq += 1;
            let line = line_of(&Event::TerminalOutput {
                terminal_id: self.terminal_id.clone(),
                seq: st.seq,
                data: encode_bytes(chunk),
            });
            self.broadcast(&mut st, &line);
            drop(st);
            if answer {
                self.answer(&replies);
            }
        }
        {
            let mut st = self.lock();
            st.mid_batch = false;
            if seq > 0 {
                st.host_seq = seq;
            }
            st.since_checkpoint += bytes.len();
            if st.since_checkpoint >= self.checkpoint_every.load(Ordering::Relaxed) {
                self.checkpoint_locked(&mut st);
            }
        }
        // One call for the whole batch, so a replay changes the status at most once.
        if !signals.is_empty()
            && let Some(sink) = self.signals.get()
        {
            sink(signals);
        }
    }

    /// Writes the answers to the PTY, outside the stream lock (a write may block).
    fn answer(&self, replies: &[u8]) {
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(writer) = writer {
            // The process may be gone; nobody waits for the answer then.
            let _ = writer.write(replies);
        }
    }

    /// The controller's resize: grid and records. (The caller resizes the PTY.)
    pub fn resized(&self, cols: u16, rows: u16) {
        if let Some(history) = self.history() {
            history.resize(&self.terminal_id, cols, rows);
        }
        let mut st = self.lock();
        st.emulator.resize(cols, rows);
        st.seq += 1;
        let line = line_of(&Event::TerminalResized {
            terminal_id: self.terminal_id.clone(),
            seq: st.seq,
            cols,
            rows,
        });
        self.broadcast(&mut st, &line);
    }

    /// The process ended: tell the clients and end their streams.
    pub fn exited(&self, exit_code: Option<u32>) {
        let mut st = self.lock();
        // The final state of the grid is the log's last checkpoint (#1194).
        self.checkpoint_locked(&mut st);
        st.exited = Some(exit_code);
        let line = line_of(&Event::TerminalExited {
            terminal_id: self.terminal_id.clone(),
            exit_code,
        });
        for client in st.clients.drain(..) {
            let _ = client.tx.try_send(Line {
                text: Arc::clone(&line),
                last: true,
            });
        }
        st.controller = None;
    }

    /// The terminal is closed: end every attachment.
    pub fn closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let mut st = self.lock();
        let line = line_of(&Event::TerminalDetached {
            terminal_id: self.terminal_id.clone(),
            reason: DetachReason::Closed,
        });
        for client in st.clients.drain(..) {
            kick(&client, &line);
        }
        st.controller = None;
    }

    /// The server stops and leaves the terminal running in its host: every attachment ends
    /// without a record, as if the server went away, so its client attaches to the next server.
    pub fn release(&self) {
        let mut st = self.lock();
        // A checkpoint of the grid, so the next server rebuilds without a replay (#1194).
        self.checkpoint_locked(&mut st);
        if let Some(history) = self.history() {
            history.flush();
        }
        for client in st.clients.drain(..) {
            client.link.released.store(true, Ordering::SeqCst);
            // Wakes the writer; a full queue is noticed by its next poll of the flag.
            let _ = client.tx.try_send(Line {
                text: Arc::from(""),
                last: true,
            });
        }
        st.controller = None;
    }

    /// Attaches a client. The response with the snapshot is queued first on the client's
    /// channel, the records follow. Fails with `conflict` when a controller exists and the
    /// request does not take over.
    pub fn attach(
        &self,
        request_id: &str,
        mode: AttachMode,
        take_over: bool,
        tx: SyncSender<Line>,
        link: Arc<Link>,
    ) -> Result<Attached, (ErrorCode, String)> {
        let mut st = self.lock();
        // The history of an ended terminal can only be looked at.
        let mode = if st.read_only {
            AttachMode::Observe
        } else {
            mode
        };
        if mode == AttachMode::Control && st.controller.is_some() && !take_over {
            return Err((
                ErrorCode::Conflict,
                format!("terminal {} already has a controller", self.terminal_id),
            ));
        }
        let (cols, rows) = st.emulator.size();
        let result = AttachResult {
            terminal_id: self.terminal_id.clone(),
            seq: st.seq,
            cols,
            rows,
            mode,
            snapshot: encode_bytes(&st.emulator.snapshot()),
            read_only: st.read_only,
        };
        let response = line_of(&Response::ok(request_id, json!(result)));
        // A fresh channel has room.
        let _ = tx.try_send(Line {
            text: response,
            last: false,
        });
        if let Some(code) = st.exited {
            let _ = tx.try_send(Line {
                text: line_of(&Event::TerminalExited {
                    terminal_id: self.terminal_id.clone(),
                    exit_code: code,
                }),
                last: true,
            });
            return Ok(Attached { client_id: 0, mode });
        }
        if mode == AttachMode::Control
            && let Some(old) = st.controller.take()
        {
            {
                let line = line_of(&Event::TerminalDetached {
                    terminal_id: self.terminal_id.clone(),
                    reason: DetachReason::TakenOver,
                });
                if let Some(at) = st.clients.iter().position(|c| c.id == old) {
                    let client = st.clients.remove(at);
                    kick(&client, &line);
                }
                (self.publish)(Event::TerminalControllerDetached {
                    terminal_id: self.terminal_id.clone(),
                });
            }
        }
        let id = st.next_client;
        st.next_client += 1;
        st.clients.push(Client {
            id,
            tx,
            link,
            lagging: false,
        });
        if mode == AttachMode::Control {
            st.controller = Some(id);
            (self.publish)(Event::TerminalControllerAttached {
                terminal_id: self.terminal_id.clone(),
            });
        }
        Ok(Attached {
            client_id: id,
            mode,
        })
    }

    /// Removes a client whose connection ended. Reports the controller leaving.
    pub fn detach(&self, client_id: u64) {
        let mut st = self.lock();
        self.remove(&mut st, client_id);
    }

    fn remove(&self, st: &mut State, client_id: u64) {
        let before = st.clients.len();
        st.clients.retain(|c| c.id != client_id);
        st.drafts.retain(|d| *d != client_id);
        if st.clients.len() != before && st.controller == Some(client_id) {
            st.controller = None;
            (self.publish)(Event::TerminalControllerDetached {
                terminal_id: self.terminal_id.clone(),
            });
        }
    }

    /// Sends a record that changes the grid (output, resize) to every client. The controller
    /// that cannot take it is detached; an observer is marked lagging and gets a resync later.
    fn broadcast(&self, st: &mut State, line: &Arc<str>) {
        let controller = st.controller;
        let mut gone = Vec::new();
        for client in &mut st.clients {
            if client.lagging {
                continue;
            }
            match client.tx.try_send(Line {
                text: Arc::clone(line),
                last: false,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) if controller != Some(client.id) => {
                    client.lagging = true;
                    client.link.lagging.store(true, Ordering::SeqCst);
                }
                Err(TrySendError::Full(_)) => {
                    client.link.overflowed.store(true, Ordering::SeqCst);
                    gone.push(client.id);
                }
                Err(TrySendError::Disconnected(_)) => gone.push(client.id),
            }
        }
        for id in gone {
            self.remove(st, id);
        }
    }

    /// Brings a lagging observer up to date when its queue has room: the current size and the
    /// grid as one output record. Called by its writer when the queue is drained, and before
    /// every record.
    pub fn resync(&self, client_id: u64) {
        let mut st = self.lock();
        let State {
            emulator,
            seq,
            clients,
            ..
        } = &mut *st;
        let Some(client) = clients.iter_mut().find(|c| c.id == client_id) else {
            return;
        };
        if !client.lagging {
            return;
        }
        *seq += 1;
        let (cols, rows) = emulator.size();
        let resized = line_of(&Event::TerminalResized {
            terminal_id: self.terminal_id.clone(),
            seq: *seq,
            cols,
            rows,
        });
        let output = line_of(&Event::TerminalOutput {
            terminal_id: self.terminal_id.clone(),
            seq: *seq,
            data: encode_bytes(&emulator.snapshot()),
        });
        for text in [resized, output] {
            if client.tx.try_send(Line { text, last: false }).is_err() {
                // Still full (or gone, which the writer notices); try again later.
                return;
            }
        }
        client.lagging = false;
        client.link.lagging.store(false, Ordering::SeqCst);
    }
}

/// Ends a client's attachment with the given `detached` line: queued when there is room,
/// otherwise the writer is told to report an overflow.
fn kick(client: &Client, line: &Arc<str>) {
    let sent = client
        .tx
        .try_send(Line {
            text: Arc::clone(line),
            last: true,
        })
        .is_ok();
    if !sent {
        client.link.overflowed.store(true, Ordering::SeqCst);
    }
}

/// The writer of an attached connection: the lines of its queue in order, then, when the stream
/// ran over, a `detached` record. A client that stops reading blocks only this thread, which
/// gives up after [`WRITE_TIMEOUT`] (Unix; named pipes have no write timeout, so on Windows it
/// waits for that client alone). An observer whose queue ran over is not detached (#976): the
/// stream drops its output and, once this queue is drained and idle, `resync` queues the
/// current size and grid.
pub(crate) fn write_stream(
    mut stream: Conn,
    rx: &Receiver<Line>,
    link: &Link,
    terminal_id: &str,
    term: &TermStream,
    client_id: u64,
) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    loop {
        let lagging = link.lagging.load(std::sync::atomic::Ordering::SeqCst);
        let wait = Duration::from_millis(if lagging { 50 } else { 200 });
        match rx.recv_timeout(wait) {
            Ok(line) => {
                if write_line(&mut stream, &line.text).is_err() || line.last {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if lagging {
                    term.resync(client_id);
                }
            }
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

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
