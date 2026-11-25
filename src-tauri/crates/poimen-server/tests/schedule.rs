//! Schedules of command task modes (#1000, ADR 0020) against the server with a fake clock and a
//! fake command: a shell script that writes a line and ends with the code the test chose.
//!
//! Unix-only like `agent.rs`: the fake command is a shell script.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{ScheduleInfo, ScheduleList, ScheduleOutcome, SpaceList};
use poimen_server::{Config, PtyHost, Server, StartError};
use serde_json::{Value, json};

const LONG: Duration = Duration::from_secs(15);
const MINUTE: u64 = 60_000;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
/// 2026-01-10 00:00:00 UTC.
const MIDNIGHT: u64 = 1_768_003_200_000;
/// 08:59, a minute before the due time of `0 9 * * *`.
const BEFORE: u64 = MIDNIGHT + 9 * HOUR - MINUTE;
const NINE: u64 = MIDNIGHT + 9 * HOUR;

/// The directories of one test; they outlive the servers started in it.
struct Dirs {
    root: PathBuf,
    clock: Arc<AtomicU64>,
    _guard: tempfile::TempDir,
}

impl Dirs {
    fn new() -> Self {
        let guard = tempfile::Builder::new()
            .prefix("poimen-sched-")
            .tempdir()
            .unwrap();
        let root = guard.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("repo")).unwrap();
        // Writes a line per start; waits while `hold` exists; ends with the code in `code`.
        std::fs::write(
            root.join("job.sh"),
            "d=$(dirname \"$0\")\necho \"run in $(pwd)\" >> \"$d/runs.log\"\n\
             while [ -e \"$d/hold\" ]; do sleep 0.05; done\n\
             exit $(cat \"$d/code\" 2>/dev/null || echo 0)\n",
        )
        .unwrap();
        Self {
            root,
            clock: Arc::new(AtomicU64::new(BEFORE)),
            _guard: guard,
        }
    }

    fn repo(&self) -> String {
        self.root.join("repo").display().to_string()
    }

    fn at(&self, ms: u64) {
        self.clock.store(ms, Ordering::SeqCst);
    }

    fn schedule(&self, cron: &str) -> Value {
        json!({"cron": cron, "timezone": "UTC", "dir": self.repo(), "mode": "job"})
    }

    /// Writes the user config with `schedules`; `extra` adds top-level keys.
    fn config(&self, schedules: Value, extra: Value) {
        let job = self.root.join("job.sh").display().to_string();
        let mut config = json!({
            "tasks": {"modes": [{"name": "job", "command": ["sh", job], "worktree": false}]},
            "schedules": schedules,
        });
        for (key, value) in extra.as_object().into_iter().flatten() {
            config[key] = value.clone();
        }
        let path = poimen_core::settings::config_path(&self.root.join("home"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, config.to_string()).unwrap();
    }

    fn runs_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("runs.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn start(&self) -> Env<'_> {
        let config = Config {
            socket: self.root.join("run").join("server.sock"),
            state_file: self.root.join("state").join("server.json"),
            history_dir: self.root.join("state").join("history"),
            scrollback: 100,
            home: Some(self.root.join("home")),
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        // The lock of a server just shut down can be held a moment longer by a child that another
        // test of this process is just spawning.
        let deadline = Instant::now() + LONG;
        let server = loop {
            match Server::bind(config.clone()) {
                Err(StartError::AlreadyRunning(_)) if Instant::now() < deadline => {
                    thread::yield_now();
                }
                server => break server.unwrap(),
            }
        };
        let clock = Arc::clone(&self.clock);
        server.set_clock(move || clock.load(Ordering::SeqCst));
        let sweep = Box::new(server.schedule_sweeper());
        let (stop_tx, stopped) = mpsc::channel();
        server.on_shutdown(move || {
            let _ = stop_tx.send(());
        });
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        let subscription = client.subscribe().unwrap();
        let (event_tx, events) = mpsc::channel();
        thread::spawn(move || {
            for event in subscription {
                if event.is_err() || event_tx.send(()).is_err() {
                    return;
                }
            }
        });
        Env {
            dirs: self,
            client,
            sweep,
            stopped,
            events,
        }
    }
}

/// A running server; dropping it stops it.
struct Env<'a> {
    dirs: &'a Dirs,
    client: Client,
    sweep: Box<dyn Fn() + Send + Sync>,
    stopped: Receiver<()>,
    events: Receiver<()>,
}

impl Env<'_> {
    /// Sets the clock and lets the scheduler look at it.
    fn tick(&self, ms: u64) {
        self.dirs.at(ms);
        (self.sweep)();
    }

    fn list(&self) -> ScheduleList {
        serde_json::from_value(self.client.request("schedule.list", json!({})).unwrap()).unwrap()
    }

    fn only(&self) -> ScheduleInfo {
        let mut list = self.list().schedules;
        assert_eq!(list.len(), 1);
        list.remove(0)
    }

    /// Waits until the only schedule satisfies `ok`; looks again on every event of the server.
    fn until(&self, ok: impl Fn(&ScheduleInfo) -> bool) -> ScheduleInfo {
        let deadline = Instant::now() + LONG;
        loop {
            let info = self.only();
            if ok(&info) {
                return info;
            }
            assert!(Instant::now() < deadline, "the schedule stays {info:?}");
            let _ = self.events.recv_timeout(Duration::from_millis(50));
        }
    }

    /// Waits for the end of the run number `n` (from 1).
    fn finished(&self, n: usize) -> ScheduleInfo {
        self.until(|s| s.runs.len() == n && s.runs[n - 1].outcome != ScheduleOutcome::Running)
    }

    fn spaces(&self) -> SpaceList {
        serde_json::from_value(self.client.request("space.list", json!({})).unwrap()).unwrap()
    }

    fn last_reason(&self) -> (ScheduleOutcome, Option<String>) {
        let last = self.only().last.expect("a last result");
        (last.outcome, last.reason)
    }
}

impl Drop for Env<'_> {
    fn drop(&mut self) {
        let _ = self.client.request("server.shutdown", json!({}));
        let _ = self.stopped.recv_timeout(LONG);
    }
}

fn names(spaces: &SpaceList) -> Vec<&str> {
    spaces.tabs.iter().map(|t| t.name.as_str()).collect()
}

fn skipped(reason: &str) -> (ScheduleOutcome, Option<String>) {
    (ScheduleOutcome::Skipped, Some(reason.to_owned()))
}

#[test]
fn a_due_run_starts_the_command_once_in_a_new_space_with_the_users_layout() {
    let d = Dirs::new();
    // A project file adds no layout, no command and no schedule.
    let project = Path::new(&d.repo()).join(".poimen");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("config.json"),
        r#"{"layout":{"panes":[{"name":"theirs","command":"true"}]},"setup":"true",
            "schedules":[{"cron":"* * * * *","timezone":"UTC","dir":"/","mode":"job"}]}"#,
    )
    .unwrap();
    d.config(
        json!([d.schedule("0 9 * * *")]),
        json!({"spaces": {"layout": {"panes": [{"name": "mine"}]}}}),
    );
    let env = d.start();
    env.tick(BEFORE);
    let info = env.only();
    assert_eq!(info.next_ms, Some(NINE));
    assert!(info.runs.is_empty() && info.last.is_none());
    assert!(env.spaces().spaces.is_empty(), "nothing is due yet");

    env.tick(NINE);
    let info = env.finished(1);
    assert_eq!(info.runs[0].due_ms, NINE);
    assert_eq!(info.runs[0].exit_code, Some(0));
    assert_eq!(info.runs[0].outcome, ScheduleOutcome::Ok);
    assert_eq!(info.last.unwrap().outcome, ScheduleOutcome::Ok);
    assert_eq!(info.next_ms, Some(NINE + DAY));
    assert_eq!(d.runs_log(), [format!("run in {}", d.repo())]);
    let spaces = env.spaces();
    assert_eq!(spaces.spaces.len(), 1);
    assert_eq!(spaces.spaces[0].cwd, d.repo());
    assert!(!names(&spaces).contains(&"theirs"), "{:?}", names(&spaces));
    assert!(!names(&spaces).contains(&"setup"), "{:?}", names(&spaces));
    assert!(
        env.client
            .list_terminals(None)
            .unwrap()
            .iter()
            .any(|t| t.name == "mine")
    );

    // Handled: neither the same time again nor a clock that went backwards starts it.
    env.tick(NINE);
    env.tick(NINE - MINUTE);
    env.tick(NINE);
    assert_eq!(env.only().runs.len(), 1);

    // The next day runs in the same space.
    env.tick(NINE + DAY);
    env.finished(2);
    assert_eq!(env.spaces().spaces.len(), 1);
}

#[test]
fn a_run_missed_while_the_server_was_down_runs_once_within_the_grace_time() {
    let d = Dirs::new();
    d.config(json!([d.schedule("0 9 * * *")]), json!({}));
    d.start().tick(BEFORE);

    // Three due times passed; only the newest, exactly 12 hours old, runs.
    d.at(NINE + 2 * DAY + 12 * HOUR);
    let env = d.start();
    let info = env.finished(1);
    assert_eq!(info.runs[0].due_ms, NINE + 2 * DAY);
    env.tick(NINE + 2 * DAY + 12 * HOUR + MINUTE);
    assert_eq!(env.only().runs.len(), 1);
    assert_eq!(d.runs_log().len(), 1);
}

#[test]
fn a_run_missed_by_more_than_the_grace_time_is_skipped_and_recorded() {
    let d = Dirs::new();
    d.config(json!([d.schedule("0 9 * * *")]), json!({}));
    d.start().tick(BEFORE);

    d.at(NINE + 12 * HOUR + MINUTE);
    let env = d.start();
    env.tick(NINE + 12 * HOUR + MINUTE);
    assert_eq!(env.last_reason(), skipped("missed by more than 12 hours"));
    assert!(env.only().runs.is_empty());
    assert!(d.runs_log().is_empty());
}

#[test]
fn a_new_schedule_has_nothing_to_catch_up() {
    let d = Dirs::new();
    d.config(json!([d.schedule("0 9 * * *")]), json!({}));
    d.at(NINE + MINUTE);
    let env = d.start();
    env.tick(NINE + MINUTE);
    let info = env.only();
    assert!(info.runs.is_empty() && info.last.is_none());
}

#[test]
fn a_schedule_does_not_start_while_its_last_run_is_running_also_after_a_restart() {
    let d = Dirs::new();
    std::fs::write(d.root.join("hold"), "").unwrap();
    std::fs::write(d.root.join("code"), "3").unwrap();
    d.config(json!([d.schedule("0 * * * *")]), json!({}));
    let env = d.start();
    env.tick(BEFORE);
    env.tick(NINE);
    env.until(|s| s.runs.len() == 1);
    env.tick(NINE + HOUR);
    assert_eq!(env.last_reason(), skipped("the last run is still running"));

    // The terminal outlives the server; the next one knows the run still runs.
    env.client
        .request("server.shutdown", json!({"keep_terminals": true}))
        .unwrap();
    env.stopped.recv_timeout(LONG).unwrap();
    drop(env);
    d.at(NINE + 2 * HOUR);
    let env = d.start();
    env.tick(NINE + 2 * HOUR);
    assert_eq!(env.only().runs.len(), 1);
    assert_eq!(env.last_reason(), skipped("the last run is still running"));

    // An edited cron expression does not get around the running run.
    d.config(json!([d.schedule("30 * * * *")]), json!({}));
    env.tick(NINE + 2 * HOUR);
    env.tick(NINE + 2 * HOUR + 30 * MINUTE);
    assert_eq!(env.last_reason(), skipped("the last run is still running"));
    assert!(env.only().runs.is_empty());

    d.config(json!([d.schedule("0 * * * *")]), json!({}));
    env.tick(NINE + 2 * HOUR + 31 * MINUTE);
    std::fs::remove_file(d.root.join("hold")).unwrap();
    // The run of before the restart ends, and the next due time starts the second one.
    assert_eq!(env.finished(1).runs[0].exit_code, Some(3));
    env.tick(NINE + 3 * HOUR);
    env.finished(2);
    assert_eq!(d.runs_log().len(), 2);
}

#[test]
fn the_exit_code_is_the_result_and_closing_the_terminal_ends_the_run() {
    let d = Dirs::new();
    std::fs::write(d.root.join("code"), "3").unwrap();
    d.config(json!([d.schedule("0 * * * *")]), json!({}));
    let env = d.start();
    env.tick(BEFORE);
    env.tick(NINE);
    let info = env.finished(1);
    assert_eq!(info.runs[0].exit_code, Some(3));
    assert_eq!(info.runs[0].outcome, ScheduleOutcome::Failed);
    assert_eq!(
        env.last_reason(),
        (ScheduleOutcome::Failed, Some("exit code 3".into()))
    );

    std::fs::write(d.root.join("hold"), "").unwrap();
    env.tick(NINE + HOUR);
    let info = env.until(|s| s.runs.len() == 2);
    env.client
        .request(
            "terminal.close",
            json!({"terminal_id": info.runs[1].terminal_id}),
        )
        .unwrap();
    env.tick(NINE + HOUR + MINUTE);
    let info = env.finished(2);
    assert_eq!(info.runs[1].exit_code, None);
    assert_eq!(info.runs[1].outcome, ScheduleOutcome::Failed);
    std::fs::remove_file(d.root.join("hold")).unwrap();
}

#[test]
fn a_run_starts_only_when_the_precheck_exits_with_0() {
    let d = Dirs::new();
    let mut schedule = d.schedule("0 * * * *");
    // Runs in `dir`: the file is looked for there.
    schedule["precheck"] = json!(["test", "-f", "ready"]);
    d.config(json!([schedule]), json!({}));
    let env = d.start();
    env.tick(BEFORE);
    env.tick(NINE);
    let info = env.until(|s| s.last.is_some());
    let last = info.last.unwrap();
    assert_eq!(last.outcome, ScheduleOutcome::Skipped);
    assert!(
        last.reason
            .as_deref()
            .unwrap()
            .starts_with("precheck failed"),
        "{last:?}"
    );
    assert!(info.runs.is_empty());
    assert!(env.spaces().spaces.is_empty());

    std::fs::write(Path::new(&d.repo()).join("ready"), "").unwrap();
    env.tick(NINE + HOUR);
    env.finished(1);
    assert_eq!(d.runs_log().len(), 1);
}

#[test]
fn nothing_starts_while_paused_or_switched_off_and_nothing_is_caught_up_after() {
    let d = Dirs::new();
    d.config(
        json!([d.schedule("0 * * * *")]),
        json!({"triggers": {"paused": true}}),
    );
    let env = d.start();
    env.tick(BEFORE);
    assert!(env.list().paused);
    assert_eq!(env.only().next_ms, None);
    env.tick(NINE);
    assert_eq!(env.last_reason(), skipped("paused"));

    d.config(json!([d.schedule("0 * * * *")]), json!({}));
    env.tick(NINE + MINUTE);
    assert!(env.only().runs.is_empty());

    let mut off = d.schedule("0 * * * *");
    off["enabled"] = json!(false);
    d.config(json!([off]), json!({}));
    env.tick(NINE + HOUR);
    let info = env.only();
    assert!(!info.enabled);
    assert_eq!(info.next_ms, None);

    d.config(json!([d.schedule("0 * * * *")]), json!({}));
    env.tick(NINE + HOUR + MINUTE);
    assert!(env.only().runs.is_empty());
    env.tick(NINE + 2 * HOUR);
    env.finished(1);
}

#[test]
fn the_runs_per_day_are_limited() {
    let d = Dirs::new();
    d.config(
        json!([d.schedule("0 * * * *")]),
        json!({"triggers": {"maxRunsPerDay": 2}}),
    );
    let env = d.start();
    env.tick(BEFORE);
    env.tick(NINE);
    env.finished(1);
    env.tick(NINE + HOUR);
    env.finished(2);
    env.tick(NINE + 2 * HOUR);
    assert_eq!(
        env.last_reason(),
        skipped("the limit of runs per day is reached")
    );
    assert_eq!(env.only().runs.len(), 2);
    // 24 hours after the first start there is room again.
    env.tick(NINE + DAY);
    env.finished(3);
}
