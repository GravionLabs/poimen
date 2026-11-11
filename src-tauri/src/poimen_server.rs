//! Starting `poimen-server` (ADR 0003) when it is absent. poimen stops it only on a confirmed
//! restart (an older server than the bundled one, [`restart`]: the terminals keep running in
//! their hosts and the new server adopts them, ADR 0009) and a confirmed "Quit poimen and stop
//! server" ([`stop`]: ends every terminal); otherwise the server and the PTY hosts of its
//! terminals outlive the app. The wire protocol lives in `crates/poimen-protocol`, the client
//! in `crates/poimen-client`; this module only decides whether to start a server and does so
//! detached, like `backend.rs` does for herdr. On Windows it starts a per-version copy under
//! `%LOCALAPPDATA%\poimen-server\bin` (see [`stage`]), breaking away from the app's job; inside
//! a Linux AppImage one under `$XDG_DATA_HOME/poimen-server/bin` (see [`stage_root`]).

use std::ffi::OsString;
use std::fmt;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use poimen_client::protocol::{CHANNEL, Channel};
use poimen_client::{Client, ClientError};

use crate::backend::{Outcome, POLL_INTERVAL, START_TIMEOUT};
use crate::gui_path;

/// This build's server program: `poimen-server`, on rc `poimen-server-rc` (the sidecar of the rc
/// bundle, `tauri.rc.conf.json`), so a stable and an rc installation never start each other's.
fn program() -> String {
    CHANNEL.named("poimen-server")
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// No `poimen-server` next to the app or on the PATH poimen uses.
    NotFound,
    Spawn(String),
    Timeout(Duration),
    /// A server runs but refused the connection (Windows: started elevated, see [`Ping`]).
    Denied(String),
    /// The running server did not stop after `server.shutdown`.
    Shutdown(String),
}

/// What a `ping` of the socket found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ping {
    /// A server answered with the protocol poimen speaks.
    Answers,
    /// A server with the protocol poimen speaks, but older than the bundled one. Never replaced
    /// without the user's consent: a server from before #604 ends every terminal when it stops
    /// (the terminals of a later one survive the restart, `Terminal.survives_restart`).
    Outdated { running: String },
    /// A server with another protocol version: it cannot be talked to, and a second one cannot
    /// start on its socket.
    Incompatible { running: String, protocol: u32 },
    /// No server, or one that does not answer (in time or with that protocol).
    Absent,
    /// The connect was denied: on Windows the pipe admits its owner only, so a server started
    /// elevated (owned by Administrators) refuses a normal poimen. Starting another cannot help.
    Denied(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::NotFound => {
                write!(f, "`poimen-server` was not found next to poimen or on PATH")
            }
            StartError::Spawn(e) => write!(f, "could not start `poimen-server`: {e}"),
            StartError::Timeout(t) => {
                write!(f, "`poimen-server` did not answer within {} s", t.as_secs())
            }
            StartError::Denied(e) | StartError::Shutdown(e) => write!(f, "{e}"),
        }
    }
}

/// The side effects of `ensure_running`; faked in tests.
pub trait Runner {
    /// Whether a server answers `ping` with the protocol poimen speaks.
    fn ping(&self) -> Ping;
    fn resolve(&self) -> Option<PathBuf>;
    /// Starts the program detached and returns without waiting for it.
    fn spawn(&self, program: &Path) -> io::Result<()>;
    /// Asks the running server to exit (`server.shutdown`): with `keep_terminals` it leaves them
    /// running in their hosts for the next server, else it ends every terminal.
    fn shutdown(&self, keep_terminals: bool) -> Result<(), String>;
    fn sleep(&self, duration: Duration);
    fn now(&self) -> Duration;
}

/// Returns at once when a server answers; otherwise starts one (when `auto_start`) and waits until
/// it answers. Never stops or restarts a running server: an outdated or incompatible one is only
/// reported ([`Outcome::Outdated`], [`Outcome::Incompatible`]); [`restart`] replaces it.
pub fn ensure_running(runner: &impl Runner, auto_start: bool) -> Result<Outcome, StartError> {
    match runner.ping() {
        Ping::Answers => return Ok(Outcome::AlreadyRunning),
        Ping::Outdated { running } => {
            return Ok(Outcome::Outdated {
                running,
                bundled: BUNDLED_VERSION.to_string(),
            });
        }
        Ping::Incompatible { running, protocol } => {
            return Ok(Outcome::Incompatible { running, protocol });
        }
        Ping::Denied(e) => return Err(StartError::Denied(e)),
        Ping::Absent => {}
    }
    if !auto_start {
        return Ok(Outcome::Disabled);
    }
    let program = runner.resolve().ok_or(StartError::NotFound)?;
    runner
        .spawn(&program)
        .map_err(|e| StartError::Spawn(e.to_string()))?;
    let start = runner.now();
    while runner.now().saturating_sub(start) < START_TIMEOUT {
        runner.sleep(POLL_INTERVAL);
        match runner.ping() {
            Ping::Answers => return Ok(Outcome::Started),
            Ping::Denied(e) => return Err(StartError::Denied(e)),
            // Another poimen raced us with an older server: report it as above.
            Ping::Outdated { .. } | Ping::Incompatible { .. } | Ping::Absent => {}
        }
    }
    Err(StartError::Timeout(START_TIMEOUT))
}

/// The version poimen bundles the server at (the app and the server share the workspace version).
pub const BUNDLED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Asks the running server to shut down and waits until nothing answers any more. Ends every
/// terminal: call it only after the user confirmed ("Quit poimen and stop server").
pub fn stop(runner: &impl Runner) -> Result<(), StartError> {
    shut_down(runner, false)
}

/// [`stop`], keeping the terminals in their hosts when `keep_terminals` (a server from before
/// #604 ends them anyway).
fn shut_down(runner: &impl Runner, keep_terminals: bool) -> Result<(), StartError> {
    match runner.ping() {
        Ping::Absent => return Ok(()),
        Ping::Denied(e) => return Err(StartError::Denied(e)),
        _ => {}
    }
    runner.shutdown(keep_terminals).map_err(|e| {
        StartError::Shutdown(format!(
            "could not stop the running `poimen-server`: {e}. A server older than this poimen may not know `server.shutdown`: end its process by hand"
        ))
    })?;
    let start = runner.now();
    while runner.now().saturating_sub(start) < START_TIMEOUT {
        if runner.ping() == Ping::Absent {
            return Ok(());
        }
        runner.sleep(POLL_INTERVAL);
    }
    Err(StartError::Shutdown(format!(
        "`poimen-server` did not stop within {} s",
        START_TIMEOUT.as_secs()
    )))
}

/// The confirmed restart: stop the running server, leaving the terminals in their hosts, then
/// start the bundled one, which adopts them (ADR 0009).
pub fn restart(runner: &impl Runner) -> Result<Outcome, StartError> {
    shut_down(runner, true)?;
    ensure_running(runner, true)
}

/// The file name of the bundled server of `channel` next to the app executable: Tauri installs
/// the sidecar as `poimen-server.exe` on Windows (not the first `PATHEXT` candidate, `.COM`),
/// `poimen-server-rc(.exe)` on rc.
pub fn beside_name(windows: bool, channel: Channel) -> String {
    channel.server_program(windows)
}

/// `CREATE_NEW_PROCESS_GROUP`: the server gets no Ctrl+C of the app's console group.
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// `CREATE_NO_WINDOW` (as `gui_path::CREATE_NO_WINDOW`, which exists on Windows only).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `CREATE_BREAKAWAY_FROM_JOB`: the server leaves a job object of the app (a terminal or IDE that
/// started `pnpm tauri dev`), so closing that job does not kill it.
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// The Windows creation flags of the detached server (ignored on Unix).
pub fn creation_flags(breakaway: bool) -> u32 {
    let flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    if breakaway {
        flags | CREATE_BREAKAWAY_FROM_JOB
    } else {
        flags
    }
}

/// Spawns with [`creation_flags`]`(true)` first; a job that forbids breaking away refuses that
/// with `PermissionDenied`, then it is started without (inside the job). Unix: one attempt.
pub fn spawn_breaking_away<T>(mut spawn: impl FnMut(u32) -> io::Result<T>) -> io::Result<T> {
    if !cfg!(windows) {
        return spawn(0);
    }
    match spawn(creation_flags(true)) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => spawn(creation_flags(false)),
        other => other,
    }
}

/// Where the app keeps per-version copies of the server it starts (see [`stage`]), read from
/// `env` on `os` (`std::env::consts::OS`). Windows: `%LOCALAPPDATA%\poimen-server\bin`. Linux
/// only inside an AppImage (`APPIMAGE` or `APPDIR` set), whose mount ends with the app and moves
/// on every run: `$XDG_DATA_HOME/poimen-server/bin`, else `$HOME/.local/share/poimen-server/bin`.
/// On rc the directories are `poimen-server-rc`. Elsewhere (other Linux packages, macOS) `None`:
/// the installed binary is started as it is.
pub fn stage_root(
    env: impl Fn(&str) -> Option<OsString>,
    os: &str,
    channel: Channel,
) -> Option<PathBuf> {
    let set = |name: &str| env(name).filter(|v| !v.is_empty());
    let absolute = |name: &str| set(name).map(PathBuf::from).filter(|p| p.is_absolute());
    match os {
        "windows" => poimen_client::protocol::windows_dir_from(env("LOCALAPPDATA"), channel)
            .map(|dir| dir.join("bin")),
        "linux" if set("APPIMAGE").is_some() || set("APPDIR").is_some() => {
            let data = absolute("XDG_DATA_HOME")
                .or_else(|| absolute("HOME").map(|home| home.join(".local").join("share")))?;
            Some(data.join(channel.named("poimen-server")).join("bin"))
        }
        _ => None,
    }
}

/// Creates `dir` and its missing parents; on Unix those are private (0700), like the socket
/// directory.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Copies the bundled server to `<bin_root>/<version>/<its file name>` and returns that path,
/// together with the `poimen-pty-host` next to it (ADR 0009: the server starts its terminals in
/// hosts from its own directory), when there is one.
/// Windows: the running copy then locks only itself, so an update can replace the installed one.
/// AppImage: the copy outlives the mount it came from. The copy is executable (0755 on Unix). An
/// up-to-date copy (same size and modification time) is reused; the directories of other versions
/// are then removed by [`prune`] unless a server or a host still runs from them.
pub fn stage(bundled: &Path, bin_root: &Path, version: &str) -> io::Result<PathBuf> {
    let dir = bin_root.join(version);
    let name = bundled.file_name().map_or_else(
        || beside_name(cfg!(windows), CHANNEL).into(),
        OsString::from,
    );
    let target = copy_into(bundled, &dir, &name)?;
    let host = OsString::from(CHANNEL.host_program(cfg!(windows)));
    let bundled_host = bundled.with_file_name(&host);
    if bundled_host.is_file() {
        copy_into(&bundled_host, &dir, &host)?;
    }
    prune(bin_root, version, &running_exes());
    Ok(target)
}

/// Copies `source` to `<dir>/<name>` unless an up-to-date copy is there; returns its path.
fn copy_into(source_path: &Path, dir: &Path, name: &OsString) -> io::Result<PathBuf> {
    let source = std::fs::metadata(source_path)?;
    let target = dir.join(name);
    let same = |m: &std::fs::Metadata| {
        m.len() == source.len() && m.modified().ok() == source.modified().ok()
    };
    if !std::fs::metadata(&target).is_ok_and(|m| same(&m)) {
        create_private_dir(dir)?;
        let mut tmp_name = name.clone();
        tmp_name.push(format!(".{}.tmp", std::process::id()));
        let tmp = dir.join(tmp_name);
        std::fs::copy(source_path, &tmp)?;
        // The copy takes the source's mode (0555 in a read-only mount): make it writable for
        // `set_modified` and executable for everyone, as an installed binary.
        #[cfg(unix)]
        let writable = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        };
        #[cfg(not(unix))]
        let writable = Ok(());
        let copied = writable
            .and_then(|()| std::fs::File::options().write(true).open(&tmp))
            .and_then(|f| source.modified().and_then(|t| f.set_modified(t)))
            .and_then(|()| std::fs::rename(&tmp, &target).or_else(|e| reuse_staged(e, &target)));
        let _ = std::fs::remove_file(&tmp);
        copied?;
    }
    Ok(target)
}

/// The executables of the running processes, or `None` where that cannot be read (Windows:
/// there a running exe cannot be moved, which [`prune`] uses instead). Linux reads `/proc/*/exe`;
/// other Unix systems `ps` (full path in `comm`).
fn running_exes() -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        let mut exes = Vec::new();
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            if let Ok(exe) = std::fs::read_link(entry.path().join("exe")) {
                exes.push(exe);
            }
        }
        Some(exes)
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let out = gui_path::command("ps")
            .args(["-axo", "comm="])
            .output()
            .ok()?;
        out.status.success().then(|| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| PathBuf::from(l.trim()))
                .collect()
        })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Removes the directories of other versions under `bin_root` when no process runs from them:
/// neither a server nor a `poimen-pty-host` (ADR 0009: a host outlives its server, so it can
/// run from an old version's directory for as long as its terminal lives). `running` lists the
/// executables of the running processes; `None` means unknown, then a directory is probed by
/// renaming it, which Windows refuses while an executable in it runs. A directory in use is left
/// whole and tried again at the next stage.
pub fn prune(bin_root: &Path, keep: &str, running: &Option<Vec<PathBuf>>) {
    let Ok(entries) = std::fs::read_dir(bin_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == keep || !path.is_dir() {
            continue;
        }
        let in_use = match running {
            Some(exes) => exes.iter().any(|exe| exe.starts_with(&path)),
            None => {
                let probe =
                    bin_root.join(format!(".{}.removing", entry.file_name().to_string_lossy()));
                match std::fs::rename(&path, &probe) {
                    Ok(()) => {
                        let _ = std::fs::remove_dir_all(&probe);
                        false
                    }
                    Err(_) => true,
                }
            }
        };
        if !in_use {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// `<bin_root>/poimen-server`, a symlink to the staged copy (relative, so the directory can
/// move): the path the hooks call, which stays valid when a later version is staged (Unix).
#[cfg(unix)]
pub fn stable_link(bin_root: &Path, staged: &Path) -> io::Result<PathBuf> {
    let link = bin_root.join(program());
    let target = staged.strip_prefix(bin_root).unwrap_or(staged);
    if std::fs::read_link(&link).is_ok_and(|current| current == target) {
        return Ok(link);
    }
    let tmp = bin_root.join(format!(".{}.{}.link", program(), std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)?;
    std::fs::rename(&tmp, &link).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(link)
}

/// A staged server: the copy to run and, on Unix, the stable path the hooks call.
#[derive(Debug, PartialEq, Eq)]
pub struct Staged {
    pub program: PathBuf,
    pub hook_exe: Option<PathBuf>,
}

/// Stages `program` under `bin_root` ([`stage`]) and, on Unix, points the [`stable_link`] at it
/// (the versioned copy itself when the link cannot be made). `None` when the copy fails.
pub fn prepare(program: &Path, bin_root: &Path, version: &str) -> Option<Staged> {
    let staged = stage(program, bin_root, version).ok()?;
    #[cfg(unix)]
    let hook_exe = Some(stable_link(bin_root, &staged).unwrap_or_else(|_| staged.clone()));
    #[cfg(not(unix))]
    let hook_exe = None;
    Some(Staged {
        program: staged,
        hook_exe,
    })
}

/// [`prepare`] under the [`stage_root`] of this process, if it has one.
pub fn prepare_here(program: &Path) -> Option<Staged> {
    stage_root(|name| std::env::var_os(name), std::env::consts::OS, CHANNEL)
        .and_then(|root| prepare(program, &root, env!("CARGO_PKG_VERSION")))
}

/// A failed rename onto an existing staged copy (Windows: another poimen staged it and its server
/// runs from it, so it is locked) keeps that copy; any other failure stays an error.
fn reuse_staged(error: io::Error, target: &Path) -> io::Result<()> {
    if target.is_file() { Ok(()) } else { Err(error) }
}

/// Whether `running` is an older version than `bundled`: `x.y.z` compared numerically, a
/// pre-release suffix ignored. A version that does not parse is never called outdated.
pub fn is_older(running: &str, bundled: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        let core = v.split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    }
    match (parse(running), parse(bundled)) {
        (Some(r), Some(b)) => r < b,
        _ => false,
    }
}

/// What a ping result means. Only on Windows is a denied connect [`Ping::Denied`] (the pipe of an
/// elevated server); on Unix the client's directory check denies a loose or foreign socket
/// directory, and starting a server heals that (it makes its own directory private again).
fn classify(
    result: Result<poimen_client::protocol::PingResult, ClientError>,
    windows: bool,
    bundled: &str,
) -> Ping {
    match result {
        Ok(p) if p.protocol == poimen_client::protocol::PROTOCOL_VERSION => {
            if is_older(&p.version, bundled) {
                Ping::Outdated { running: p.version }
            } else {
                Ping::Answers
            }
        }
        Ok(p) => Ping::Incompatible {
            running: p.version,
            protocol: p.protocol,
        },
        Err(ref e @ ClientError::Connect(ref io))
            if windows && io.kind() == io::ErrorKind::PermissionDenied =>
        {
            Ping::Denied(e.to_string())
        }
        _ => Ping::Absent,
    }
}

/// The real runner: the socket of the environment, the server binary next to the app executable
/// or on the login-shell PATH.
pub struct SystemRunner {
    client: Client,
    origin: Instant,
}

impl SystemRunner {
    pub fn new() -> Self {
        Self {
            client: Client::default(),
            origin: Instant::now(),
        }
    }
}

impl Default for SystemRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for SystemRunner {
    fn ping(&self) -> Ping {
        classify(self.client.ping(), cfg!(windows), BUNDLED_VERSION)
    }

    fn resolve(&self) -> Option<PathBuf> {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join(beside_name(cfg!(windows), CHANNEL)))
            })
            .filter(|p| p.is_file());
        beside.or_else(|| gui_path::resolve_program(&program(), &gui_path::child_path()))
    }

    fn spawn(&self, program: &Path) -> io::Result<()> {
        // Own process group, no terminal, no pipes, never waited on: the server outlives poimen.
        let staged = prepare_here(program);
        let program = staged.as_ref().map_or(program, |s| s.program.as_path());
        let command = |flags: u32| {
            let mut command = gui_path::command(program);
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
        };
        let mut child = spawn_breaking_away(|flags| command(flags).spawn())?;
        // Reap it if it exits (a second server exits at once); it is never killed.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn shutdown(&self, keep_terminals: bool) -> Result<(), String> {
        let result = if keep_terminals {
            self.client.shutdown_keeping_terminals()
        } else {
            self.client.shutdown()
        };
        result.map_err(|e| e.to_string())
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Answers after `up_after` calls following the spawn; a clock that advances on `sleep`.
    struct Fake {
        denied: bool,
        running: Cell<bool>,
        /// What a running server's ping says; `None`: a current server (`Answers`).
        found: RefCell<Option<Ping>>,
        shutdowns: Cell<u32>,
        /// `keep_terminals` of every shutdown asked for.
        kept: RefCell<Vec<bool>>,
        shutdown_fails: bool,
        installed: bool,
        becomes_up: bool,
        spawned: RefCell<Vec<PathBuf>>,
        clock: Cell<Duration>,
    }

    impl Fake {
        fn new(running: bool, installed: bool, becomes_up: bool) -> Self {
            Self {
                denied: false,
                running: Cell::new(running),
                found: RefCell::new(None),
                shutdowns: Cell::new(0),
                kept: RefCell::new(vec![]),
                shutdown_fails: false,
                installed,
                becomes_up,
                spawned: RefCell::new(vec![]),
                clock: Cell::new(Duration::ZERO),
            }
        }
    }

    impl Runner for Fake {
        fn ping(&self) -> Ping {
            if self.denied {
                Ping::Denied("denied".into())
            } else if self.running.get() {
                self.found.borrow().clone().unwrap_or(Ping::Answers)
            } else {
                Ping::Absent
            }
        }
        fn resolve(&self) -> Option<PathBuf> {
            self.installed.then(|| PathBuf::from("/bin/poimen-server"))
        }
        fn spawn(&self, program: &Path) -> io::Result<()> {
            self.spawned.borrow_mut().push(program.to_path_buf());
            self.running.set(self.becomes_up);
            self.found.replace(None);
            Ok(())
        }
        fn shutdown(&self, keep_terminals: bool) -> Result<(), String> {
            self.shutdowns.set(self.shutdowns.get() + 1);
            self.kept.borrow_mut().push(keep_terminals);
            if self.shutdown_fails {
                return Err("unknown method server.shutdown".into());
            }
            self.running.set(false);
            self.found.replace(None);
            Ok(())
        }
        fn sleep(&self, d: Duration) {
            self.clock.set(self.clock.get() + d);
        }
        fn now(&self) -> Duration {
            self.clock.get()
        }
    }

    #[test]
    fn the_bundled_server_has_the_exe_suffix_on_windows_only() {
        assert_eq!(beside_name(true, Channel::Stable), "poimen-server.exe");
        assert_eq!(beside_name(false, Channel::Stable), "poimen-server");
        assert_eq!(beside_name(true, Channel::Rc), "poimen-server-rc.exe");
        assert_eq!(beside_name(false, Channel::Rc), "poimen-server-rc");
    }

    #[test]
    fn creation_flags_add_breakaway_on_request() {
        assert_eq!(creation_flags(false), 0x0800_0200);
        assert_eq!(creation_flags(true), 0x0900_0200);
    }

    #[test]
    fn a_job_that_forbids_breakaway_gets_a_second_spawn_without_it() {
        let tried = RefCell::new(vec![]);
        let result = spawn_breaking_away(|flags| {
            tried.borrow_mut().push(flags);
            if flags & CREATE_BREAKAWAY_FROM_JOB != 0 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                Ok(flags)
            }
        });
        if cfg!(windows) {
            assert_eq!(result.unwrap(), 0x0800_0200);
            assert_eq!(*tried.borrow(), [0x0900_0200, 0x0800_0200]);
        } else {
            assert_eq!(result.unwrap(), 0);
            assert_eq!(*tried.borrow(), [0]);
        }
    }

    #[test]
    fn other_spawn_errors_are_not_retried() {
        let tries = Cell::new(0);
        let result: io::Result<()> = spawn_breaking_away(|_| {
            tries.set(tries.get() + 1);
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(tries.get(), 1);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-stage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_server_is_copied_per_version_and_old_copies_are_removed() {
        let dir = temp_dir("copy");
        let bundled = dir.join("poimen-server.exe");
        std::fs::write(&bundled, "v2").unwrap();
        let bin = dir.join("bin");
        std::fs::create_dir_all(bin.join("0.7.0")).unwrap();
        std::fs::write(bin.join("0.7.0").join("poimen-server.exe"), "v1").unwrap();

        let staged = stage(&bundled, &bin, "0.8.0").unwrap();
        assert_eq!(staged, bin.join("0.8.0").join("poimen-server.exe"));
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), "v2");
        assert!(!bin.join("0.7.0").exists());
        let names: Vec<_> = std::fs::read_dir(bin.join("0.8.0"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(names.len(), 1, "no temp file is left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_pty_host_next_to_the_server_is_staged_with_it() {
        let dir = temp_dir("host");
        let bundled = dir.join(beside_name(cfg!(windows), CHANNEL));
        std::fs::write(&bundled, "server").unwrap();
        let host = CHANNEL.host_program(cfg!(windows));
        std::fs::write(dir.join(&host), "host").unwrap();
        let bin = dir.join("bin");
        let staged = stage(&bundled, &bin, "0.9.0").unwrap();
        let staged_host = staged.with_file_name(&host);
        assert_eq!(std::fs::read_to_string(&staged_host).unwrap(), "host");
        // Without a host next to the server only the server is staged.
        std::fs::remove_file(dir.join(&host)).unwrap();
        let staged = stage(&bundled, &bin, "1.0.0").unwrap();
        assert!(!staged.with_file_name(&host).exists());
        assert!(!staged_host.exists(), "the old version went");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_up_to_date_copy_is_reused_and_a_changed_bundle_recopied() {
        let dir = temp_dir("reuse");
        let bundled = dir.join("poimen-server.exe");
        std::fs::write(&bundled, "build-1").unwrap();
        let bin = dir.join("bin");
        let staged = stage(&bundled, &bin, "0.8.0").unwrap();
        // A copy that is up to date is not written again (it may be running and locked).
        let marker = std::fs::metadata(&staged).unwrap().modified().unwrap();
        assert_eq!(stage(&bundled, &bin, "0.8.0").unwrap(), staged);
        assert_eq!(
            std::fs::metadata(&staged).unwrap().modified().unwrap(),
            marker
        );

        std::fs::write(&bundled, "build-two").unwrap();
        stage(&bundled, &bin, "0.8.0").unwrap();
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), "build-two");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            vars.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn an_appimage_stages_under_the_data_dir() {
        let appimage = [
            ("APPIMAGE", "/home/jo/poimen.AppImage"),
            ("APPDIR", "/tmp/.mount_poimenAb12"),
            ("HOME", "/home/jo"),
        ];
        assert_eq!(
            stage_root(env_of(&appimage), "linux", Channel::Stable),
            Some(PathBuf::from("/home/jo/.local/share/poimen-server/bin"))
        );
        let with_xdg = [
            ("APPDIR", "/tmp/.mount_poimenAb12"),
            ("XDG_DATA_HOME", "/data"),
            ("HOME", "/home/jo"),
        ];
        assert_eq!(
            stage_root(env_of(&with_xdg), "linux", Channel::Stable),
            Some(PathBuf::from("/data/poimen-server/bin"))
        );
        // A relative XDG_DATA_HOME is ignored (XDG base directory spec).
        let relative = [
            ("APPIMAGE", "/a"),
            ("XDG_DATA_HOME", "data"),
            ("HOME", "/h"),
        ];
        assert_eq!(
            stage_root(env_of(&relative), "linux", Channel::Stable),
            Some(PathBuf::from("/h/.local/share/poimen-server/bin"))
        );
        assert_eq!(
            stage_root(env_of(&[("APPIMAGE", "/a")]), "linux", Channel::Stable),
            None
        );
    }

    #[test]
    fn other_linux_packages_and_macos_do_not_stage() {
        let plain = [("HOME", "/home/jo"), ("XDG_DATA_HOME", "/data")];
        assert_eq!(stage_root(env_of(&plain), "linux", Channel::Stable), None);
        let empty = [("APPIMAGE", ""), ("APPDIR", ""), ("HOME", "/home/jo")];
        assert_eq!(stage_root(env_of(&empty), "linux", Channel::Stable), None);
        let appimage = [("APPIMAGE", "/a"), ("HOME", "/Users/jo")];
        assert_eq!(
            stage_root(env_of(&appimage), "macos", Channel::Stable),
            None
        );
    }

    #[test]
    fn windows_stages_under_local_app_data() {
        let env = [("LOCALAPPDATA", r"C:\Users\jo\AppData\Local")];
        assert_eq!(
            stage_root(env_of(&env), "windows", Channel::Stable),
            Some(
                PathBuf::from(r"C:\Users\jo\AppData\Local")
                    .join("poimen-server")
                    .join("bin")
            )
        );
        assert_eq!(stage_root(env_of(&[]), "windows", Channel::Stable), None);
    }

    #[test]
    fn the_rc_channel_stages_in_its_own_directories() {
        let windows = [("LOCALAPPDATA", r"C:\Users\jo\AppData\Local")];
        assert_eq!(
            stage_root(env_of(&windows), "windows", Channel::Rc),
            Some(
                PathBuf::from(r"C:\Users\jo\AppData\Local")
                    .join("poimen-server-rc")
                    .join("bin")
            )
        );
        let appimage = [("APPIMAGE", "/a"), ("HOME", "/home/jo")];
        assert_eq!(
            stage_root(env_of(&appimage), "linux", Channel::Rc),
            Some(PathBuf::from("/home/jo/.local/share/poimen-server-rc/bin"))
        );
        assert_eq!(stage_root(env_of(&appimage), "macos", Channel::Rc), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_unix_copy_is_executable_in_a_private_dir_behind_a_stable_link() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = temp_dir("unix");
        // As in a squashfs mount: readable and executable, not writable.
        let bundled = dir.join("poimen-server");
        std::fs::write(&bundled, "v1").unwrap();
        std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o555)).unwrap();
        let bin = dir.join("data").join("poimen-server").join("bin");

        let staged = prepare(&bundled, &bin, "0.9.0").unwrap();
        let copy = bin.join("0.9.0").join("poimen-server");
        let link = bin.join("poimen-server");
        assert_eq!(
            staged,
            Staged {
                program: copy.clone(),
                hook_exe: Some(link.clone())
            }
        );
        assert_eq!(mode(&copy), 0o755);
        assert_eq!(mode(&bin.join("0.9.0")), 0o700);
        assert_eq!(mode(&dir.join("data").join("poimen-server")), 0o700);
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            Path::new("0.9.0/poimen-server")
        );
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "v1");

        // The next version moves the link; the hooks keep their path.
        std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&bundled, "v2").unwrap();
        let next = prepare(&bundled, &bin, "0.10.0").unwrap();
        assert_eq!(next.hook_exe, Some(link.clone()));
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "v2");
        assert!(!bin.join("0.9.0").exists());
        let names: Vec<_> = std::fs::read_dir(&bin)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            names.len(),
            2,
            "the version and the link, no temp file: {names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn version_dir_with(bin: &Path, version: &str, files: &[&str]) -> PathBuf {
        let dir = bin.join(version);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    #[test]
    fn the_host_beside_the_server_is_staged_into_the_same_version_directory() {
        let dir = temp_dir("host-same-version");
        let host_name = CHANNEL.host_program(cfg!(windows));
        let server_name = beside_name(cfg!(windows), CHANNEL);
        let bundled = dir.join("app");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(&server_name), "server").unwrap();
        std::fs::write(bundled.join(&host_name), "host").unwrap();
        let bin = dir.join("bin");

        let staged = prepare(&bundled.join(&server_name), &bin, "1.0.0").unwrap();
        assert_eq!(staged.program, bin.join("1.0.0").join(&server_name));
        assert_eq!(
            std::fs::read_to_string(bin.join("1.0.0").join(&host_name)).unwrap(),
            "host"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bundle_without_a_host_stages_the_server_alone() {
        let dir = temp_dir("no-host");
        let server_name = beside_name(cfg!(windows), CHANNEL);
        std::fs::write(dir.join(&server_name), "server").unwrap();
        let bin = dir.join("bin");
        assert!(prepare(&dir.join(&server_name), &bin, "1.0.0").is_some());
        let names: Vec<_> = std::fs::read_dir(bin.join("1.0.0"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(names.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_with_a_running_host_or_server_is_kept_and_the_others_removed() {
        let dir = temp_dir("prune");
        let bin = dir.join("bin");
        let host_only = version_dir_with(&bin, "0.1.0", &["poimen-server", "poimen-pty-host"]);
        let server_only = version_dir_with(&bin, "0.2.0", &["poimen-server", "poimen-pty-host"]);
        let idle = version_dir_with(&bin, "0.3.0", &["poimen-server", "poimen-pty-host"]);
        let current = version_dir_with(&bin, "0.4.0", &["poimen-server"]);
        let running = Some(vec![
            host_only.join("poimen-pty-host"),
            server_only.join("poimen-server"),
            PathBuf::from("/usr/bin/other"),
        ]);

        prune(&bin, "0.4.0", &running);
        assert!(host_only.exists(), "a running host keeps its directory");
        assert!(server_only.exists(), "a running server keeps its directory");
        assert!(!idle.exists());
        assert!(current.exists(), "the current version is never removed");

        // Once nothing runs from them, the next prune removes them.
        prune(&bin, "0.4.0", &Some(Vec::new()));
        assert!(!host_only.exists() && !server_only.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_leaves_files_and_a_missing_root_alone() {
        let dir = temp_dir("prune-files");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("poimen-server"), "link").unwrap();
        prune(&bin, "1.0.0", &Some(Vec::new()));
        assert!(bin.join("poimen-server").exists());
        prune(&dir.join("missing"), "1.0.0", &None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_process_list_probes_by_renaming() {
        let dir = temp_dir("prune-probe");
        let bin = dir.join("bin");
        let old = version_dir_with(&bin, "0.1.0", &["poimen-pty-host"]);
        prune(&bin, "0.2.0", &None);
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_without_a_bundle_stages_nothing() {
        let dir = temp_dir("prepare-missing");
        assert_eq!(
            prepare(&dir.join("poimen-server"), &dir.join("bin"), "0.9.0"),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_bundle_is_an_error() {
        let dir = temp_dir("missing");
        assert!(stage(&dir.join("nope.exe"), &dir.join("bin"), "0.8.0").is_err());
        assert!(!dir.join("bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_onto_an_existing_staged_copy_reuses_it() {
        let dir = temp_dir("in-use");
        let target = dir.join("poimen-server.exe");
        let locked = || io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(
            reuse_staged(locked(), &target).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::write(&target, "running").unwrap();
        assert!(reuse_staged(locked(), &target).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn denied() -> ClientError {
        ClientError::Connect(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    #[test]
    fn a_denied_connect_is_denied_on_windows_only() {
        assert!(matches!(
            classify(Err(denied()), true, "0.8.0"),
            Ping::Denied(_)
        ));
        assert_eq!(classify(Err(denied()), false, "0.8.0"), Ping::Absent);
        let refused = ClientError::Connect(io::Error::from(io::ErrorKind::ConnectionRefused));
        assert_eq!(classify(Err(refused), true, "0.8.0"), Ping::Absent);
    }

    #[test]
    fn a_ping_with_the_protocol_answers_and_another_is_absent() {
        let pong = |protocol| {
            Ok(poimen_client::protocol::PingResult {
                version: "0.8.0".into(),
                protocol,
                pid: 1,
                clients: Vec::new(),
            })
        };
        let current = poimen_client::protocol::PROTOCOL_VERSION;
        assert_eq!(classify(pong(current), false, "0.8.0"), Ping::Answers);
    }

    fn pong_of(
        version: &str,
        protocol: u32,
    ) -> Result<poimen_client::protocol::PingResult, ClientError> {
        Ok(poimen_client::protocol::PingResult {
            version: version.into(),
            protocol,
            pid: 1,
            clients: Vec::new(),
        })
    }

    #[test]
    fn an_older_server_is_outdated_and_a_newer_or_equal_one_answers() {
        let current = poimen_client::protocol::PROTOCOL_VERSION;
        assert_eq!(
            classify(pong_of("0.11.2", current), false, "0.12.0"),
            Ping::Outdated {
                running: "0.11.2".into()
            }
        );
        assert_eq!(
            classify(pong_of("0.12.0", current), false, "0.12.0"),
            Ping::Answers
        );
        assert_eq!(
            classify(pong_of("0.12.0-rc1", current), false, "0.12.0"),
            Ping::Answers
        );
        assert_eq!(
            classify(pong_of("0.13.0", current), false, "0.12.0"),
            Ping::Answers
        );
        // numeric, not textual: 0.9.0 is older than 0.10.0
        assert!(is_older("0.9.0", "0.10.0"));
        assert!(!is_older("garbage", "0.10.0"));
    }

    #[test]
    fn another_protocol_is_incompatible_not_absent() {
        let current = poimen_client::protocol::PROTOCOL_VERSION;
        assert_eq!(
            classify(pong_of("0.5.0", current + 1), false, "0.12.0"),
            Ping::Incompatible {
                running: "0.5.0".into(),
                protocol: current + 1
            }
        );
    }

    #[test]
    fn a_protocol_4_server_without_the_trust_handshake_is_incompatible() {
        // The app skips `trust::gate` on the poimen backend and relies on the `space.create`
        // trust handshake, which protocol 5 introduced; a protocol 4 server never asks.
        assert_eq!(
            classify(pong_of("0.14.0", 4), false, "0.14.0"),
            Ping::Incompatible {
                running: "0.14.0".into(),
                protocol: 4
            }
        );
        assert_eq!(
            classify(pong_of("0.15.0", 4), false, "0.14.0"),
            Ping::Incompatible {
                running: "0.15.0".into(),
                protocol: 4
            }
        );
    }

    #[test]
    fn a_protocol_5_server_without_apply_layout_is_incompatible() {
        // Rebuild and presets go through `space.apply_layout` (protocol 6) on the poimen
        // backend; a protocol 5 server answers it with `unknown_method`.
        assert_eq!(poimen_client::protocol::PROTOCOL_VERSION, 6);
        for running in ["0.14.0", "0.15.0"] {
            assert_eq!(
                classify(pong_of(running, 5), false, "0.14.0"),
                Ping::Incompatible {
                    running: running.into(),
                    protocol: 5
                }
            );
        }
    }

    fn outdated_fake() -> Fake {
        let fake = Fake::new(true, true, true);
        fake.found.replace(Some(Ping::Outdated {
            running: "0.1.0".into(),
        }));
        fake
    }

    #[test]
    fn an_outdated_server_is_reported_and_neither_stopped_nor_replaced() {
        let fake = outdated_fake();
        assert_eq!(
            ensure_running(&fake, true),
            Ok(Outcome::Outdated {
                running: "0.1.0".into(),
                bundled: BUNDLED_VERSION.into()
            })
        );
        assert_eq!(fake.shutdowns.get(), 0);
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn an_incompatible_server_is_reported_and_no_second_server_is_spawned() {
        let fake = Fake::new(true, true, true);
        fake.found.replace(Some(Ping::Incompatible {
            running: "0.1.0".into(),
            protocol: 1,
        }));
        assert_eq!(
            ensure_running(&fake, true),
            Ok(Outcome::Incompatible {
                running: "0.1.0".into(),
                protocol: 1
            })
        );
        assert!(fake.spawned.borrow().is_empty());
        assert_eq!(fake.shutdowns.get(), 0);
    }

    #[test]
    fn a_confirmed_restart_shuts_down_keeping_the_terminals_then_starts_the_bundled_server() {
        let fake = outdated_fake();
        assert_eq!(restart(&fake), Ok(Outcome::Started));
        assert_eq!(fake.shutdowns.get(), 1);
        assert_eq!(*fake.kept.borrow(), [true]);
        assert_eq!(fake.spawned.borrow().len(), 1);
        assert_eq!(fake.ping(), Ping::Answers);
    }

    #[test]
    fn a_restart_whose_shutdown_fails_starts_nothing() {
        let fake = Fake {
            shutdown_fails: true,
            ..outdated_fake()
        };
        assert!(matches!(restart(&fake), Err(StartError::Shutdown(_))));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn stop_without_a_server_asks_for_nothing() {
        let fake = Fake::new(false, true, true);
        assert_eq!(stop(&fake), Ok(()));
        assert_eq!(fake.shutdowns.get(), 0);
    }

    #[test]
    fn stop_shuts_a_current_server_down_ending_the_terminals_and_starts_nothing() {
        let fake = Fake::new(true, true, true);
        assert_eq!(stop(&fake), Ok(()));
        assert_eq!(fake.shutdowns.get(), 1);
        assert_eq!(*fake.kept.borrow(), [false]);
        assert!(fake.spawned.borrow().is_empty());
    }

    /// The real ping of a socket in a loose directory (0755), with a fake start.
    #[cfg(unix)]
    struct LooseDir {
        system: SystemRunner,
        fake: Fake,
    }

    #[cfg(unix)]
    impl Runner for LooseDir {
        fn ping(&self) -> Ping {
            if self.fake.spawned.borrow().is_empty() {
                self.system.ping()
            } else {
                self.fake.ping()
            }
        }
        fn resolve(&self) -> Option<PathBuf> {
            self.fake.resolve()
        }
        fn spawn(&self, program: &Path) -> io::Result<()> {
            self.fake.spawn(program)
        }
        fn shutdown(&self, keep_terminals: bool) -> Result<(), String> {
            self.fake.shutdown(keep_terminals)
        }
        fn sleep(&self, d: Duration) {
            self.fake.sleep(d)
        }
        fn now(&self) -> Duration {
            self.fake.now()
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_loose_socket_directory_on_unix_starts_a_server_instead_of_denying() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("loose");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let client = Client::new(dir.join("poimen.sock"));
        assert!(
            matches!(client.ping(), Err(ClientError::Connect(ref e)) if e.kind() == io::ErrorKind::PermissionDenied),
            "the directory check refuses the socket"
        );
        let runner = LooseDir {
            system: SystemRunner {
                client,
                origin: Instant::now(),
            },
            fake: Fake::new(false, true, true),
        };
        assert_eq!(runner.system.ping(), Ping::Absent);
        assert_eq!(ensure_running(&runner, true), Ok(Outcome::Started));
        assert_eq!(runner.fake.spawned.borrow().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_denied_connect_is_reported_and_starts_nothing() {
        let fake = Fake {
            denied: true,
            ..Fake::new(false, true, true)
        };
        assert_eq!(
            ensure_running(&fake, true),
            Err(StartError::Denied("denied".into()))
        );
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_running_server_is_left_alone() {
        let fake = Fake::new(true, true, true);
        assert_eq!(ensure_running(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn an_absent_server_is_started_and_awaited() {
        let fake = Fake::new(false, true, true);
        assert_eq!(ensure_running(&fake, true), Ok(Outcome::Started));
        assert_eq!(fake.spawned.borrow().len(), 1);
    }

    #[test]
    fn auto_start_off_starts_nothing() {
        let fake = Fake::new(false, true, true);
        assert_eq!(ensure_running(&fake, false), Ok(Outcome::Disabled));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_missing_binary_is_reported() {
        let fake = Fake::new(false, false, true);
        assert_eq!(ensure_running(&fake, true), Err(StartError::NotFound));
    }

    #[test]
    fn a_server_that_never_answers_times_out() {
        let fake = Fake::new(false, true, false);
        assert_eq!(
            ensure_running(&fake, true),
            Err(StartError::Timeout(START_TIMEOUT))
        );
        assert_eq!(fake.spawned.borrow().len(), 1);
    }
}
