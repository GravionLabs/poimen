//! The terminal backend poimen talks to, and starting it when it is not running (ADR 0003).
//!
//! poimen never owns pane state and never stops the server: `ensure_running` only starts a missing
//! server, detached from poimen, and waits until it answers. A server that already answers (also
//! one started by the herdr TUI) is left alone. A later `poimen-server` backend implements the same
//! trait.

use std::fmt;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::gui_path;
use crate::herdr::{Client, Probe, SUPPORTED_PROTOCOL, ServerInfo};

/// How long a started server may take to answer, and how often it is asked.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// What `ensure_running` found or did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A server already answered; nothing was started.
    AlreadyRunning,
    /// Nothing answered and `autoStart` is off.
    Disabled,
    /// poimen started the server and it answers.
    Started,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// The `herdr` program is not on the PATH poimen uses.
    NotFound,
    /// The process could not be spawned.
    Spawn(String),
    /// The server did not answer within the timeout.
    Timeout(Duration),
    /// The socket is bound but does not answer, and `autoStart` is off (nothing was waited for).
    NotAnswering,
    /// The started server exited before it answered.
    Exited,
    /// The started server speaks an older protocol than poimen supports.
    ProtocolTooOld { version: String, protocol: u32 },
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::NotFound => write!(
                f,
                "`herdr` was not found on PATH; install herdr or add it to the PATH of your login shell"
            ),
            StartError::Spawn(err) => write!(f, "could not start `herdr server`: {err}"),
            StartError::Timeout(limit) => write!(
                f,
                "`herdr server` did not answer within {} s",
                limit.as_secs()
            ),
            StartError::NotAnswering => {
                write!(f, "the herdr socket exists but the server is not answering")
            }
            StartError::Exited => write!(f, "`herdr server` exited right after it was started"),
            StartError::ProtocolTooOld { version, protocol } => write!(
                f,
                "herdr {version} speaks protocol {protocol}, poimen needs {SUPPORTED_PROTOCOL}; update herdr"
            ),
        }
    }
}

/// A backend poimen can bring up on its own.
pub trait Backend {
    /// Makes sure the backend answers: returns at once when it does, otherwise starts it (when
    /// allowed) and waits. Never stops or restarts a running backend.
    fn ensure_running(&self) -> Result<Outcome, StartError>;
}

/// The side effects `HerdrBackend` needs; faked in tests.
pub trait Runner {
    fn probe(&self) -> Probe;
    /// Finds `herdr` on the PATH of a GUI app.
    fn resolve(&self) -> Option<PathBuf>;
    /// Starts `<program> server` detached and returns without waiting for it.
    fn spawn(&self, program: &Path) -> io::Result<()>;
    fn sleep(&self, duration: Duration);
    /// Monotonic time since an arbitrary start; the deadline of `ensure_running` is measured with
    /// it, so slow probes count.
    fn now(&self) -> Duration;
    /// Whether the server started by `spawn` has exited again.
    fn exited(&self) -> bool;
}

pub struct HerdrBackend<R: Runner> {
    runner: R,
    auto_start: bool,
}

impl<R: Runner> HerdrBackend<R> {
    pub fn new(runner: R, auto_start: bool) -> Self {
        Self { runner, auto_start }
    }
}

fn too_old(info: &ServerInfo) -> Option<StartError> {
    match info.protocol {
        Some(protocol) if protocol < SUPPORTED_PROTOCOL => Some(StartError::ProtocolTooOld {
            version: info.version.clone(),
            protocol,
        }),
        _ => None,
    }
}

impl<R: Runner> Backend for HerdrBackend<R> {
    /// Only `Probe::Down` (nothing listens) allows a spawn, and only once: a socket that is bound
    /// but silent (server starting, busy, or started a moment ago by the herdr TUI or another
    /// poimen) is waited for up to `START_TIMEOUT` without spawning. A second poimen starting at
    /// the same instant can still pass its own re-probe before the first one spawns; the
    /// `StartLock` only serializes this process.
    fn ensure_running(&self) -> Result<Outcome, StartError> {
        let start = self.runner.now();
        let mut spawned = false;
        let mut first = true;
        // The deadline is checked before every probe (a probe can take up to PROBE_TIMEOUT), so the
        // call ends within START_TIMEOUT plus at most one probe.
        while first || self.runner.now().saturating_sub(start) < START_TIMEOUT {
            if !first {
                self.runner.sleep(POLL_INTERVAL);
                if self.runner.now().saturating_sub(start) >= START_TIMEOUT {
                    break;
                }
            }
            first = false;
            match self.runner.probe() {
                Probe::Up(info) if spawned => {
                    return match info.as_ref().and_then(too_old) {
                        Some(err) => Err(err),
                        None => Ok(Outcome::Started),
                    };
                }
                Probe::Up(_) => return Ok(Outcome::AlreadyRunning),
                Probe::Unresponsive if !self.auto_start => return Err(StartError::NotAnswering),
                Probe::Unresponsive => {}
                Probe::Down if spawned => {
                    if self.runner.exited() {
                        return Err(StartError::Exited);
                    }
                }
                Probe::Down if !self.auto_start => return Ok(Outcome::Disabled),
                Probe::Down => {
                    let program = self.runner.resolve().ok_or(StartError::NotFound)?;
                    // Re-probe right before spawning: the server may have appeared meanwhile.
                    if self.runner.probe() == Probe::Down {
                        self.runner
                            .spawn(&program)
                            .map_err(|e| StartError::Spawn(e.to_string()))?;
                        spawned = true;
                    }
                }
            }
        }
        Err(StartError::Timeout(START_TIMEOUT))
    }
}

/// The real runner: the herdr socket of the environment and the `herdr` binary of the login PATH.
pub struct SystemRunner {
    client: Client,
    origin: Instant,
    exited: Arc<AtomicBool>,
}

impl SystemRunner {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            origin: Instant::now(),
            exited: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Runner for SystemRunner {
    fn probe(&self) -> Probe {
        self.client.probe()
    }

    fn resolve(&self) -> Option<PathBuf> {
        gui_path::resolve_program("herdr", &gui_path::child_path())
    }

    fn spawn(&self, program: &Path) -> io::Result<()> {
        // Own process group, no terminal, no pipes: the server outlives poimen. It inherits
        // HERDR_SESSION / HERDR_SOCKET_PATH, so it listens where poimen looks. A small thread
        // reaps the child if it ever exits, so it does not stay a zombie while poimen runs.
        self.exited.store(false, Ordering::SeqCst);
        let exited = Arc::clone(&self.exited);
        let mut child = Command::new(program)
            .arg("server")
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
            exited.store(true, Ordering::SeqCst);
        });
        Ok(())
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

/// The result sent to the frontend (command `ensure_backend`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    /// `running`, `started`, `disabled` or `failed`.
    pub state: &'static str,
    /// Why the start failed; `None` otherwise.
    pub reason: Option<String>,
}

impl From<Result<Outcome, StartError>> for BackendStatus {
    fn from(result: Result<Outcome, StartError>) -> Self {
        match result {
            Ok(outcome) => Self {
                state: match outcome {
                    Outcome::AlreadyRunning => "running",
                    Outcome::Started => "started",
                    Outcome::Disabled => "disabled",
                },
                reason: None,
            },
            Err(err) => Self {
                state: "failed",
                reason: Some(err.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Answers `Down` for `down_probes` probes, then `up`; records what happened.
    struct Fake {
        down_probes: Cell<u32>,
        up: Probe,
        binary: Option<PathBuf>,
        spawn_error: bool,
        spawned: RefCell<Vec<PathBuf>>,
        slept: Cell<u32>,
        /// Fake clock; every probe and sleep advances it.
        clock: Cell<Duration>,
        probe_cost: Duration,
        exited: bool,
        /// Probe answers served after the `Down` ones and before `up`.
        script: RefCell<std::collections::VecDeque<Probe>>,
    }

    impl Fake {
        fn new(down_probes: u32, up: Probe) -> Self {
            Self {
                down_probes: Cell::new(down_probes),
                up,
                binary: Some(PathBuf::from("/opt/bin/herdr")),
                spawn_error: false,
                spawned: RefCell::new(Vec::new()),
                slept: Cell::new(0),
                clock: Cell::new(Duration::ZERO),
                probe_cost: Duration::ZERO,
                exited: false,
                script: RefCell::new(Default::default()),
            }
        }
    }

    impl Runner for &Fake {
        fn probe(&self) -> Probe {
            self.clock.set(self.clock.get() + self.probe_cost);
            if self.down_probes.get() > 0 {
                self.down_probes.set(self.down_probes.get() - 1);
                return Probe::Down;
            }
            if let Some(next) = self.script.borrow_mut().pop_front() {
                return next;
            }
            match &self.up {
                Probe::Down => Probe::Down,
                Probe::Unresponsive => Probe::Unresponsive,
                Probe::Up(info) => Probe::Up(info.as_ref().map(|i| ServerInfo {
                    version: i.version.clone(),
                    protocol: i.protocol,
                    supported: i.supported,
                })),
            }
        }
        fn resolve(&self) -> Option<PathBuf> {
            self.binary.clone()
        }
        fn spawn(&self, program: &Path) -> io::Result<()> {
            if self.spawn_error {
                return Err(io::Error::other("denied"));
            }
            self.spawned.borrow_mut().push(program.to_path_buf());
            Ok(())
        }
        fn sleep(&self, _d: Duration) {
            self.slept.set(self.slept.get() + 1);
            self.clock.set(self.clock.get() + _d);
        }
        fn now(&self) -> Duration {
            self.clock.get()
        }
        fn exited(&self) -> bool {
            self.exited
        }
    }

    fn up(protocol: Option<u32>) -> Probe {
        Probe::Up(Some(ServerInfo {
            version: "0.9.5".into(),
            protocol,
            supported: SUPPORTED_PROTOCOL,
        }))
    }

    fn run(fake: &Fake, auto_start: bool) -> Result<Outcome, StartError> {
        HerdrBackend::new(fake, auto_start).ensure_running()
    }

    #[test]
    fn a_running_server_is_not_started_again() {
        let fake = Fake::new(0, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_running_server_with_an_old_protocol_is_left_alone() {
        let fake = Fake::new(0, up(Some(5)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_missing_server_is_started_and_awaited() {
        let fake = Fake::new(3, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
        assert_eq!(*fake.spawned.borrow(), [PathBuf::from("/opt/bin/herdr")]);
        assert!(fake.slept.get() >= 1);
    }

    #[test]
    fn a_bound_but_silent_socket_is_waited_for_without_spawning() {
        let fake = Fake::new(0, up(Some(22)));
        fake.script
            .borrow_mut()
            .extend([Probe::Unresponsive, Probe::Unresponsive]);
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
        assert_eq!(fake.slept.get(), 2);
    }

    #[test]
    fn a_socket_that_never_answers_times_out_without_spawning() {
        let fake = Fake::new(0, Probe::Unresponsive);
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Timeout(Duration::from_secs(10)));
        assert!(fake.spawned.borrow().is_empty());
        assert_eq!(fake.slept.get(), 100);
    }

    #[test]
    fn slow_probes_count_against_the_deadline() {
        let mut fake = Fake::new(0, Probe::Unresponsive);
        fake.probe_cost = Duration::from_millis(1500);
        assert_eq!(run(&fake, true), Err(StartError::Timeout(START_TIMEOUT)));
        // 1.6 s per round: the 10 s deadline passes after 7 rounds, not after 100 attempts.
        assert_eq!(fake.slept.get(), 6);
        assert!(fake.clock.get() < START_TIMEOUT + Duration::from_millis(1500) + POLL_INTERVAL);
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_slow_server_that_is_ready_at_eight_seconds_is_accepted() {
        let mut fake = Fake::new(0, up(Some(22)));
        fake.probe_cost = Duration::from_millis(1500);
        // Five silent probes, the sixth answers at about 9.6 s of fake time.
        fake.script
            .borrow_mut()
            .extend([const { Probe::Unresponsive }; 5]);
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.clock.get() >= Duration::from_secs(8));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_started_server_that_exits_fails_fast() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.exited = true;
        assert_eq!(run(&fake, true), Err(StartError::Exited));
        assert_eq!(fake.spawned.borrow().len(), 1);
        assert!(fake.clock.get() < Duration::from_secs(1));
    }

    #[test]
    fn a_silent_socket_with_auto_start_off_says_the_server_is_not_answering() {
        let fake = Fake::new(0, Probe::Unresponsive);
        let err = run(&fake, false).unwrap_err();
        assert_eq!(err, StartError::NotAnswering);
        assert!(!err.to_string().contains("auto start"));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_refused_connection_spawns_exactly_once() {
        let fake = Fake::new(5, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
        assert_eq!(fake.spawned.borrow().len(), 1);
    }

    #[test]
    fn a_server_that_appears_before_the_spawn_is_not_started_again() {
        let fake = Fake::new(1, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_server_without_a_reported_protocol_is_accepted() {
        let fake = Fake::new(2, up(None));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
    }

    #[test]
    fn auto_start_off_starts_nothing() {
        let fake = Fake::new(u32::MAX, Probe::Down);
        assert_eq!(run(&fake, false), Ok(Outcome::Disabled));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn auto_start_off_still_reports_a_running_server() {
        let fake = Fake::new(0, up(Some(22)));
        assert_eq!(run(&fake, false), Ok(Outcome::AlreadyRunning));
    }

    #[test]
    fn a_missing_binary_is_reported_without_spawning() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.binary = None;
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::NotFound);
        assert!(err.to_string().contains("not found on PATH"));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_spawn_failure_carries_the_reason() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.spawn_error = true;
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Spawn("denied".into()));
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn a_server_that_never_answers_times_out_after_one_start() {
        let fake = Fake::new(u32::MAX, Probe::Down);
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Timeout(Duration::from_secs(10)));
        assert!(err.to_string().contains("10 s"));
        assert_eq!(fake.spawned.borrow().len(), 1);
        assert_eq!(fake.slept.get(), 100);
    }

    #[test]
    fn a_started_server_with_an_old_protocol_is_an_error() {
        let fake = Fake::new(2, up(Some(21)));
        let err = run(&fake, true).unwrap_err();
        assert_eq!(
            err,
            StartError::ProtocolTooOld {
                version: "0.9.5".into(),
                protocol: 21
            }
        );
        assert!(err.to_string().contains("protocol 21"));
    }

    #[test]
    fn the_frontend_status_names_the_state_and_reason() {
        let ok: BackendStatus = Ok(Outcome::Started).into();
        assert_eq!((ok.state, ok.reason), ("started", None));
        let off: BackendStatus = Ok(Outcome::Disabled).into();
        assert_eq!(off.state, "disabled");
        let bad: BackendStatus = Err(StartError::NotFound).into();
        assert_eq!(bad.state, "failed");
        assert!(bad.reason.unwrap().contains("herdr"));
        let json = serde_json::to_value(BackendStatus::from(Ok(Outcome::AlreadyRunning))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "running", "reason": null})
        );
    }

    #[test]
    fn the_real_runner_finds_a_program_on_the_login_path_or_not_at_all() {
        // `sh` exists on every supported system; this only reads the PATH, it starts nothing.
        let found = gui_path::resolve_program("sh", &gui_path::child_path());
        assert!(found.is_some());
    }
}
