//! The grid of a terminal rebuilt from its history log (#1194): the newest valid checkpoint,
//! then the records after it. A checkpoint that cannot be restored is skipped for the one
//! before it, and with none the log is replayed from its start. Never fails: a missing or
//! damaged log gives `None` or what its valid part holds.

use std::path::Path;

use crate::emulator::{Colors, Emulator};
use crate::history::{Checkpoint, Record, read};

/// Where a rebuild ends.
pub(crate) struct Rebuilt {
    pub emulator: Emulator,
    /// The host `seq` the log covers: the records up to it are in the grid.
    pub seq: u64,
}

/// The largest side of a checkpoint's grid; more is damage.
const MAX_SIDE: u16 = 10_000;

/// An emulator of the checkpoint's size at its state, `None` when it does not restore.
pub(crate) fn restore(checkpoint: &Checkpoint, scrollback: usize) -> Option<Emulator> {
    let (cols, rows) = (checkpoint.cols, checkpoint.rows);
    if cols == 0 || rows == 0 || cols > MAX_SIDE || rows > MAX_SIDE {
        return None;
    }
    if !checkpoint.grid.starts_with(b"\x1bc") {
        return None;
    }
    let mut emulator = Emulator::new(cols, rows, scrollback);
    emulator.feed(&checkpoint.grid);
    quiet(&mut emulator);
    (emulator.size() == (cols, rows)).then_some(emulator)
}

/// Forgets what the bytes fed so far would have caused: no status signal, no answer to a query.
fn quiet(emulator: &mut Emulator) {
    emulator.take_signals();
    emulator.take_answers(&Colors::default());
}

/// Rebuilds the grid from the log at `path`. `initial` is the size to start a log without a
/// checkpoint with. `None` when the log holds nothing.
pub(crate) fn rebuild(path: &Path, initial: (u16, u16), scrollback: usize) -> Option<Rebuilt> {
    let records = read(path);
    if records.is_empty() {
        return None;
    }
    let (mut emulator, mut seq, from) = records
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, record)| match record {
            Record::Checkpoint(c) => restore(c, scrollback).map(|e| (e, c.seq, at + 1)),
            _ => None,
        })
        .unwrap_or_else(|| (Emulator::new(initial.0, initial.1, scrollback), 0, 0));
    let mut tail = &records[from..];
    // Output a crash left without its `seq` would be read from the host's ring again.
    if records.iter().any(|r| matches!(r, Record::Seq(_))) {
        while let [rest @ .., Record::Output(_)] = tail {
            tail = rest;
        }
    }
    // Dropped records: the log cannot say what the grid missed since the checkpoint.
    if tail.iter().any(|r| matches!(r, Record::Hole)) {
        return None;
    }
    for record in tail {
        match record {
            Record::Output(bytes) => emulator.feed(bytes),
            Record::Resize { cols, rows } => emulator.resize(*cols, *rows),
            Record::Hole => {}
            Record::Seq(s) => seq = seq.max(*s),
            // An invalid one: its records are replayed from the one before.
            Record::Checkpoint(c) => seq = seq.max(c.seq),
        }
    }
    quiet(&mut emulator);
    Some(Rebuilt { emulator, seq })
}

#[cfg(test)]
#[path = "rebuild_tests.rs"]
mod tests;
