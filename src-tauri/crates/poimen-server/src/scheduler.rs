//! The scheduler of ADR 0020 (#1000): at the due times of the user's `schedules[]` the command of
//! a task mode starts as the process of a new terminal in the space of the repository. The server
//! keeps in `server.json` which due time it handled (written before the run starts, so none is
//! handled twice), the last result and the runs. A missed run is caught up once within the grace
//! time, two runs of a schedule never overlap, and the global pause and the runs per day limit
//! hold. Only the user config is read: nothing of a project starts anything.
//!
//! Agent script schedules (#1096) use this scheduler too; they are not built yet.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use poimen_core::schedules::{self, GRACE_MS, Schedule, Schedules};
use poimen_core::settings::config_path;
use poimen_core::tasks::{self, Cmd, TaskVars};
use poimen_core::{git, gui_path};
use poimen_protocol::{
    ScheduleInfo, ScheduleList, ScheduleOutcome, ScheduleResult, ScheduleRun, TerminalCreate,
    TerminalStatus, TrustAnswer,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    ApiResult, Inner, Server, Shared, build_space, create, lock, program_available, pty,
    spawn_terminal, type_commands,
};

/// How often the scheduler looks at the clock.
pub(crate) const SWEEP: Duration = Duration::from_secs(1);
/// How long a `precheck` may take before it counts as failed.
const PRECHECK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PRECHECK_POLL: Duration = Duration::from_millis(50);
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// The runs kept per schedule: at least a day's worth of the highest runs per day limit.
const KEEP_RUNS: usize = 96;

/// Held while a run finds or creates the space of its repository, so that two runs due at once
/// make one space.
static SPACES: Mutex<()> = Mutex::new(());

/// What `server.json` keeps of a schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredSchedule {
    pub(crate) key: String,
    pub(crate) dir: String,
    pub(crate) mode: String,
    /// Every due time up to here is handled.
    pub(crate) handled_ms: u64,
    /// Switched off, or gone from the config while a run of it was running: nothing is caught up
    /// when it comes back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) off: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last: Option<ScheduleResult>,
    /// Newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) runs: Vec<ScheduleRun>,
}

impl StoredSchedule {
    fn running(&self) -> bool {
        self.runs
            .iter()
            .any(|r| r.outcome == ScheduleOutcome::Running)
    }

    fn skipped(&mut self, now: u64, reason: &str) {
        self.record(ScheduleOutcome::Skipped, now, Some(reason.to_owned()));
    }

    fn record(&mut self, outcome: ScheduleOutcome, at_ms: u64, reason: Option<String>) {
        self.last = Some(ScheduleResult {
            outcome,
            at_ms,
            reason,
        });
    }
}

/// The scheduler's part of the server state, under the server's `inner` lock.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// By schedule key.
    entries: BTreeMap<String, StoredSchedule>,
    /// The schedules whose precheck or start is under way; they count as running.
    starting: HashSet<String>,
    /// `server.shutdown` began: nothing starts any more.
    halted: bool,
}

impl State {
    pub(crate) fn load(stored: Vec<StoredSchedule>) -> Self {
        Self {
            entries: stored.into_iter().map(|s| (s.key.clone(), s)).collect(),
            starting: HashSet::new(),
            halted: false,
        }
    }

    pub(crate) fn halt(&mut self) {
        self.halted = true;
    }

    pub(crate) fn stored(&self) -> Vec<StoredSchedule> {
        self.entries.values().cloned().collect()
    }

    /// Records the end of the run in `terminal_id`; `false` when no run is in it.
    pub(crate) fn ended(&mut self, terminal_id: &str, exit_code: Option<u32>, now: u64) -> bool {
        for entry in self.entries.values_mut() {
            let Some(run) = entry
                .runs
                .iter_mut()
                .find(|r| r.terminal_id == terminal_id && r.outcome == ScheduleOutcome::Running)
            else {
                continue;
            };
            let (outcome, reason) = match exit_code {
                Some(0) => (ScheduleOutcome::Ok, None),
                Some(code) => (ScheduleOutcome::Failed, Some(format!("exit code {code}"))),
                None => (
                    ScheduleOutcome::Failed,
                    Some("the terminal ended, its exit code is not known".to_owned()),
                ),
            };
            run.outcome = outcome;
            run.exit_code = exit_code;
            run.ended_ms = Some(now);
            entry.record(outcome, now, reason);
            return true;
        }
        false
    }

    /// Whether a run of the schedule is starting or running, also one under an earlier cron
    /// expression of the same directory and mode.
    fn busy(&self, s: &Schedule, configured: &Schedules) -> bool {
        self.starting.contains(&s.key)
            || self.entries.values().any(|e| {
                e.running()
                    && (e.key == s.key
                        || (e.dir == s.dir
                            && e.mode == s.mode
                            && !configured.list.iter().any(|c| c.key == e.key)))
            })
    }
}

/// A run the sweep decided to start.
struct Due {
    schedule: Schedule,
    due_ms: u64,
}

/// The user config's schedules, read now; `None` for a file that cannot be read or parsed (half
/// edited), which changes nothing.
fn configured(home: Option<&Path>) -> Option<Schedules> {
    let root = match home.map(|h| std::fs::read_to_string(config_path(h))) {
        None => Value::Null,
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Some(Err(_)) => return None,
        Some(Ok(text)) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)?,
    };
    Some(schedules::parse(&root, home, &mut Vec::new()))
}

/// The argv of the mode's command: an argv list as it is, a shell string run by the user's shell.
fn argv_of(s: &Schedule) -> Vec<String> {
    let repo = git::toplevel(&s.dir).unwrap_or_else(|| s.dir.clone());
    let vars = TaskVars {
        repo: Path::new(&repo)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        ..TaskVars::default()
    };
    match tasks::expand_command(&s.command, &vars) {
        Cmd::Argv(argv) => argv,
        Cmd::Shell(line) => {
            let shell = pty::default_shell();
            let name = pty::program_name(&shell).to_ascii_lowercase();
            let flags: &[&str] = match name.as_str() {
                "cmd" => &["/d", "/c"],
                "pwsh" | "powershell" => &["-NoLogo", "-Command"],
                _ => &["-c"],
            };
            let mut argv = vec![shell];
            argv.extend(flags.iter().map(|f| (*f).to_owned()));
            argv.push(line);
            argv
        }
    }
}

/// Runs the precheck in `dir`: `Err` says why it did not exit with 0.
fn precheck(argv: &[String], dir: &str) -> Result<(), String> {
    let mut child = gui_path::command(&argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        // The server's own `PATH` can be the thin one of a GUI app.
        .env("PATH", gui_path::child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("precheck could not start: {e}"))?;
    let end = Instant::now() + PRECHECK_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("precheck failed ({status})")),
            Ok(None) if Instant::now() < end => thread::sleep(PRECHECK_POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("precheck timed out".into());
            }
            Err(e) => return Err(format!("precheck failed: {e}")),
        }
    }
}

/// Whether the two directories are the same place: the same path or the same git checkout.
fn same_place(a: &str, b: &str) -> bool {
    let real = |d: &str| std::fs::canonicalize(d).ok();
    if real(a).is_some_and(|a| Some(a) == real(b)) {
        return true;
    }
    match (git::toplevel(a), git::toplevel(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

impl Shared {
    pub(crate) fn clock_ms(&self) -> u64 {
        (self.clock.read().unwrap_or_else(|e| e.into_inner()))()
    }

    /// One pass of the scheduler at the current time of the clock.
    pub(crate) fn schedule_sweep(self: &Arc<Self>) {
        let Some(config) = configured(self.home.as_deref()) else {
            return;
        };
        let now = self.clock_ms();
        let mut due = Vec::new();
        {
            let mut guard = lock(&self.inner);
            let inner = &mut *guard;
            if inner.schedules.halted {
                return;
            }
            let mut changed = end_lost_runs(inner, now);
            let state = &mut inner.schedules;
            // A schedule that left the config is forgotten once nothing of it runs.
            let before = state.entries.len();
            state
                .entries
                .retain(|key, e| e.running() || config.list.iter().any(|s| s.key == *key));
            changed |= state.entries.len() != before;
            for entry in state.entries.values_mut() {
                if !entry.off && !config.list.iter().any(|s| s.key == entry.key) {
                    entry.off = true;
                    changed = true;
                }
            }
            for s in &config.list {
                let busy = state.busy(s, &config);
                let entry = state.entries.entry(s.key.clone()).or_insert_with(|| {
                    changed = true;
                    // A new schedule counts from now and has nothing to catch up.
                    StoredSchedule {
                        key: s.key.clone(),
                        dir: s.dir.clone(),
                        mode: s.mode.clone(),
                        handled_ms: now,
                        ..StoredSchedule::default()
                    }
                });
                if entry.off == s.enabled {
                    // Switched off or on: what was due meanwhile is not caught up.
                    entry.off = !s.enabled;
                    if s.enabled {
                        entry.handled_ms = entry.handled_ms.max(now);
                    }
                    changed = true;
                }
                if entry.off {
                    continue;
                }
                let oldest = now.saturating_sub(GRACE_MS + 1).max(entry.handled_ms);
                let Some(newest) = s.timing.latest_between(oldest, now) else {
                    // Nothing within the grace time; was something missed before it?
                    if oldest > entry.handled_ms
                        && s.timing
                            .next_after(entry.handled_ms)
                            .is_some_and(|t| t <= now)
                    {
                        entry.handled_ms = oldest;
                        entry.skipped(
                            now,
                            if config.paused {
                                "paused"
                            } else {
                                "missed by more than 12 hours"
                            },
                        );
                        changed = true;
                    }
                    continue;
                };
                entry.handled_ms = newest;
                changed = true;
                let day = now.saturating_sub(DAY_MS);
                let today = entry.runs.iter().filter(|r| r.started_ms > day).count();
                if config.paused {
                    entry.skipped(now, "paused");
                } else if busy {
                    entry.skipped(now, "the last run is still running");
                } else if today >= config.max_runs_per_day as usize {
                    entry.skipped(now, "the limit of runs per day is reached");
                } else {
                    state.starting.insert(s.key.clone());
                    due.push(Due {
                        schedule: s.clone(),
                        due_ms: newest,
                    });
                }
            }
            // The handled due times are on disk before anything starts.
            if changed && !self.persist(inner) {
                for d in due.drain(..) {
                    inner.schedules.starting.remove(&d.schedule.key);
                    if let Some(entry) = inner.schedules.entries.get_mut(&d.schedule.key) {
                        entry.skipped(now, "server.json could not be written");
                    }
                }
            }
        }
        for d in due {
            let shared = Arc::clone(self);
            // A thread each: a slow precheck holds up no other schedule.
            thread::spawn(move || shared.run_due(d));
        }
    }

    /// Prechecks and starts a due run, and records why when it does not start.
    fn run_due(self: &Arc<Self>, due: Due) {
        let s = &due.schedule;
        let checked = match &s.precheck {
            Some(argv) => precheck(argv, &s.dir),
            None => Ok(()),
        };
        let result = match checked {
            Err(reason) => Err((ScheduleOutcome::Skipped, reason)),
            // The precheck takes time: a pause or a switch off meanwhile holds.
            Ok(()) => match configured(self.home.as_deref()) {
                Some(c) if c.paused => Err((ScheduleOutcome::Skipped, "paused".to_owned())),
                Some(c) if !c.list.iter().any(|n| n.key == s.key && n.enabled) => {
                    Err((ScheduleOutcome::Skipped, "switched off".to_owned()))
                }
                Some(_) => self
                    .start_run(&due)
                    .map_err(|e| (ScheduleOutcome::Failed, e)),
                // Whether it is paused or switched off now is not known.
                None => Err((
                    ScheduleOutcome::Skipped,
                    "config.json cannot be read".to_owned(),
                )),
            },
        };
        if let Err((outcome, reason)) = result {
            self.log(
                "scheduler",
                &format!("schedule {} did not run: {reason}", s.key),
            );
            let now = self.clock_ms();
            let mut inner = lock(&self.inner);
            inner.schedules.starting.remove(&s.key);
            if let Some(entry) = inner.schedules.entries.get_mut(&s.key) {
                entry.record(outcome, now, Some(reason));
            }
            self.persist(&inner);
        }
    }

    /// The space of `dir`; without one it is created with the user's layout. The project file
    /// is not read: it neither asks nor starts anything here.
    fn space_for(self: &Arc<Self>, dir: &str) -> Result<String, String> {
        let _one = SPACES.lock().unwrap_or_else(|e| e.into_inner());
        let spaces = lock(&self.inner).layout.spaces.clone();
        // The exact directory wins over another one of the same checkout.
        let found = spaces
            .iter()
            .find(|s| s.cwd == dir)
            .or_else(|| spaces.iter().find(|s| same_place(&s.cwd, dir)));
        if let Some(space) = found {
            return Ok(space.id.clone());
        }
        let build = match create::prepare(
            self.home.as_deref(),
            dir,
            Some(&TrustAnswer::Without),
            create::Wanted::User,
            &program_available(),
        )
        .map_err(|(_, message)| message)?
        {
            create::Prepared::Build(build) => build,
            create::Prepared::Ask(_) => return Err("the space needs an answer".into()),
        };
        let name = Path::new(dir)
            .file_name()
            .map_or_else(|| dir.to_owned(), |n| n.to_string_lossy().into_owned());
        if lock(&self.inner).schedules.halted {
            return Err("the server is shutting down".into());
        }
        let (mut created, typing) =
            build_space(self, name, build).map_err(|(_, message)| message)?;
        type_commands(typing, &mut created);
        Ok(created.space.id)
    }

    /// Starts the command as the process of a new terminal in its own tab.
    fn start_run(self: &Arc<Self>, due: &Due) -> Result<(), String> {
        let s = &due.schedule;
        if !Path::new(&s.dir).is_dir() {
            return Err(format!("{} is not a directory", s.dir));
        }
        let space_id = self.space_for(&s.dir)?;
        let command = argv_of(s);
        let now = self.clock_ms();
        // Read before the lock: the first read asks the login shell.
        let path = gui_path::child_path();
        // Held until the run is recorded, so its exit cannot come first.
        let mut inner = lock(&self.inner);
        if inner.schedules.halted {
            return Err("the server is shutting down".into());
        }
        let terminal = spawn_terminal(
            self,
            &mut inner,
            TerminalCreate {
                space_id,
                name: Some(format!("schedule {}", s.mode)),
                cwd: Some(s.dir.clone()),
                command: Some(command),
                env: [("PATH".to_owned(), path)].into(),
                ..TerminalCreate::default()
            },
        )
        .map_err(|(_, message)| message)?;
        inner.schedules.starting.remove(&s.key);
        if let Some(entry) = inner.schedules.entries.get_mut(&s.key) {
            entry.runs.push(ScheduleRun {
                terminal_id: terminal.id.clone(),
                due_ms: due.due_ms,
                started_ms: now,
                ended_ms: None,
                exit_code: None,
                outcome: ScheduleOutcome::Running,
            });
            let over = entry.runs.len().saturating_sub(KEEP_RUNS);
            entry.runs.drain(..over);
            entry.record(ScheduleOutcome::Running, now, None);
        }
        let events = inner
            .layout
            .add_tab(
                &terminal.space_id,
                self.new_id("tab"),
                terminal.name.clone(),
                &terminal.id,
            )
            .map_err(|(_, message)| message)?;
        self.apply(&inner, events);
        Ok(())
    }
}

/// Ends the runs whose terminal is gone or has exited without the server noting it (closed, or
/// ended while no server ran).
fn end_lost_runs(inner: &mut Inner, now: u64) -> bool {
    let lost: Vec<(String, Option<u32>)> = inner
        .schedules
        .entries
        .values()
        .flat_map(|e| &e.runs)
        .filter(|r| r.outcome == ScheduleOutcome::Running)
        .filter_map(
            |r| match inner.terminals.iter().find(|t| t.info.id == r.terminal_id) {
                Some(t) if t.info.status != TerminalStatus::Exited => None,
                found => Some((r.terminal_id.clone(), found.and_then(|t| t.info.exit_code))),
            },
        )
        .collect();
    for (id, code) in &lost {
        inner.schedules.ended(id, *code, now);
    }
    !lost.is_empty()
}

/// `schedule.list`: the schedules of the user config with what the server keeps of them.
pub(crate) fn list(shared: &Shared) -> ApiResult {
    let config = configured(shared.home.as_deref()).unwrap_or_default();
    let now = shared.clock_ms();
    let inner = lock(&shared.inner);
    let schedules = config
        .list
        .iter()
        .map(|s| {
            let entry = inner.schedules.entries.get(&s.key);
            let after = entry.map_or(now, |e| e.handled_ms.max(now));
            ScheduleInfo {
                key: s.key.clone(),
                cron: s.cron.clone(),
                timezone: s.timezone.clone(),
                dir: s.dir.clone(),
                mode: s.mode.clone(),
                enabled: s.enabled,
                next_ms: (s.enabled && !config.paused)
                    .then(|| s.timing.next_after(after))
                    .flatten(),
                last: entry.and_then(|e| e.last.clone()),
                runs: entry.map(|e| e.runs.clone()).unwrap_or_default(),
            }
        })
        .collect();
    Ok(json!(ScheduleList {
        paused: config.paused,
        schedules,
    }))
}

impl Server {
    /// Replaces the clock of the scheduler (milliseconds since the epoch); tests give their own
    /// before `serve`.
    #[doc(hidden)]
    pub fn set_clock(&self, clock: impl Fn() -> u64 + Send + Sync + 'static) {
        *self.shared.clock.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(clock);
    }

    /// One pass of the scheduler; `serve` runs it on a timer, tests call it. Take it before
    /// `serve`, which consumes the server.
    #[doc(hidden)]
    pub fn schedule_sweeper(&self) -> impl Fn() + Send + Sync + 'static {
        let shared = Arc::clone(&self.shared);
        move || shared.schedule_sweep()
    }
}

/// Runs the scheduler while the server serves; the first pass catches up at once.
pub(crate) fn spawn_sweeper(shared: &Arc<Shared>) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        loop {
            shared.schedule_sweep();
            thread::sleep(SWEEP);
        }
    });
}
