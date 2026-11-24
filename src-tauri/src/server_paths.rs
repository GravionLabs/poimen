//! Where the poimen-server of this build keeps its socket and its log (the Server settings page),
//! and the message of a server that did not come up (#967): a `server.json` of a newer version
//! makes the server refuse to start and say so in `server.log`; the app shows that line, else the
//! path of the log.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::backend::StartError;
use crate::poimen_server::{self, START_TIMEOUT};

/// The socket of the poimen-server this build talks to.
#[tauri::command]
pub fn server_socket_path() -> String {
    poimen_client::protocol::socket_path()
        .to_string_lossy()
        .into_owned()
}

/// The `server.log` of the poimen-server this build talks to, next to its `server.json`.
#[tauri::command]
pub fn server_log_path() -> String {
    poimen_client::protocol::files::state_file_path()
        .with_file_name("server.log")
        .to_string_lossy()
        .into_owned()
}

const MARKER: &str = " server refusing to start: ";

/// The message of the last `refusing to start` line of `log` when the file was written within
/// `window` before `now`.
pub fn refusal_in(log: &Path, now: SystemTime, window: Duration) -> Option<String> {
    let modified = std::fs::metadata(log).ok()?.modified().ok()?;
    if now.duration_since(modified).is_ok_and(|age| age > window) {
        return None;
    }
    let text = std::fs::read_to_string(log).ok()?;
    let (_, message) = text.lines().next_back()?.split_once(MARKER)?;
    Some(message.to_string())
}

/// The start failure to show: the server's own message when it refused to run, else the error
/// with the path of `server.log` for a start that timed out.
pub fn describe(error: poimen_server::StartError) -> StartError {
    let log = poimen_client::protocol::files::state_file_path().with_file_name("server.log");
    describe_with(error, &log, SystemTime::now())
}

fn describe_with(error: poimen_server::StartError, log: &Path, now: SystemTime) -> StartError {
    if matches!(error, poimen_server::StartError::Timeout(_)) {
        return StartError::Other(match refusal_in(log, now, START_TIMEOUT * 2) {
            Some(message) => message,
            None => format!("{error}; see {}", log.display()),
        });
    }
    StartError::Other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;

    fn log_with(name: &str, text: &str) -> (TestDir, std::path::PathBuf) {
        let dir = TestDir::new(&format!("poimen-fail-{name}-"));
        let log = dir.join("server.log");
        std::fs::write(&log, text).unwrap();
        (dir, log)
    }

    #[test]
    fn a_timeout_shows_the_refusal_of_the_server() {
        let (_dir, log) = log_with(
            "refused",
            "2026-11-20T06:00:00Z server started\n2026-11-20T06:01:00Z server refusing to start: server.json has version 9\n",
        );
        let error = poimen_server::StartError::Timeout(START_TIMEOUT);
        assert_eq!(
            describe_with(error, &log, SystemTime::now()),
            StartError::Other("server.json has version 9".into())
        );
    }

    #[test]
    fn a_timeout_without_a_refusal_names_the_log() {
        let (_dir, log) = log_with("plain", "2026-11-20T06:00:00Z server started\n");
        let StartError::Other(message) = describe_with(
            poimen_server::StartError::Timeout(START_TIMEOUT),
            &log,
            SystemTime::now(),
        );
        assert!(message.contains("did not answer"), "{message}");
        assert!(message.contains(&log.display().to_string()), "{message}");
    }

    #[test]
    fn an_old_refusal_is_not_this_start() {
        let (_dir, log) = log_with(
            "old",
            "2026-11-20T06:01:00Z server refusing to start: old\n",
        );
        let later = SystemTime::now() + START_TIMEOUT * 10;
        assert_eq!(refusal_in(&log, later, START_TIMEOUT * 2), None);
        assert_eq!(
            refusal_in(
                &log.with_file_name("none.log"),
                SystemTime::now(),
                START_TIMEOUT
            ),
            None
        );
    }

    #[test]
    fn other_errors_keep_their_text() {
        let (_dir, log) = log_with("other", "");
        assert_eq!(
            describe_with(poimen_server::StartError::NotFound, &log, SystemTime::now()),
            StartError::Other(poimen_server::StartError::NotFound.to_string())
        );
    }
}
