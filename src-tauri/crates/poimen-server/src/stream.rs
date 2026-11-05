//! The attachments of one terminal: the headless emulator, the sequence number, and the clients
//! that receive the output.
//!
//! Everything that changes the terminal's visible state happens under one lock and bumps `seq`:
//! output, resize. An attach builds its snapshot and registers its client under that same lock,
//! so the snapshot covers exactly the records up to its `seq` and the stream carries every
//! record after it: nothing is lost or repeated between the two.
//!
//! The PTY is never blocked by a client: records go into a bounded channel per client with
//! `try_send`; a client whose channel is full is detached (it must attach again for a fresh
//! snapshot).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use poimen_protocol::{
    AttachMode, AttachResult, DetachReason, ErrorCode, Event, Response, encode_bytes,
};
use serde_json::json;

use crate::emulator::{Colors, Emulator};
use crate::pty::{MAX_BATCH, PtyWriter};

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
}

struct Client {
    id: u64,
    tx: SyncSender<Line>,
    link: Arc<Link>,
}

struct State {
    emulator: Emulator,
    seq: u64,
    clients: Vec<Client>,
    next_client: u64,
    controller: Option<u64>,
    /// `Some(code)` once the process has exited.
    exited: Option<Option<u32>>,
}

pub(crate) struct TermStream {
    terminal_id: String,
    state: Mutex<State>,
    publish: Arc<dyn Fn(Event) + Send + Sync>,
    /// The scheme colors for color queries, shared by all terminals.
    colors: Arc<RwLock<Colors>>,
    /// Where answers to terminal queries go while no controller is attached.
    writer: Mutex<Option<PtyWriter>>,
}

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
                exited: None,
            }),
            publish,
            colors,
            writer: Mutex::new(None),
        }
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

    pub fn is_controller(&self, client_id: u64) -> bool {
        self.lock().controller == Some(client_id)
    }

    /// Feeds PTY output to the emulator and passes it on to every client. The queries in it
    /// (colors, cursor position, device attributes) are answered here while no controller is
    /// attached; a controller's xterm.js answers them from the same bytes, and then the
    /// answers of the grid are dropped so a query never gets two replies. The hand-over is at
    /// attach and detach of the controller.
    pub fn output(&self, bytes: &[u8]) {
        for chunk in bytes.chunks(MAX_BATCH) {
            let mut st = self.lock();
            st.emulator.feed(chunk);
            let replies = {
                let colors = self.colors.read().unwrap_or_else(|e| e.into_inner());
                st.emulator.take_replies(&colors)
            };
            let answer = st.controller.is_none() && !replies.is_empty();
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
        st.clients.push(Client { id, tx, link });
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
        if st.clients.len() != before && st.controller == Some(client_id) {
            st.controller = None;
            (self.publish)(Event::TerminalControllerDetached {
                terminal_id: self.terminal_id.clone(),
            });
        }
    }

    fn broadcast(&self, st: &mut State, line: &Arc<str>) {
        let mut gone = Vec::new();
        for client in &st.clients {
            match client.tx.try_send(Line {
                text: Arc::clone(line),
                last: false,
            }) {
                Ok(()) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, sync_channel};

    fn stream() -> (TermStream, Arc<Mutex<Vec<Event>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let s = TermStream::new(
            "t1",
            20,
            5,
            100,
            Arc::default(),
            Arc::new(move |e| sink.lock().unwrap().push(e)),
        );
        (s, events)
    }

    fn client(
        s: &TermStream,
        mode: AttachMode,
        take_over: bool,
        cap: usize,
    ) -> Result<(Attached, Receiver<Line>), (ErrorCode, String)> {
        let (tx, rx) = sync_channel(cap);
        let link = Arc::new(Link {
            overflowed: AtomicBool::new(false),
        });
        s.attach("1", mode, take_over, tx, link).map(|a| (a, rx))
    }

    fn texts(rx: &Receiver<Line>) -> Vec<String> {
        rx.try_iter().map(|l| l.text.to_string()).collect()
    }

    #[test]
    fn output_before_attach_is_in_the_snapshot_and_after_it_in_the_stream() {
        let (s, _) = stream();
        s.output(b"before");
        let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
        s.output(b"after");
        let lines = texts(&rx);
        let attach: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(attach["result"]["seq"], 1);
        let snap =
            poimen_protocol::decode_bytes(attach["result"]["snapshot"].as_str().unwrap()).unwrap();
        assert!(String::from_utf8_lossy(&snap).contains("before"));
        assert!(!String::from_utf8_lossy(&snap).contains("after"));
        let record: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(record["data"]["seq"], 2);
        assert_eq!(
            poimen_protocol::decode_bytes(record["data"]["data"].as_str().unwrap()).unwrap(),
            b"after"
        );
    }

    #[test]
    fn a_second_controller_needs_take_over_and_the_first_is_detached() {
        let (s, events) = stream();
        let (first, rx1) = client(&s, AttachMode::Control, false, 8).unwrap();
        let err = client(&s, AttachMode::Control, false, 8).unwrap_err();
        assert_eq!(err.0, ErrorCode::Conflict);
        let (second, _rx2) = client(&s, AttachMode::Control, true, 8).unwrap();
        assert!(!s.is_controller(first.client_id));
        assert!(s.is_controller(second.client_id));
        let last = rx1.try_iter().last().unwrap();
        assert!(last.last && last.text.contains("taken_over"));
        let events = events.lock().unwrap();
        assert_eq!(
            events.as_slice(),
            [
                Event::TerminalControllerAttached {
                    terminal_id: "t1".into()
                },
                Event::TerminalControllerDetached {
                    terminal_id: "t1".into()
                },
                Event::TerminalControllerAttached {
                    terminal_id: "t1".into()
                },
            ]
        );
    }

    #[test]
    fn observers_are_many_and_do_not_take_the_control() {
        let (s, _) = stream();
        let (_c, _rc) = client(&s, AttachMode::Control, false, 8).unwrap();
        let (a, _ra) = client(&s, AttachMode::Observe, false, 8).unwrap();
        let (b, _rb) = client(&s, AttachMode::Observe, true, 8).unwrap();
        assert!(!s.is_controller(a.client_id) && !s.is_controller(b.client_id));
        assert!(s.has_controller());
    }

    #[test]
    fn a_full_client_is_dropped_and_the_others_keep_receiving() {
        let (s, events) = stream();
        let (slow, slow_rx) = client(&s, AttachMode::Control, false, 2).unwrap();
        let (_ok, ok_rx) = client(&s, AttachMode::Observe, false, 64).unwrap();
        for i in 0..5 {
            s.output(format!("{i}").as_bytes());
        }
        assert!(!s.is_controller(slow.client_id));
        assert_eq!(texts(&ok_rx).len(), 1 + 5);
        assert_eq!(slow_rx.try_iter().count(), 2);
        assert!(
            events
                .lock()
                .unwrap()
                .contains(&Event::TerminalControllerDetached {
                    terminal_id: "t1".into()
                })
        );
    }

    #[test]
    fn detaching_the_controller_is_reported_once() {
        let (s, events) = stream();
        let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
        s.detach(c.client_id);
        s.detach(c.client_id);
        assert!(!s.has_controller());
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[test]
    fn exit_ends_the_streams_and_late_attachers_get_the_snapshot_and_the_exit() {
        let (s, _) = stream();
        let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
        s.output(b"bye");
        s.exited(Some(3));
        let lines = texts(&rx);
        assert!(lines.last().unwrap().contains(r#""exit_code":3"#));
        let (_, late) = client(&s, AttachMode::Control, false, 8).unwrap();
        let lines = texts(&late);
        assert!(lines[0].contains("snapshot"));
        assert!(lines[1].contains("terminal.exited"));
        assert!(!s.has_controller());
    }

    #[test]
    fn resize_is_recorded_with_a_sequence_number() {
        let (s, _) = stream();
        let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
        s.resized(30, 9);
        let lines = texts(&rx);
        let r: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(r["event"], "terminal.resized");
        assert_eq!(
            (
                r["data"]["cols"].as_u64(),
                r["data"]["rows"].as_u64(),
                r["data"]["seq"].as_u64()
            ),
            (Some(30), Some(9), Some(1))
        );
    }

    /// A writer that collects what the stream answers.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn answering() -> (TermStream, Sink) {
        let (s, _) = stream();
        let sink = Sink::default();
        s.set_writer(PtyWriter::new(Box::new(sink.clone())));
        (s, sink)
    }

    fn answered(sink: &Sink) -> String {
        String::from_utf8_lossy(&sink.0.lock().unwrap()).into_owned()
    }

    #[test]
    fn without_a_controller_queries_are_answered_on_the_pty() {
        let (s, sink) = answering();
        s.output(b"ab\x1b[6n\x1b[c\x1b[>c");
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            answered(&sink).starts_with("\x1b[1;3R\x1b[?6c\x1b[>0;"),
            "{version}"
        );
    }

    #[test]
    fn color_queries_use_the_colors_set_by_the_app() {
        let (s, sink) = answering();
        {
            let mut colors = s.colors.write().unwrap();
            colors.background = alacritty_terminal::vte::ansi::Rgb {
                r: 0x11,
                g: 0x22,
                b: 0x33,
            };
        }
        s.output(b"\x1b]11;?\x07");
        assert_eq!(answered(&sink), "\x1b]11;rgb:1111/2222/3333\x07");
    }

    #[test]
    fn with_a_controller_the_server_stays_silent_and_answers_again_after_it_left() {
        let (s, sink) = answering();
        let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
        s.output(b"\x1b[c\x1b]10;?\x07\x1b[6n");
        assert_eq!(answered(&sink), "");
        s.detach(c.client_id);
        s.output(b"\x1b[c");
        assert_eq!(answered(&sink), "\x1b[?6c");
    }

    #[test]
    fn replies_of_a_silent_period_are_not_delivered_late() {
        let (s, sink) = answering();
        let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
        s.output(b"\x1b[c");
        s.detach(c.client_id);
        s.output(b"x");
        assert_eq!(answered(&sink), "");
    }
}
