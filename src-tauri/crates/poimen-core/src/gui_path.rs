//! `PATH` of a GUI app. A macOS GUI app starts with `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `node`,
//! `gh` and `herdr` are not found. Children get the login shell's `PATH` plus the usual install
//! directories, and programs are resolved against that.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// A `Command` for a child of poimen. The release build on Windows is a GUI app without a console,
/// so every console program it starts would open its own window unless `CREATE_NO_WINDOW` is set.
/// All spawn sites go through here (clippy forbids `Command::new` elsewhere); no-op off Windows.
#[allow(clippy::disallowed_methods)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Windows process creation flag: do not open a console window for the child.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

/// File names to try for `program`: on Windows one per `PATHEXT` extension (default
/// `.COM;.EXE;.BAT;.CMD`, lowercased: `herdr.exe`) first, then the bare name; elsewhere just the
/// bare name. A name that already ends in a `PATHEXT` extension stays as it is.
pub fn executable_names(program: &str, windows: bool, pathext: Option<&str>) -> Vec<String> {
    if !windows {
        return vec![program.to_owned()];
    }
    let exts: Vec<String> = pathext
        .filter(|e| !e.is_empty())
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .map(str::to_lowercase)
        .collect();
    let lower = program.to_lowercase();
    if exts.iter().any(|e| lower.ends_with(e.as_str())) {
        return vec![program.to_owned()];
    }
    let mut names: Vec<String> = exts.iter().map(|e| format!("{program}{e}")).collect();
    names.push(program.to_owned());
    names
}

/// Resolves `program` against `path` the way a shell would: a name with a separator (`/`, and `\`
/// on Windows) is returned as is, a bare name is searched in each directory of `path`.
pub fn resolve_program(program: &str, path: &str) -> Option<PathBuf> {
    let pathext = std::env::var("PATHEXT").ok();
    resolve_program_with(program, path, cfg!(windows), pathext.as_deref(), |p| {
        p.is_file()
    })
}

pub fn resolve_program_with(
    program: &str,
    path: &str,
    windows: bool,
    pathext: Option<&str>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    if program.contains(separators) {
        return Some(PathBuf::from(program));
    }
    let names = executable_names(program, windows, pathext);
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|candidate| exists(candidate))
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
            let mut cmd = command(shell);
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

    /// A `PATH` of `parts` with the separator of this system.
    fn join(parts: &[&str]) -> String {
        std::env::join_paths(parts)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn split(path: &str) -> Vec<PathBuf> {
        std::env::split_paths(path).collect()
    }

    #[cfg(unix)]
    #[test]
    fn path_read_returns_even_when_a_child_keeps_the_pipe_open() {
        let mut cmd = command("sh");
        cmd.args(["-c", "sleep 5 & printf '__POIMEN_PATH__/a:/b'"]);
        let started = Instant::now();
        let got = path_from_command(cmd, Duration::from_secs(2));
        assert_eq!(got.as_deref(), Some("/a:/b"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn path_read_gives_up_on_a_shell_that_outlives_the_limit() {
        let mut cmd = command("sh");
        cmd.args(["-c", "sleep 5 & sleep 5"]);
        let started = Instant::now();
        assert_eq!(path_from_command(cmd, Duration::from_millis(200)), None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_path_puts_the_shell_path_first_and_drops_duplicates() {
        let path = augmented_path(
            Some(&join(&["/a", "/b", "", "/a"])),
            "/nonexistent-home",
            &join(&["/usr/bin", "/b"]),
        );
        let parts = split(&path);
        assert_eq!(parts[..2], [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert!(parts.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(parts.contains(&PathBuf::from("/usr/bin")));
        assert_eq!(parts.iter().filter(|p| **p == Path::new("/b")).count(), 1);
        let home = Path::new("/h");
        assert!(split(&augmented_path(None, "/h", "")).contains(&home.join(".local/bin")));
    }

    #[test]
    fn windows_keeps_the_inherited_path_and_skips_the_login_shell() {
        let current = r"C:\Windows\System32;C:\Users\me\.cargo\bin";
        let path = child_path_for(true, || panic!("login shell probed"), "", current);
        assert_eq!(path, current);
    }

    #[test]
    fn other_systems_probe_the_login_shell() {
        let path = child_path_for(
            false,
            || Some(join(&["/a"])),
            "/nonexistent-home",
            &join(&["/b"]),
        );
        let parts = split(&path);
        assert_eq!(parts[0], PathBuf::from("/a"));
        assert!(parts.contains(&PathBuf::from("/b")));
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_split_at_semicolons_not_drive_colons() {
        let path = augmented_path(None, r"C:\h", r"C:\a;D:\b");
        let parts = split(&path);
        assert!(parts.contains(&PathBuf::from(r"C:\a")));
        assert!(parts.contains(&PathBuf::from(r"D:\b")));
    }

    #[test]
    fn executable_names_try_the_pathext_names_before_the_bare_one_on_windows() {
        assert_eq!(executable_names("herdr", false, Some(".EXE")), ["herdr"]);
        assert_eq!(
            executable_names("herdr", true, Some(".COM;.EXE;;exe")),
            ["herdr.com", "herdr.exe", "herdr"]
        );
        assert_eq!(
            executable_names("herdr", true, None),
            ["herdr.com", "herdr.exe", "herdr.bat", "herdr.cmd", "herdr"]
        );
        assert_eq!(executable_names("node.EXE", true, None), ["node.EXE"]);
    }

    #[test]
    fn programs_resolve_against_the_given_path_with_the_given_names() {
        let path = join(&["/nonexistent", "/win/bin"]);
        let found = resolve_program_with("cmd", &path, true, None, |p| {
            p == Path::new("/win/bin").join("cmd.exe")
        });
        assert_eq!(found, Some(Path::new("/win/bin").join("cmd.exe")));
        assert_eq!(
            resolve_program_with("cmd", &path, true, None, |_| false),
            None
        );
        let unix = resolve_program_with("node", &path, false, None, |p| {
            p == Path::new("/win/bin").join("node")
        });
        assert_eq!(unix, Some(Path::new("/win/bin").join("node")));
    }

    #[test]
    fn a_program_with_a_separator_is_returned_as_is() {
        assert_eq!(
            resolve_program_with("/abs/node", "", false, None, |_| false),
            Some(PathBuf::from("/abs/node"))
        );
        assert_eq!(
            resolve_program_with(r"C:\bin\node", "", true, None, |_| false),
            Some(PathBuf::from(r"C:\bin\node"))
        );
        // A backslash is an ordinary file name character outside Windows.
        assert_eq!(
            resolve_program_with(r"a\b", &join(&["/d"]), false, None, |_| false),
            None
        );
    }

    #[test]
    fn programs_resolve_against_the_real_file_system() {
        let dir = std::env::temp_dir().join(format!("poimen-pv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fake-node"), "").unwrap();
        let path = join(&["/nonexistent", &dir.to_string_lossy()]);
        assert_eq!(
            resolve_program("fake-node", &path),
            Some(dir.join("fake-node"))
        );
        assert_eq!(resolve_program("missing-node", &path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
