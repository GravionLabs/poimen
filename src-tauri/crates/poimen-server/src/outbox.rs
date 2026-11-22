//! What the server sends a host over its connection (input and resize), queued without ever
//! making a caller wait for the host.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use poimen_protocol::encode_bytes;
use poimen_protocol::host::{ClientMessage, to_line};

use crate::host::HostConn;

/// Input queued for a host and not yet written; a writer past it waits (outside the global lock).
const INPUT_QUEUE_BYTES: usize = 1024 * 1024;

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
pub(crate) struct Outbox {
    pending: Mutex<Pending>,
    changed: Condvar,
    /// When input was last queued (#976: the output right after it is an echo).
    last_input: Mutex<Option<Instant>>,
}

impl Outbox {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(Pending::default()),
            changed: Condvar::new(),
            last_input: Mutex::new(None),
        })
    }

    /// Whether input was queued within `window`.
    pub(crate) fn input_within(&self, window: Duration) -> bool {
        self.last_input
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| at.elapsed() <= window)
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn input(&self, data: &[u8]) -> io::Result<()> {
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
        *self.last_input.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        pending.queued += data.len();
        pending.input.push_back(data.to_vec());
        self.changed.notify_all();
        Ok(())
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let mut pending = self.lock();
        if pending.closed {
            return Err("the terminal has ended".into());
        }
        pending.resize = Some((cols, rows));
        self.changed.notify_all();
        Ok(())
    }

    /// Nothing more is written; queued input is dropped and waiting writers return an error.
    pub(crate) fn close(&self) {
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
    pub(crate) fn run(&self, conn: &HostConn) {
        while let Some(message) = self.next() {
            let _ = (&conn.current()).write_all(&to_line(&message));
        }
    }
}

/// The input side for [`PtyWriter`]: every write is one `input` line.
pub(crate) struct InputLines(pub(crate) Arc<Outbox>);

impl Write for InputLines {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.input(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
