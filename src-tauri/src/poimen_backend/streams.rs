//! The open streams of the app and their prompt drafts (#1163): which terminals hold a draft, and
//! the streams that tell the server. A stream opened later (a reattach, a new mode) tells it
//! again, so the guard outlives one connection.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use poimen_client::AttachmentWriter;

use super::{PoimenBackend, lock, text};

/// One open stream: the write half of its attachment.
pub(super) struct Stream {
    pub(super) terminal_id: String,
    pub(super) writer: Mutex<AttachmentWriter>,
}

impl PoimenBackend {
    pub(super) fn stream(&self, id: u32) -> Result<Arc<Stream>, String> {
        lock(&self.streams)
            .get(&id)
            .cloned()
            .ok_or_else(|| "the terminal stream is closed".to_owned())
    }
}

/// The terminals whose prompt bar holds text.
pub(super) type Drafts = Mutex<HashSet<String>>;

/// Remembers the draft of `terminal_id` and tells the server on every open stream of it.
pub(super) fn set(backend: &PoimenBackend, terminal_id: &str, draft: bool) -> Result<(), String> {
    {
        let mut held = lock(&backend.drafts);
        if draft {
            held.insert(terminal_id.to_owned());
        } else {
            held.remove(terminal_id);
        }
    }
    let streams: Vec<_> = lock(&backend.streams)
        .values()
        .filter(|s| s.terminal_id == terminal_id)
        .cloned()
        .collect();
    streams
        .iter()
        .try_for_each(|s| lock(&s.writer).set_draft(draft).map_err(text))
}

/// A new stream of `terminal_id` tells the server about a draft that is still held.
pub(super) fn restore(backend: &PoimenBackend, terminal_id: &str, writer: &mut AttachmentWriter) {
    if lock(&backend.drafts).contains(terminal_id) {
        let _ = writer.set_draft(true);
    }
}
