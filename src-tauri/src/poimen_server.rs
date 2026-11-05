//! Starting `poimen-server` (ADR 0003) when it is absent. poimen never stops it: the server owns
//! the PTYs and outlives the app. The wire protocol lives in `crates/poimen-protocol`, the client
//! in `crates/poimen-client`; this module only decides whether to start a server and does so
//! detached, like `backend.rs` does for herdr.

use std::fmt;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use poimen_client::Client;

use crate::backend::{Outcome, POLL_INTERVAL, START_TIMEOUT};
use crate::gui_path;

const PROGRAM: &str = "poimen-server";

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// No `poimen-server` next to the app or on the PATH poimen uses.
    NotFound,
    Spawn(String),
    Timeout(Duration),
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
        }
    }
}

/// The side effects of `ensure_running`; faked in tests.
pub trait Runner {
    /// Whether a server answers `ping` with the protocol poimen speaks.
    fn answers(&self) -> bool;
    fn resolve(&self) -> Option<PathBuf>;
    /// Starts the program detached and returns without waiting for it.
    fn spawn(&self, program: &Path) -> io::Result<()>;
    fn sleep(&self, duration: Duration);
    fn now(&self) -> Duration;
}

/// Returns at once when a server answers; otherwise starts one (when `auto_start`) and waits until
/// it answers. Never stops or restarts a running server.
pub fn ensure_running(runner: &impl Runner, auto_start: bool) -> Result<Outcome, StartError> {
    if runner.answers() {
        return Ok(Outcome::AlreadyRunning);
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
        if runner.answers() {
            return Ok(Outcome::Started);
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
    fn answers(&self) -> bool {
        self.client
            .ping()
            .is_ok_and(|p| p.protocol == poimen_client::protocol::PROTOCOL_VERSION)
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
        let mut command = gui_path::command(program);
        if !cfg!(windows) {
            command.env("PATH", gui_path::child_path());
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(0x0000_0200 | gui_path::CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
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
        running: Cell<bool>,
        installed: bool,
        becomes_up: bool,
        spawned: RefCell<Vec<PathBuf>>,
        clock: Cell<Duration>,
    }

    impl Fake {
        fn new(running: bool, installed: bool, becomes_up: bool) -> Self {
            Self {
                running: Cell::new(running),
                installed,
                becomes_up,
                spawned: RefCell::new(vec![]),
                clock: Cell::new(Duration::ZERO),
            }
        }
    }

    impl Runner for Fake {
        fn answers(&self) -> bool {
            self.running.get()
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
