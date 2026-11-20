//! Files of `poimen-server`: where its state directory is (`server.json`, the logs) and the time
//! stamps of the log lines.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Channel, windows_dir_name};

/// `%LOCALAPPDATA%\poimen-server` (on rc `poimen-server-rc`) when `LOCALAPPDATA` is set and not
/// empty.
pub fn windows_dir_from(
    local_app_data: Option<std::ffi::OsString>,
    channel: Channel,
) -> Option<PathBuf> {
    local_app_data
        .filter(|s| !s.is_empty())
        .map(|dir| PathBuf::from(dir).join(windows_dir_name(channel)))
}

/// Seconds since the epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `YYYY-MM-DDTHH:MM:SSZ` of now.
pub fn utc_now() -> String {
    utc(now_secs())
}

/// `YYYY-MM-DDTHH:MM:SSZ` of seconds since the epoch (proleptic Gregorian, days to civil date).
pub fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Unix: `$POIMEN_STATE_DIR/server.json`, else `$XDG_STATE_HOME/poimen/server.json`, else
/// `~/.local/state/poimen/server.json`. Windows: `$POIMEN_STATE_DIR\server.json`, else
/// `%LOCALAPPDATA%\poimen-server\server.json`, else `~\AppData\Local\poimen-server\server.json`.
/// An rc build uses `poimen-rc` and `poimen-server-rc` ([`crate::CHANNEL`]).
pub fn state_file_path() -> PathBuf {
    let channel = crate::CHANNEL;
    if cfg!(windows) {
        windows_state_file_from(
            std::env::var_os("POIMEN_STATE_DIR"),
            std::env::var_os("LOCALAPPDATA"),
            std::env::home_dir(),
            channel,
        )
    } else {
        state_file_from(
            std::env::var_os("POIMEN_STATE_DIR"),
            std::env::var_os("XDG_STATE_HOME"),
            std::env::var_os("HOME"),
            channel,
        )
    }
}

/// The Windows [`state_file_path`] with the environment passed in; empty values count as unset.
pub fn windows_state_file_from(
    state_dir: Option<std::ffi::OsString>,
    local_app_data: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
    channel: Channel,
) -> PathBuf {
    if let Some(dir) = state_dir.filter(|s| !s.is_empty()) {
        return PathBuf::from(dir).join("server.json");
    }
    crate::windows_dir_from(local_app_data, channel)
        .unwrap_or_else(|| {
            home.filter(|h| !h.as_os_str().is_empty())
                .unwrap_or_else(std::env::temp_dir)
                .join("AppData")
                .join("Local")
                .join(crate::windows_dir_name(channel))
        })
        .join("server.json")
}

pub fn state_file_from(
    state_dir: Option<std::ffi::OsString>,
    xdg_state: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    channel: Channel,
) -> PathBuf {
    let set = |v: Option<std::ffi::OsString>| v.filter(|s| !s.is_empty()).map(PathBuf::from);
    if let Some(dir) = set(state_dir) {
        return dir.join("server.json");
    }
    let dir = channel.named("poimen");
    if let Some(state) = set(xdg_state) {
        return state.join(dir).join("server.json");
    }
    set(home)
        .unwrap_or_else(std::env::temp_dir)
        .join(".local")
        .join("state")
        .join(dir)
        .join("server.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
