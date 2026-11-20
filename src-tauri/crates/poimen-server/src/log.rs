//! The log files of the server: `server.log` next to `server.json`, the `hooks.log` of
//! [`crate::hook`] and the `hosts/<terminal id>.log` of every PTY host.
//!
//! A line is `<UTC timestamp> <component> <message>`. Terminal output, input, prompts and
//! environment values are never logged. A full log moves to `<name>.1` (replacing an older one),
//! the files are private (mode 0600). Writing never fails the caller.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use poimen_protocol::files::utc_now;
use poimen_protocol::host::{HostMessage, TreeEnd};

/// Size at which a log moves to `<name>.1`.
pub const MAX_LOG: u64 = 256 * 1024;
const SERVER_LOG: &str = "server.log";
/// How long a host log is kept when nothing removed it.
pub const HOST_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// `server.log` next to the state file.
pub fn path(state_file: &Path) -> PathBuf {
    state_file.with_file_name(SERVER_LOG)
}

/// The log of the host at `endpoint`: the endpoint with `.log` appended (`hosts/<id>.log`).
pub fn host_path(endpoint: &Path) -> PathBuf {
    let mut name = endpoint.as_os_str().to_owned();
    name.push(".log");
    PathBuf::from(name)
}

/// Writes `<UTC timestamp> <component> <message>` to stderr and to `log`.
pub fn line(log: &Path, component: &str, message: &str) {
    let line = format!("{} {component} {message}", utc_now());
    eprintln!("{line}");
    append(log, &line);
}

/// Logs an `exit` of a host whose process tree is unconfirmed (#968).
pub fn tree_end(log: &Path, endpoint: &Path, message: &HostMessage) {
    if !matches!(
        message,
        HostMessage::Exit {
            tree: Some(TreeEnd::Unconfirmed),
            ..
        }
    ) {
        return;
    }
    let id = endpoint
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let id = id.unwrap_or_else(|| endpoint.display().to_string());
    line(
        log,
        "host",
        &format!("terminal {id} ended, process tree unconfirmed"),
    );
}

/// Appends `line` to the log, moving a full log to `<name>.1` first. Never fails the caller.
pub fn append(log: &Path, line: &str) {
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(log).is_ok_and(|m| m.len() >= MAX_LOG) {
        let mut old = log.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(log, PathBuf::from(old));
    }
    if let Some(mut file) = open(log) {
        let _ = writeln!(file, "{line}");
    }
}

/// Opens `log` for appending, created private (0600).
pub fn open(log: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(log).ok()
}

/// Removes the `*.log` files in `dir` that were last written more than `age` ago.
pub fn prune_old(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "log") {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|d| d > age);
        if old {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-log-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_an_unconfirmed_tree_is_logged_with_the_terminal_id() {
        let dir = temp("tree");
        let log = dir.join("server.log");
        let endpoint = dir.join("hosts").join("t7");
        let exit = |tree| HostMessage::Exit {
            code: Some(0),
            tree,
        };
        tree_end(&log, &endpoint, &exit(None));
        tree_end(&log, &endpoint, &exit(Some(TreeEnd::Ended)));
        assert!(!log.exists());
        tree_end(&log, &endpoint, &exit(Some(TreeEnd::Unconfirmed)));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains("terminal t7 ended, process tree unconfirmed"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_log_sits_next_to_the_state_file() {
        assert_eq!(
            path(Path::new("/s/poimen/server.json")),
            PathBuf::from("/s/poimen/server.log")
        );
        assert_eq!(
            host_path(Path::new("/s/poimen/hosts/t1")),
            PathBuf::from("/s/poimen/hosts/t1.log")
        );
    }

    #[test]
    fn a_line_is_timestamp_component_message() {
        let dir = temp("line");
        let log = dir.join("server.log");
        line(&log, "server", "hello there");
        let text = std::fs::read_to_string(&log).unwrap();
        let parts: Vec<&str> = text.trim_end().splitn(3, ' ').collect();
        assert_eq!(parts[1..], ["server", "hello there"]);
        let stamp = parts[0].as_bytes();
        assert_eq!(stamp.len(), 20);
        assert!(stamp[10] == b'T' && stamp[19] == b'Z');
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&log).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_full_log_moves_to_dot_one_replacing_an_older_one() {
        let dir = temp("rotate");
        let log = dir.join("server.log");
        std::fs::write(&log, vec![b'x'; usize::try_from(MAX_LOG).unwrap()]).unwrap();
        std::fs::write(dir.join("server.log.1"), "older").unwrap();
        append(&log, "new");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "new\n");
        assert_eq!(
            std::fs::metadata(dir.join("server.log.1")).unwrap().len(),
            MAX_LOG
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_old_logs_are_pruned() {
        let dir = temp("prune");
        let old = SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
        for name in ["old.log", "plain"] {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            file.set_modified(old).unwrap();
        }
        std::fs::write(dir.join("new.log"), "x").unwrap();
        prune_old(&dir, HOST_LOG_AGE);
        assert!(!dir.join("old.log").exists());
        assert!(dir.join("new.log").exists());
        assert!(dir.join("plain").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
