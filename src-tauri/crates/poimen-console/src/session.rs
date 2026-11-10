//! One attachment: snapshot first, then the raw stream to `out`; keyboard input to the terminal
//! (controller only); local size changes to the PTY (controller only); detach key.

use std::fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use poimen_client::protocol::{AttachMode, DetachReason, ErrorCode, Event, decode_bytes};
use poimen_client::{Client, ClientError};

use crate::keys::{KeyFilter, Utf8Joiner};

/// How often the stop flag and the local size are looked at.
const TICK: Duration = Duration::from_millis(100);

/// Written to the local terminal when the session ends: leaves the alternate screen, mouse
/// tracking, bracketed paste, application keys and focus reporting that the stream may have
/// switched on, then shows the cursor and resets the pen.
pub const RESET: &[u8] = b"\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\
\x1b[?2004l\x1b[?1004l\x1b[?1l\x1b>\x1b[?25h\x1b[0m";

#[derive(Debug)]
pub struct Options {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub take_over: bool,
    pub prefix: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The detach key; the terminal keeps running.
    Detached,
    /// The process in the terminal ended.
    Exited(Option<u32>),
    /// The server ended the attachment (`taken_over`, `overflow`, `closed`).
    ServerDetached(DetachReason),
    /// `stop` was set (a signal).
    Interrupted,
    /// The connection to the server ended.
    Closed(String),
}

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Client(ClientError::Server {
                code: ErrorCode::Conflict,
                ..
            }) => write!(
                f,
                "the terminal already has a controller (use --takeover to replace it)"
            ),
            Error::Client(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<ClientError> for Error {
    fn from(e: ClientError) -> Self {
        Error::Client(e)
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

enum Msg {
    Record(Box<Result<Event, String>>),
    Input(Vec<u8>),
}

/// Attaches, writes the snapshot and then the stream to `out` until detach, exit, the server
/// ending the attachment or `stop`. `input` yields raw keyboard bytes, `size` the local window
/// size. As controller the PTY is resized to the window at once and whenever it changes; as
/// observer the controller's size is kept. `out` gets [`RESET`] at the end.
pub fn run(
    client: &Client,
    opts: &Options,
    input: Receiver<Vec<u8>>,
    out: &mut dyn Write,
    size: &dyn Fn() -> (u16, u16),
    stop: &AtomicBool,
) -> Result<Outcome, Error> {
    let controller = opts.mode == AttachMode::Control;
    let mut attachment = client.attach(&opts.terminal_id, opts.mode, opts.take_over)?;
    let mut writer = attachment.writer()?;
    let snapshot = decode_bytes(&attachment.info.snapshot)
        .map_err(|e| ClientError::Protocol(format!("snapshot: {e}")))?;

    let (tx, rx) = mpsc::channel();
    let records = tx.clone();
    thread::spawn(move || {
        loop {
            let next = attachment.next_record().map_err(|e| e.to_string());
            let failed = next.is_err();
            if records.send(Msg::Record(Box::new(next))).is_err() || failed {
                return;
            }
        }
    });
    thread::spawn(move || {
        for chunk in input {
            if tx.send(Msg::Input(chunk)).is_err() {
                return;
            }
        }
    });

    let result = (|| -> Result<Outcome, Error> {
        out.write_all(&snapshot)?;
        out.flush()?;
        let mut last_size = size();
        if controller {
            writer.resize(last_size.0, last_size.1)?;
        }
        let mut filter = KeyFilter::new(opts.prefix);
        let mut joiner = Utf8Joiner::default();
        loop {
            if stop.load(Ordering::SeqCst) {
                return Ok(Outcome::Interrupted);
            }
            match rx.recv_timeout(TICK) {
                Ok(Msg::Record(rec)) => match *rec {
                    Ok(event) => match event {
                        Event::TerminalOutput { data, .. } => {
                            let bytes = decode_bytes(&data)
                                .map_err(|e| ClientError::Protocol(format!("output: {e}")))?;
                            out.write_all(&bytes)?;
                            out.flush()?;
                        }
                        Event::TerminalExited { exit_code, .. } => {
                            return Ok(Outcome::Exited(exit_code));
                        }
                        Event::TerminalDetached { reason, .. } => {
                            return Ok(Outcome::ServerDetached(reason));
                        }
                        _ => {}
                    },
                    Err(e) => return Ok(Outcome::Closed(e)),
                },
                Ok(Msg::Input(chunk)) => {
                    let filtered = filter.feed(&chunk);
                    if controller && !filtered.forward.is_empty() {
                        writer.input(&joiner.push(&filtered.forward))?;
                    }
                    if filtered.detach {
                        return Ok(Outcome::Detached);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let now = size();
                    if controller && now != last_size {
                        last_size = now;
                        writer.resize(now.0, now.1)?;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Ok(Outcome::Closed("connection ended".into()));
                }
            }
        }
    })();
    writer.close();
    let _ = out.write_all(RESET);
    let _ = out.flush();
    result
}
