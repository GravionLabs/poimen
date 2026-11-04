//! `PATH` of a GUI app. A macOS GUI app starts with `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `node`,
//! `gh` and `herdr` are not found. Children get the login shell's `PATH` plus the usual install
//! directories, and programs are resolved against that.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Directories where `node`, `gh` and `herdr` usually live on macOS and Linux.
fn usual_dirs(home: &str) -> Vec<String> {
    let mut dirs = vec![
        "/opt/homebrew/bin".to_owned(),
        "/usr/local/bin".to_owned(),
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        format!("{home}/.volta/bin"),
        format!("{home}/.local/share/mise/shims"),
        format!("{home}/.asdf/shims"),
    ];
    // nvm keeps one directory per version; take the newest by name.
    if let Ok(entries) = std::fs::read_dir(format!("{home}/.nvm/versions/node")) {
        let mut versions: Vec<String> = entries
            .flatten()
            .map(|e| e.path().join("bin").to_string_lossy().into_owned())
            .collect();
        versions.sort();
        dirs.extend(versions.into_iter().rev().take(1));
    }
    dirs
}

/// The `PATH` for a child: the login shell's, the usual install directories, then the
/// current one; duplicates and empty entries dropped, order kept.
pub fn augmented_path(shell_path: Option<&str>, home: &str, current: &str) -> String {
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    let extra = usual_dirs(home);
    let sources = std::env::split_paths(shell_path.unwrap_or(""))
        .chain(extra.iter().map(PathBuf::from))
        .chain(std::env::split_paths(current));
    for part in sources {
        if !part.as_os_str().is_empty() && seen.insert(part.clone()) {
            parts.push(part);
        }
    }
    std::env::join_paths(&parts)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| current.to_owned())
}

/// Resolves `program` against `path` (a name without slash) the way a shell would.
pub fn resolve_program(program: &str, path: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(program))
        .find(|candidate| candidate.is_file())
}

/// Runs `cmd` (stdout piped) for at most `limit` and extracts the text after `__POIMEN_PATH__`.
/// A background process started by the rc files can keep the pipe open after the shell exited, so
/// the reader is never joined: it fills a shared buffer and is abandoned after a short grace.
fn path_from_command(mut cmd: Command, limit: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let sink = Arc::clone(&buffer);
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut b) = sink.lock() {
                b.extend_from_slice(&chunk[..n]);
            }
        }
        let _ = done_tx.send(());
    });
    let started = Instant::now();
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < limit => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !exited {
        return None;
    }
    // EOF normally follows at once; a pipe held open by a background process ends the wait here.
    let _ = done_rx.recv_timeout(Duration::from_millis(500));
    let bytes = buffer.lock().ok()?.clone();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    text.rsplit_once("__POIMEN_PATH__")
        .map(|(_, path)| path.trim().to_owned())
        .filter(|p| !p.is_empty())
}

/// `PATH` as the user's login shell sees it, read once (`-ilc`, as fix-path does); `None` when the
/// shell fails or takes longer than five seconds.
fn login_shell_path() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            let mut cmd = Command::new(shell);
            cmd.args(["-ilc", "printf '%s' \"__POIMEN_PATH__$PATH\""]);
            path_from_command(cmd, Duration::from_secs(5))
        })
        .clone()
}

/// The `PATH` for a child. Windows has no login shell and a GUI app inherits the full `PATH`, so
/// it is returned as is; elsewhere the login shell is probed (`shell_path`, called only then).
pub fn child_path_for(
    windows: bool,
    shell_path: impl FnOnce() -> Option<String>,
    home: &str,
    current: &str,
) -> String {
    if windows {
        return current.to_owned();
    }
    augmented_path(shell_path().as_deref(), home, current)
}

pub fn child_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let current = std::env::var("PATH").unwrap_or_default();
    child_path_for(cfg!(windows), login_shell_path, &home, &current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_read_returns_even_when_a_child_keeps_the_pipe_open() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 5 & printf '__POIMEN_PATH__/a:/b'"]);
        let started = Instant::now();
        let got = path_from_command(cmd, Duration::from_secs(2));
        assert_eq!(got.as_deref(), Some("/a:/b"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn path_read_gives_up_on_a_shell_that_outlives_the_limit() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 5 & sleep 5"]);
        let started = Instant::now();
        assert_eq!(path_from_command(cmd, Duration::from_millis(200)), None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_path_puts_the_shell_path_first_and_drops_duplicates() {
        let path = augmented_path(Some("/a:/b::/a"), "/nonexistent-home", "/usr/bin:/b");
        let parts: Vec<&str> = path.split(':').collect();
        assert_eq!(&parts[..2], ["/a", "/b"]);
        assert!(parts.contains(&"/opt/homebrew/bin"));
        assert!(parts.contains(&"/usr/bin"));
        assert_eq!(parts.iter().filter(|p| **p == "/b").count(), 1);
        assert!(augmented_path(None, "/h", "").contains("/h/.local/bin"));
    }

    #[test]
    fn windows_keeps_the_inherited_path_and_skips_the_login_shell() {
        let current = r"C:\Windows\System32;C:\Users\me\.cargo\bin";
        let path = child_path_for(true, || panic!("login shell probed"), "", current);
        assert_eq!(path, current);
    }

    #[test]
    fn other_systems_probe_the_login_shell() {
        let path = child_path_for(false, || Some("/a".into()), "/nonexistent-home", "/b");
        assert!(path.starts_with("/a") || cfg!(windows));
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_split_at_semicolons_not_drive_colons() {
        let path = augmented_path(None, r"C:\h", r"C:\a;D:\b");
        let parts: Vec<_> = std::env::split_paths(&path).collect();
        assert!(parts.contains(&PathBuf::from(r"C:\a")));
        assert!(parts.contains(&PathBuf::from(r"D:\b")));
    }

    #[test]
    fn programs_resolve_against_the_given_path() {
        let dir = std::env::temp_dir().join(format!("poimen-pv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fake-node"), "").unwrap();
        let path = format!("/nonexistent:{}", dir.display());
        assert_eq!(
            resolve_program("fake-node", &path),
            Some(dir.join("fake-node"))
        );
        assert_eq!(resolve_program("missing-node", &path), None);
        assert_eq!(
            resolve_program("/abs/node", ""),
            Some(PathBuf::from("/abs/node"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
