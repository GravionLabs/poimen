//! The attach types of `terminal.attach`.

use serde::{Deserialize, Serialize};

/// How a connection attaches to a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachMode {
    /// Receives the output and size changes; cannot send input or resize.
    Observe,
    /// The one connection that sends input and sets the size.
    Control,
}

/// Result of `terminal.attach`. The records of the stream have a `seq` greater than `seq` here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachResult {
    pub terminal_id: String,
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
    pub mode: AttachMode,
    /// Base64 of the bytes that bring a fresh emulator of `cols` x `rows` to the current state
    /// (screen, scrollback, modes, cursor, title). Write it to the emulator as is.
    pub snapshot: String,
    /// The terminal has ended and the snapshot is its history from the log (#1195): `mode` is
    /// `observe`, no controller is possible and input is refused. Absent (false) otherwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

/// Why the server ended an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetachReason {
    /// The client did not keep up; attach again for a fresh snapshot.
    Overflow,
    /// Another connection took over the control.
    TakenOver,
    /// The terminal was closed.
    Closed,
}
