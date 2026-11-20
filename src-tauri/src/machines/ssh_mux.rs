//! One ssh connection per machine (#1012): every ssh call of a machine (the connection, the
//! install steps) shares a master through `ControlMaster=auto`, so only the first pays the
//! handshake.
//!
//! The control sockets live in a directory only the user can read (`/tmp/poimen-ssh-<uid>`: a
//! short path, a socket path is limited to about 100 bytes). Its owner and mode are checked
//! before every use; a wrong one turns the reuse off with a warning, ssh still runs. A user's
//! own `ControlMaster` or `ControlPath` for the host wins (read with `ssh -G`), and Windows'
//! OpenSSH has no multiplexing, so nothing is added there. A stale socket (the master died) is
//! removed and the call is tried once more.

use std::path::{Path, PathBuf};

/// How long the master stays after its last client (seconds).
const PERSIST: &str = "600";

/// What the fixed part of a command line needs to know about the host, from `ssh -G`.
#[derive(Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The user sets `ControlMaster` (other than `no`) or a `ControlPath` for this host.
    pub user_controls: bool,
    /// The expanded `ControlPath` of our options, when `-G` was given them.
    pub socket: Option<PathBuf>,
}

/// Reads the output of `ssh -G`: `controlmaster` is `false` and `controlpath` is `none` unless
/// the user's config sets them.
pub fn resolved(g_output: &str) -> Resolved {
    let value = |key: &str| {
        g_output.lines().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            (k == key).then(|| v.trim().to_owned())
        })
    };
    let master = value("controlmaster").unwrap_or_else(|| "false".into());
    let path = value("controlpath").filter(|p| p != "none");
    Resolved {
        user_controls: master != "false" || path.is_some(),
        socket: path.map(PathBuf::from),
    }
}

/// The directory facts the check needs (a fake in tests).
pub struct DirInfo {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub uid: u32,
    pub mode: u32,
}

/// Only the owner may read the directory: a real directory, ours, no bit for group or others.
pub fn check_dir(info: &DirInfo, uid: u32) -> Result<(), String> {
    if info.is_symlink || !info.is_dir {
        return Err("is not a plain directory".into());
    }
    if info.uid != uid {
        return Err(format!("is owned by user {}, not by you", info.uid));
    }
    if info.mode & 0o077 != 0 {
        return Err(format!("has mode {:o}, expected 700", info.mode & 0o777));
    }
    Ok(())
}

/// The ssh options of the reuse for sockets in `dir`.
pub fn options(dir: &Path) -> Vec<String> {
    [
        "-o".to_owned(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPersist={PERSIST}"),
        "-o".into(),
        format!("ControlPath={}/%C", dir.display()),
    ]
    .into()
}

/// Whether ssh's output says the control socket is stale (the master is gone).
pub fn is_stale(output: &str) -> bool {
    let out = output.to_ascii_lowercase();
    out.contains("control socket connect")
        || out.contains("mux_client_request_session")
        || out.contains("mux_client_hello_exchange")
        || out.contains("muxserver_listen")
}

/// Removes `socket` when it is a socket file (never anything else); true when one was removed.
pub fn remove_stale(socket: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if std::fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()) {
            return std::fs::remove_file(socket).is_ok();
        }
    }
    let _ = socket;
    false
}

/// The checked socket directory, created `700` when missing; `Err` is the reason the reuse is
/// off.
#[cfg(unix)]
pub fn socket_dir() -> Result<PathBuf, String> {
    let uid = current_uid().map_err(|e| format!("cannot find your user id: {e}"))?;
    ensure_dir(&PathBuf::from(format!("/tmp/poimen-ssh-{uid}")), uid)
}

#[cfg(not(unix))]
pub fn socket_dir() -> Result<PathBuf, String> {
    Err("OpenSSH on this system has no connection sharing".into())
}

/// The effective user id, read from the owner of a file this process creates (the code base
/// forbids `unsafe`, so no `geteuid`).
#[cfg(unix)]
fn current_uid() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    let probe = std::env::temp_dir().join(format!(".poimen-uid-{}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);
    let uid = file.and_then(|f| f.metadata()).map(|m| m.uid());
    let _ = std::fs::remove_file(&probe);
    uid
}

#[cfg(unix)]
fn ensure_dir(dir: &Path, uid: u32) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let made = std::fs::DirBuilder::new().mode(0o700).create(dir);
    if let Err(e) = made
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(format!("cannot create {}: {e}", dir.display()));
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let info = DirInfo {
        is_dir: meta.is_dir(),
        is_symlink: meta.file_type().is_symlink(),
        uid: meta.uid(),
        mode: meta.mode(),
    };
    check_dir(&info, uid).map_err(|why| format!("{} {why}", dir.display()))?;
    Ok(dir.to_owned())
}

/// What a caller adds to its ssh command line for `target`, and the socket to clean up when the
/// call fails with a stale one.
pub struct Mux {
    pub options: Vec<String>,
    pub socket: Option<PathBuf>,
}

impl Mux {
    pub const OFF: Mux = Mux {
        options: Vec::new(),
        socket: None,
    };
}

/// Decides the options for `target`. `resolve` runs `ssh -G <options> -- target` and returns its
/// stdout (None when ssh could not be run); it is how a user's own setting is found.
pub fn plan(dir: Result<PathBuf, String>, resolve: impl Fn(&[String]) -> Option<String>) -> Mux {
    let dir = match dir {
        Ok(dir) => dir,
        Err(why) => {
            // Windows has no support: that is no warning, only the reason.
            if cfg!(unix) {
                tracing_warn(&format!("ssh connection sharing is off: {why}"));
            }
            return Mux::OFF;
        }
    };
    // First without our options: a ControlMaster or ControlPath of the user wins.
    match resolve(&[]) {
        Some(g) if !resolved(&g).user_controls => {}
        _ => return Mux::OFF,
    }
    let options = options(&dir);
    let socket = resolve(&options).and_then(|g| resolved(&g).socket);
    Mux { options, socket }
}

/// The reuse for `target` with the real ssh at `program`, and a stale socket left by a dead master
/// removed (`ssh -O check` fails while the file is there).
pub fn for_target(program: &Path, target: &str) -> Mux {
    let run = |extra: &[&str], opts: &[String]| {
        ssh(program)
            .args(extra)
            .args(opts)
            .args(["--", target])
            .output()
            .ok()
    };
    let mux = plan(socket_dir(), |opts| {
        run(&["-G"], opts)
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    });
    if let Some(socket) = &mux.socket
        && socket.exists()
        && run(&["-o", "BatchMode=yes", "-O", "check"], &mux.options)
            .is_some_and(|o| !o.status.success())
    {
        remove_stale(socket);
    }
    mux
}

fn ssh(program: &Path) -> std::process::Command {
    let mut cmd = poimen_core::gui_path::command(program);
    cmd.env("PATH", poimen_core::gui_path::child_path());
    cmd
}

fn tracing_warn(message: &str) {
    eprintln!("poimen: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(mode: u32, uid: u32) -> DirInfo {
        DirInfo {
            is_dir: true,
            is_symlink: false,
            uid,
            mode,
        }
    }

    #[test]
    fn a_private_directory_of_ours_passes() {
        assert!(check_dir(&info(0o40700, 1000), 1000).is_ok());
    }

    #[test]
    fn another_owner_a_wider_mode_or_a_symlink_fail() {
        assert!(
            check_dir(&info(0o40700, 0), 1000)
                .unwrap_err()
                .contains("owned")
        );
        assert!(
            check_dir(&info(0o40750, 1000), 1000)
                .unwrap_err()
                .contains("750")
        );
        assert!(check_dir(&info(0o40705, 1000), 1000).is_err());
        let link = DirInfo {
            is_symlink: true,
            ..info(0o40700, 1000)
        };
        assert!(check_dir(&link, 1000).is_err());
    }

    #[test]
    fn the_options_share_one_master_with_a_short_hashed_path() {
        let o = options(Path::new("/tmp/poimen-ssh-1"));
        assert!(o.contains(&"ControlMaster=auto".to_owned()));
        assert!(o.contains(&"ControlPersist=600".to_owned()));
        assert!(o.contains(&"ControlPath=/tmp/poimen-ssh-1/%C".to_owned()));
    }

    #[test]
    fn ssh_g_defaults_are_not_the_users_setting() {
        let g = "host box\ncontrolmaster false\ncontrolpath none\n";
        assert!(!resolved(g).user_controls);
        assert!(resolved("controlmaster auto\ncontrolpath none\n").user_controls);
        let own = resolved("controlmaster false\ncontrolpath /home/u/.ssh/cm-%r@%h\n");
        assert!(own.user_controls);
        assert_eq!(own.socket, Some("/home/u/.ssh/cm-%r@%h".into()));
    }

    #[test]
    fn a_users_control_setting_wins_and_adds_nothing() {
        let mux = plan(Ok("/tmp/d".into()), |_| {
            Some("controlmaster auto\ncontrolpath none\n".into())
        });
        assert!(mux.options.is_empty() && mux.socket.is_none());
    }

    #[test]
    fn without_a_user_setting_the_options_and_the_expanded_socket_are_used() {
        let mux = plan(Ok("/tmp/d".into()), |args| {
            Some(if args.is_empty() {
                "controlmaster false\ncontrolpath none\n".into()
            } else {
                "controlmaster auto\ncontrolpath /tmp/d/abc123\n".to_owned()
            })
        });
        assert!(mux.options.contains(&"ControlMaster=auto".to_owned()));
        assert_eq!(mux.socket, Some("/tmp/d/abc123".into()));
    }

    #[test]
    fn a_bad_directory_or_an_unrunnable_ssh_disables_the_reuse() {
        assert!(plan(Err("bad".into()), |_| None).options.is_empty());
        assert!(plan(Ok("/tmp/d".into()), |_| None).options.is_empty());
    }

    #[test]
    fn stale_socket_output_is_recognized() {
        assert!(is_stale(
            "Control socket connect(/tmp/x): Connection refused"
        ));
        assert!(is_stale(
            "mux_client_request_session: read from master failed"
        ));
        assert!(!is_stale("Permission denied (publickey)."));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_is_created_private_and_a_wide_one_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("poimen-mux-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let uid = current_uid().unwrap();
        assert_eq!(ensure_dir(&base, uid).unwrap(), base);
        assert_eq!(
            std::fs::metadata(&base).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ensure_dir(&base, uid).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn only_a_socket_file_is_removed_as_stale() {
        let base = std::env::temp_dir().join(format!("poimen-mux-rm-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("plain");
        std::fs::write(&file, b"x").unwrap();
        assert!(!remove_stale(&file));
        assert!(file.exists());
        let sock = base.join("s");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        assert!(remove_stale(&sock));
        assert!(!sock.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
