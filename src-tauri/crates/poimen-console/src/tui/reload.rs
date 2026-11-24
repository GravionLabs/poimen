//! Reloading `config.json` and the user schemes while the TUI runs (#1175).

use std::path::Path;
use std::sync::mpsc::Sender;

use poimen_client::Client;

use super::grid::Palette;
use super::input::Router;
use super::{App, Msg, Options, config};

/// Watches the config directory; every change sends [`Msg::ConfigChanged`]. Dropping the
/// result stops the watch.
pub(super) fn watch(home: Option<&str>, tx: &Sender<Msg>) -> Option<notify::RecommendedWatcher> {
    let tx = tx.clone();
    config::watch(Path::new(home?), move || drop(tx.send(Msg::ConfigChanged)))
}

/// Reads `config.json` and the schemes again and applies them to the app and the router. An
/// invalid file keeps the last valid config and says why in the status line. Messages are
/// handled one at a time, so a key pressed meanwhile is routed by the old or the new config
/// and a pending prefix survives.
pub(super) fn apply(app: &mut App, router: &mut Router, opts: &Options, client: &Client) {
    // The file changes in several steps (truncate, write, rename): a partial read is invalid
    // and the next event reads it again.
    match config::try_load(opts.home.as_deref().map(Path::new)) {
        Err(e) => app.notice = Some(format!("config: {e}")),
        Ok(new) if new == app.config => {}
        Ok(new) => {
            app.notice = Some(match new.warnings.first() {
                Some(w) => format!("config reloaded: {w}"),
                None => "config reloaded".to_owned(),
            });
            router.reconfigure(&new);
            app.palette = Palette::from_scheme(new.scheme.as_ref());
            app.config = new;
            if let Err(e) = app.refresh(client) {
                app.notice = Some(format!("config reloaded, sidebar not refreshed: {e}"));
            }
        }
    }
}
