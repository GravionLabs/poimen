//! Input while the command palette is open: the palette takes every key, but a detach must not
//! wait for what the palette waits for (#1355).

use std::sync::mpsc::Sender;

use crossterm::event::Event as InputEvent;
use poimen_client::Client;

use super::input::{Modes, Routed, Router};
use super::{App, Msg, Outcome, palette, run_command};

/// Hands `ev` to the open palette. While it is busy (reading the issues, starting a task) it has
/// nothing to answer, so the detach keys end the TUI; a prefix pressed then stays good when the
/// result changes the palette before the next key.
pub(super) fn on_input(
    app: &mut App,
    router: &mut Router,
    client: &Client,
    tx: &Sender<Msg>,
    ev: &InputEvent,
) -> Option<Outcome> {
    let busy = app.command_palette.as_ref().is_some_and(|p| p.is_busy());
    if busy || router.pending() {
        let routed = router.route(ev, Modes::default());
        app.prefix_pending = router.pending();
        if matches!(routed, Some(Routed::Detach)) {
            return Some(Outcome::Detached);
        }
    }
    match ev {
        InputEvent::Key(key) => {
            let out = app.command_palette.as_mut().and_then(|p| p.key(key));
            match out {
                Some(palette::Out::Close) => app.command_palette = None,
                Some(palette::Out::Run(command)) => run_command(app, client, tx, command),
                None => {}
            }
        }
        InputEvent::Paste(text) => {
            if let Some(p) = app.command_palette.as_mut() {
                p.paste(text);
            }
        }
        _ => {}
    }
    None
}
