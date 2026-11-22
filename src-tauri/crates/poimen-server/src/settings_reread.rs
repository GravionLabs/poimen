//! The user config values the server reads again while it runs: `agents.silentAfter` (#898)
//! and `terminalHistory` (#1196).

use std::time::{Duration, Instant};

use crate::{SETTINGS_REREAD, Shared, history, lock};

/// How often the user config is read again for `terminalHistory` (#1196).
const HISTORY_REREAD: Duration = Duration::from_secs(2);

impl Shared {
    /// The `agents.silentAfter` setting in minutes, read from the user config at most every 30 s
    /// (#898); without a home it is the default.
    pub(crate) fn silent_after(&self, now: Instant) -> u64 {
        let mut cached = lock(&self.silent_after);
        if let Some((at, minutes)) = *cached
            && now.duration_since(at) < SETTINGS_REREAD
        {
            return minutes;
        }
        let minutes = match self.home.as_deref() {
            Some(home) => std::fs::read_to_string(poimen_core::settings::config_path(home))
                .map_or(poimen_core::settings::DEFAULT_SILENT_AFTER_MIN, |t| {
                    poimen_core::settings::silent_after_text(&t)
                }),
            None => poimen_core::settings::DEFAULT_SILENT_AFTER_MIN,
        };
        *cached = Some((now, minutes));
        minutes
    }

    /// Applies `terminalHistory` of the user config to the history writer, reading the file at
    /// most every 2 s (#1196); without a home the defaults apply.
    pub(crate) fn refresh_history(&self, now: Instant) {
        {
            let mut read = lock(&self.history_read);
            if read.is_some_and(|at| now.duration_since(at) < HISTORY_REREAD) {
                return;
            }
            *read = Some(now);
        }
        // No file is the default; a file that cannot be read or parsed (half-edited) keeps what
        // was applied before, so it never turns a history that is off back on.
        let text = self
            .home
            .as_deref()
            .map(|home| std::fs::read_to_string(poimen_core::settings::config_path(home)));
        let setting = match text {
            None => Default::default(),
            Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
            Some(Err(_)) => return,
            Some(Ok(t)) => match poimen_core::terminal_history::from_text(&t) {
                Some(setting) => setting,
                None => return,
            },
        };
        const MIB: u64 = 1024 * 1024;
        self.history.configure(
            setting.enabled,
            history::Limits {
                per_terminal: setting.per_terminal_mib * MIB,
                total: setting.total_mib * MIB,
            },
        );
    }
}
