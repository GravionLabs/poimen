//! What a record of a pane's attachment changes.

use std::sync::mpsc::Sender;

use poimen_client::Client;
use poimen_client::protocol::{DetachReason, Event, decode_bytes};

use super::{App, How, Msg, attach_pane, select};

pub(super) fn record(
    app: &mut App,
    client: &Client,
    tx: &Sender<Msg>,
    generation: u64,
    rec: Result<Event, String>,
) {
    let Some(at) = app.panes.iter().position(|p| p.generation == generation) else {
        return;
    };
    let focused = app.selected.as_deref() == Some(app.panes[at].terminal_id.as_str());
    match rec {
        Ok(Event::TerminalOutput { data, .. }) => {
            let pane = &mut app.panes[at];
            if let Ok(bytes) = decode_bytes(&data) {
                pane.grid.feed(&bytes);
                let replies = pane.grid.take_replies(&app.palette);
                if pane.controls() && !replies.is_empty() {
                    let _ = pane.writer.input(&String::from_utf8_lossy(&replies));
                }
            }
        }
        Ok(Event::TerminalResized { cols, rows, .. }) => app.panes[at].grid.resize(cols, rows),
        Ok(Event::TerminalExited { exit_code, .. }) => app.panes[at].exited = Some(exit_code),
        Ok(Event::TerminalDetached { reason, .. }) => {
            let id = app.panes[at].terminal_id.clone();
            match reason {
                DetachReason::TakenOver => {
                    attach_pane(app, client, tx, &id, How::Observe);
                    if focused {
                        app.notice = Some("another client took over the control; observing".into());
                    }
                }
                DetachReason::Overflow if focused => {
                    select(app, client, tx, Some(id), How::Control);
                }
                DetachReason::Overflow => attach_pane(app, client, tx, &id, How::Observe),
                DetachReason::Closed => {
                    app.panes.remove(at);
                }
            }
        }
        Ok(_) => {}
        // The server ends the stream of an exited terminal; its leaf may stay (#1305), and then
        // so does the last screen. A leaf that went is dropped by `follow_server`.
        Err(_) if app.panes[at].exited.is_some() => {}
        Err(e) => {
            app.notice = Some(format!("attachment ended: {e}"));
            app.panes.remove(at);
        }
    }
}
