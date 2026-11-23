//! Starting `poimen-server` in a transient systemd scope of its own (#1231).
//!
//! GNOME (and other desktops) start the app in `app-<desktop>-<name>-<pid>.scope`; on logout
//! systemd stops that scope and kills every process in its cgroup. A server started as a plain
//! child, and the `poimen-pty-host` of every terminal it spawns, would be in that cgroup, so a
//! logout ended all terminals although ADR 0009 keeps them across a server stop. `process_group`
//! gives no new cgroup. `systemd-run --user --scope` moves the server (and the hosts, which
//! inherit its cgroup) into a scope that is not a child of the app's. Without systemd, or when
//! `systemd-run` cannot start the scope, the server is started as before.

use std::ffi::OsString;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::spawn_breaking_away;
use crate::gui_path;

/// A `systemd-run` that exits with an error within this time did not start the scope.
const SCOPE_FAILS_WITHIN: Duration = Duration::from_secs(10);

/// Where systemd announces that it runs as the init system (`sd_booted(3)`).
const BOOTED_MARKER: &str = "/run/systemd/system";

/// How the server was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// In its own transient scope.
    Scoped,
    /// As a plain child: no systemd, no `systemd-run`, or it failed.
    Plain,
}

/// Whether this system runs systemd (always `false` off Linux).
pub fn booted() -> bool {
    cfg!(target_os = "linux") && Path::new(BOOTED_MARKER).is_dir()
}

/// The arguments of `systemd-run` that start `program` in a user scope named `unit`:
/// `--collect` removes the unit even when it failed, `--quiet` keeps its messages out of the
/// way. The scope runs `program` in the foreground, with the environment and directory of the
/// caller.
pub fn scope_args(program: &Path, unit: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--user",
        "--scope",
        "--collect",
        "--quiet",
        "--description=poimen-server",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push(format!("--unit={unit}").into());
    args.push("--".into());
    args.push(program.as_os_str().to_owned());
    args
}

/// The unit name for a server of `program` started by the app with process id `pid`
/// (`poimen-server-1234`, `poimen-server-rc-1234`): unique per app process.
pub fn unit_name(program: &Path, pid: u32) -> String {
    let stem = program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "poimen-server".into());
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{safe}-{pid}")
}

/// Starts the server with `start(executable, arguments)`: through `systemd_run` in a scope when
/// `booted` and it was found, else (or when that cannot be started) `program` itself.
pub fn launch<T>(
    booted: bool,
    systemd_run: Option<&Path>,
    program: &Path,
    unit: &str,
    mut start: impl FnMut(&Path, &[OsString]) -> io::Result<T>,
) -> io::Result<(T, Start)> {
    if booted && let Some(run) = systemd_run {
        let args = scope_args(program, unit);
        if let Ok(child) = start(run, &args) {
            return Ok((child, Start::Scoped));
        }
    }
    start(program, &[]).map(|child| (child, Start::Plain))
}

/// `systemd-run` on `path`, when the system runs systemd.
pub fn find(path: &str) -> Option<PathBuf> {
    if !booted() {
        return None;
    }
    gui_path::resolve_program("systemd-run", path)
}

fn command(flags: u32, exe: &Path, args: &[OsString]) -> Command {
    let mut command = gui_path::command(exe);
    command.args(args);
    if !cfg!(windows) {
        command.env("PATH", gui_path::child_path());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        let _ = flags;
        command.process_group(0);
    }
    #[cfg(windows)]
    command.creation_flags(flags);
    command
}

/// Starts `program` detached: own process group, no terminal, no pipes, never waited on beyond
/// reaping: the server outlives poimen. On Linux with systemd it runs in a scope of its own
/// (see the module).
pub fn spawn_detached(program: &Path) -> io::Result<()> {
    let systemd_run = find(&gui_path::child_path());
    let unit = unit_name(program, std::process::id());
    let (mut child, start) = launch(
        booted(),
        systemd_run.as_deref(),
        program,
        &unit,
        |exe, args| spawn_breaking_away(|flags| command(flags, exe, args).spawn()),
    )?;
    let program = program.to_path_buf();
    let started = Instant::now();
    // Reap it if it exits (a second server exits at once); it is never killed. A scope that
    // `systemd-run` could not create (no user manager) ends at once with an error: start the
    // server plain then.
    std::thread::spawn(move || {
        let failed = child.wait().is_ok_and(|s| !s.success());
        if start == Start::Scoped
            && failed
            && started.elapsed() < SCOPE_FAILS_WITHIN
            && let Ok(mut plain) =
                spawn_breaking_away(|flags| command(flags, &program, &[]).spawn())
        {
            let _ = plain.wait();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    type Call = (PathBuf, Vec<OsString>);

    fn run(
        booted: bool,
        systemd_run: Option<&str>,
        fail_scoped: bool,
    ) -> (io::Result<((), Start)>, Vec<Call>) {
        let calls = RefCell::new(Vec::<Call>::new());
        let run = systemd_run.map(PathBuf::from);
        let result = launch(
            booted,
            run.as_deref(),
            Path::new("/opt/poimen-server"),
            "poimen-server-7",
            |exe, args| {
                calls.borrow_mut().push((exe.to_path_buf(), args.to_vec()));
                if fail_scoped && exe.ends_with("systemd-run") {
                    Err(io::Error::from(io::ErrorKind::NotFound))
                } else {
                    Ok(())
                }
            },
        );
        (result, calls.into_inner())
    }

    #[test]
    fn the_scope_command_is_built_as_expected() {
        let (result, calls) = run(true, Some("/usr/bin/systemd-run"), false);
        assert_eq!(result.unwrap().1, Start::Scoped);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Path::new("/usr/bin/systemd-run"));
        let args: Vec<String> = calls[0]
            .1
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--user",
                "--scope",
                "--collect",
                "--quiet",
                "--description=poimen-server",
                "--unit=poimen-server-7",
                "--",
                "/opt/poimen-server"
            ]
        );
    }

    #[test]
    fn without_systemd_run_the_server_starts_plain() {
        let (result, calls) = run(true, None, false);
        assert_eq!(result.unwrap().1, Start::Plain);
        assert_eq!(calls, [(PathBuf::from("/opt/poimen-server"), vec![])]);
    }

    #[test]
    fn without_systemd_the_server_starts_plain_even_with_systemd_run() {
        let (result, calls) = run(false, Some("/usr/bin/systemd-run"), false);
        assert_eq!(result.unwrap().1, Start::Plain);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Path::new("/opt/poimen-server"));
    }

    #[test]
    fn a_systemd_run_that_cannot_start_falls_back_to_the_plain_start() {
        let (result, calls) = run(true, Some("/usr/bin/systemd-run"), true);
        assert_eq!(result.unwrap().1, Start::Plain);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], (PathBuf::from("/opt/poimen-server"), vec![]));
    }

    #[test]
    fn the_unit_name_is_safe_and_per_channel() {
        assert_eq!(
            unit_name(Path::new("/x/poimen-server"), 12),
            "poimen-server-12"
        );
        assert_eq!(
            unit_name(Path::new("/x/poimen-server-rc.exe"), 3),
            "poimen-server-rc-3"
        );
        assert_eq!(unit_name(Path::new("/x/a b_c"), 1), "a-b-c-1");
    }
}
