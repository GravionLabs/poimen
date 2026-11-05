//! Starting `poimen-server` (ADR 0003) when it is absent. poimen never stops it: the server owns
//! the PTYs and outlives the app. The wire protocol lives in `crates/poimen-protocol`, the client
//! in `crates/poimen-client`; this module only decides whether to start a server and does so
//! detached, like `backend.rs` does for herdr. On Windows it starts a per-version copy under
//! `%LOCALAPPDATA%\poimen-server\bin` (see [`stage`]), breaking away from the app's job.

use std::fmt;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use poimen_client::{Client, ClientError};

use crate::backend::{Outcome, POLL_INTERVAL, START_TIMEOUT};
use crate::gui_path;

const PROGRAM: &str = "poimen-server";

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// No `poimen-server` next to the app or on the PATH poimen uses.
    NotFound,
    Spawn(String),
    Timeout(Duration),
    /// A server runs but refused the connection (Windows: started elevated, see [`Ping`]).
    Denied(String),
}

/// What a `ping` of the socket found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ping {
    /// A server answered with the protocol poimen speaks.
    Answers,
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
            StartError::Denied(e) => write!(f, "{e}"),
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
    fn sleep(&self, duration: Duration);
    fn now(&self) -> Duration;
}

/// Returns at once when a server answers; otherwise starts one (when `auto_start`) and waits until
/// it answers. Never stops or restarts a running server.
pub fn ensure_running(runner: &impl Runner, auto_start: bool) -> Result<Outcome, StartError> {
    match runner.ping() {
        Ping::Answers => return Ok(Outcome::AlreadyRunning),
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
            Ping::Absent => {}
        }
    }
    Err(StartError::Timeout(START_TIMEOUT))
}

/// The file name of the bundled server next to the app executable: Tauri installs the sidecar
/// as `poimen-server.exe` on Windows (not the first `PATHEXT` candidate, `.COM`).
pub fn beside_name(windows: bool) -> String {
    if windows {
        format!("{PROGRAM}.exe")
    } else {
        PROGRAM.to_string()
    }
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

/// Copies the bundled server to `<bin_root>/<version>/poimen-server.exe` and returns that path
/// (Windows): the running copy then locks only itself, so an update can replace the installed
/// one. An up-to-date copy (same size and modification time) is reused; copies of other versions
/// are removed when they are not running (a running exe cannot be deleted on Windows).
pub fn stage(bundled: &Path, bin_root: &Path, version: &str) -> io::Result<PathBuf> {
    let source = std::fs::metadata(bundled)?;
    let dir = bin_root.join(version);
    let target = dir.join(beside_name(true));
    let same = |m: &std::fs::Metadata| {
        m.len() == source.len() && m.modified().ok() == source.modified().ok()
    };
    if !std::fs::metadata(&target).is_ok_and(|m| same(&m)) {
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{}.{}.tmp", beside_name(true), std::process::id()));
        std::fs::copy(bundled, &tmp)?;
        let copied = std::fs::File::options()
            .write(true)
            .open(&tmp)
            .and_then(|f| source.modified().and_then(|t| f.set_modified(t)))
            .and_then(|()| std::fs::rename(&tmp, &target).or_else(|e| reuse_staged(e, &target)));
        let _ = std::fs::remove_file(&tmp);
        copied?;
    }
    if let Ok(entries) = std::fs::read_dir(bin_root) {
        for entry in entries.flatten() {
            if entry.file_name() != version && entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(target)
}

/// A failed rename onto an existing staged copy (Windows: another poimen staged it and its server
/// runs from it, so it is locked) keeps that copy; any other failure stays an error.
fn reuse_staged(error: io::Error, target: &Path) -> io::Result<()> {
    if target.is_file() { Ok(()) } else { Err(error) }
}

/// What a ping result means. Only on Windows is a denied connect [`Ping::Denied`] (the pipe of an
/// elevated server); on Unix the client's directory check denies a loose or foreign socket
/// directory, and starting a server heals that (it makes its own directory private again).
fn classify(
    result: Result<poimen_client::protocol::PingResult, ClientError>,
    windows: bool,
) -> Ping {
    match result {
        Ok(p) if p.protocol == poimen_client::protocol::PROTOCOL_VERSION => Ping::Answers,
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
        classify(self.client.ping(), cfg!(windows))
    }

    fn resolve(&self) -> Option<PathBuf> {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(beside_name(cfg!(windows)))))
            .filter(|p| p.is_file());
        beside.or_else(|| gui_path::resolve_program(PROGRAM, &gui_path::child_path()))
    }

    fn spawn(&self, program: &Path) -> io::Result<()> {
        // Own process group, no terminal, no pipes, never waited on: the server outlives poimen.
        let staged = cfg!(windows)
            .then(|| std::env::var_os("LOCALAPPDATA"))
            .flatten()
            .and_then(|dir| poimen_client::protocol::windows_dir_from(Some(dir)))
            .and_then(|dir| stage(program, &dir.join("bin"), env!("CARGO_PKG_VERSION")).ok());
        let program = staged.as_deref().unwrap_or(program);
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
                Ping::Answers
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
        assert_eq!(beside_name(true), "poimen-server.exe");
        assert_eq!(beside_name(false), "poimen-server");
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
        assert!(matches!(classify(Err(denied()), true), Ping::Denied(_)));
        assert_eq!(classify(Err(denied()), false), Ping::Absent);
        let refused = ClientError::Connect(io::Error::from(io::ErrorKind::ConnectionRefused));
        assert_eq!(classify(Err(refused), true), Ping::Absent);
    }

    #[test]
    fn a_ping_with_the_protocol_answers_and_another_is_absent() {
        let pong = |protocol| {
            Ok(poimen_client::protocol::PingResult {
                version: "0.8.0".into(),
                protocol,
                pid: 1,
            })
        };
        let current = poimen_client::protocol::PROTOCOL_VERSION;
        assert_eq!(classify(pong(current), false), Ping::Answers);
        assert_eq!(classify(pong(current + 1), false), Ping::Absent);
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
