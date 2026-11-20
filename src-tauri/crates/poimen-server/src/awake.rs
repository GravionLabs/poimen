//! Keeping the machine awake while agents work (#987): with `agents.keepAwake` on, the server
//! holds a sleep inhibitor while a local agent is `working` (and not silent) or a run is
//! running, releases it a short while after the last one stopped, and when the setting goes off.
//! The holder is the server, so it works with the app closed.
//!
//! The inhibitor is a helper process: `caffeinate` on macOS, `systemd-inhibit` on Linux, a
//! PowerShell that sets the execution state on Windows. It runs a command that waits on its
//! standard input, so the hold ends when the server drops the pipe (a release) and when the
//! server dies (the pipe closes): nothing is left behind.

use std::io;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Reducer};
use poimen_core::gui_path;
use poimen_core::keep_awake::keep_awake;
use poimen_core::settings::config_path;
use poimen_protocol::{AgentStatus, Event, TerminalStatus};

use std::sync::mpsc::Sender;

use crate::{Server, Shared, lock, now_millis};

/// How long the machine stays held after the last agent stopped working.
pub(crate) const RELEASE_AFTER: Duration = Duration::from_secs(60);
/// How often the run files are read again.
const RUNS_CHECK: Duration = Duration::from_secs(30);
/// A run file untouched for longer is not a running run (the reducer abandons it after an hour).
const RUN_FRESH: Duration = Duration::from_secs(3600);

/// What the server holds and what it decided last.
#[derive(Default)]
pub(crate) struct Awake {
    holder: Option<Child>,
    /// Since when nothing works, while held (epoch milliseconds).
    idle_since: Option<u64>,
    /// The spawn failure was logged.
    failed: bool,
    /// The program and arguments that hold, instead of the platform's (tests).
    command: Option<Vec<String>>,
    /// The last look at the run files and when it was taken.
    runs: Option<(Instant, bool)>,
}

impl Awake {
    pub(crate) fn held(&self) -> bool {
        self.holder.is_some()
    }

    fn release(&mut self) {
        if let Some(mut child) = self.holder.take() {
            // Closing the pipe ends the helper's command; the kill is for a helper that does not
            // wait on it.
            drop(child.stdin.take());
            let _ = child.kill();
            let _ = child.wait();
        }
        self.idle_since = None;
    }

    pub(crate) fn set_command(&mut self, command: Vec<String>) {
        self.command = Some(command);
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.release();
    }
}

/// The helper's command line for this platform; it waits until its standard input closes.
fn platform_command() -> Vec<String> {
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect();
    if cfg!(target_os = "macos") {
        words(&["caffeinate", "-i", "sh", "-c", "cat >/dev/null"])
    } else if cfg!(windows) {
        words(&[
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -Name P -Namespace W -MemberDefinition '[DllImport(\"kernel32.dll\")] \
             public static extern uint SetThreadExecutionState(uint f);'; \
             [void][W.P]::SetThreadExecutionState(0x80000001); \
             [void][Console]::In.ReadToEnd()",
        ])
    } else {
        words(&[
            "systemd-inhibit",
            "--what=sleep:idle",
            "--who=poimen",
            "--why=An agent is working",
            "--mode=block",
            "sh",
            "-c",
            "cat >/dev/null",
        ])
    }
}

fn spawn(command: &[String]) -> io::Result<Child> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::other("empty command"))?;
    let mut c = gui_path::command(program);
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c.spawn()
}

impl Shared {
    /// Whether the user config turns `agents.keepAwake` on (off without a config or when invalid).
    fn keep_awake_wanted(&self) -> bool {
        self.home
            .as_deref()
            .and_then(|home| std::fs::read_to_string(config_path(home)).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|root| keep_awake(&root).unwrap_or(false))
    }

    /// An agent of this machine works: `working`, not silent (an agent that reports nothing for a
    /// long time must not keep the machine awake for good).
    fn agent_working(&self) -> bool {
        lock(&self.inner).terminals.iter().any(|t| {
            t.info.status != TerminalStatus::Exited
                && t.info
                    .agent
                    .as_ref()
                    .is_some_and(|a| a.status == AgentStatus::Working && a.silent_since.is_none())
        })
    }

    /// A run of a followed repository is running, read from the run files at most every
    /// [`RUNS_CHECK`] (whether or not a client follows the runs).
    fn run_running(&self, awake: &mut Awake, now: Instant) -> bool {
        if let Some((at, running)) = awake.runs
            && now.duration_since(at) < RUNS_CHECK
        {
            return running;
        }
        let dirs: Vec<String> = {
            let inner = lock(&self.inner);
            let mut dirs: Vec<String> = inner.layout.spaces.iter().map(|s| s.cwd.clone()).collect();
            for t in &inner.terminals {
                if t.info.status == TerminalStatus::Running && !dirs.contains(&t.info.cwd) {
                    dirs.push(t.info.cwd.clone());
                }
            }
            dirs
        };
        let repos = lock(&self.run_repos).resolve(&dirs, now, poimen_core::git::roots);
        let running = repos
            .iter()
            .any(|(_, dir)| running_in(dir, std::time::SystemTime::now()));
        awake.runs = Some((now, running));
        running
    }

    /// One pass at `now_ms`: takes, keeps or releases the hold.
    pub(crate) fn awake_sweep(&self, now_ms: u64) {
        let wanted = self.keep_awake_wanted();
        let mut awake = lock(&self.awake);
        let was = awake.held();
        if !wanted {
            awake.release();
        } else {
            let busy = self.agent_working() || self.run_running(&mut awake, Instant::now());
            if busy {
                awake.idle_since = None;
                // A helper that ended by itself is taken again.
                if let Some(child) = awake.holder.as_mut()
                    && matches!(child.try_wait(), Ok(Some(_)))
                {
                    awake.holder = None;
                }
                if awake.holder.is_none() {
                    let command = awake.command.clone().unwrap_or_else(platform_command);
                    match spawn(&command) {
                        Ok(child) => {
                            awake.holder = Some(child);
                            awake.failed = false;
                        }
                        Err(e) if !awake.failed => {
                            awake.failed = true;
                            self.log("awake", &format!("cannot hold the machine awake: {e}"));
                        }
                        Err(_) => {}
                    }
                }
            } else if awake.held() {
                let since = *awake.idle_since.get_or_insert(now_ms);
                if now_ms.saturating_sub(since) >= RELEASE_AFTER.as_millis() as u64 {
                    awake.release();
                }
            }
        }
        let now = awake.held();
        drop(awake);
        if now != was {
            self.publish(Event::ServerAwakeChanged { awake: now });
        }
    }

    /// Whether the machine is held now.
    pub(crate) fn awake_held(&self) -> bool {
        lock(&self.awake).held()
    }
}

/// Whether a run file in `dir` shows a running run: fresh, readable, no `run.finished`, its
/// writer alive.
fn running_in(dir: &std::path::Path, now: std::time::SystemTime) -> bool {
    let Some(entries) = std::fs::read_dir(dir).ok() else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if !agent_events::is_run_file(&path) {
            return false;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age < RUN_FRESH);
        if !meta.is_file() || meta.len() > MAX_EVENTS_BYTES || !fresh {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return false;
        };
        let mut reducer = Reducer::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            reducer.push_line(line);
        }
        let secs = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        reducer
            .snapshot(secs, &agent_events::pid_alive)
            .is_some_and(|run| run.state.status == "running")
    })
}

impl Server {
    /// One pass of the keep-awake check at the given time in milliseconds (#987); `serve` runs it
    /// on a timer, tests call this with a clock of their own. Take it before `serve`.
    pub fn awake_sweeper(&self) -> impl Fn(u64) + Send + Sync + 'static {
        let shared = Arc::clone(&self.shared);
        move |now_ms| shared.awake_sweep(now_ms)
    }

    /// Holds the machine awake with this command line (it must wait on its standard input)
    /// instead of the platform's; for tests.
    pub fn set_awake_command(&self, command: Vec<String>) {
        lock(&self.shared.awake).set_command(command);
    }
}

/// Runs [`Shared::awake_sweep`] on a timer for the life of the server.
pub(crate) fn spawn_sweeper(shared: &Arc<Shared>) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        loop {
            thread::park_timeout(crate::hibernate::SWEEP);
            shared.awake_sweep(now_millis());
        }
    });
}

impl Shared {
    /// Tells a new subscription whether the machine is held, so a client that missed a change
    /// while its stream was down learns the current state.
    /// Called under the `subscribers` lock, so no change falls between it and the listing. Only
    /// a declared client (one that knows the event) gets it.
    pub(crate) fn announce_awake(&self, tx: &Sender<Event>, declared: bool) {
        if !declared {
            return;
        }
        let _ = tx.send(Event::ServerAwakeChanged {
            awake: self.awake_held(),
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    #[allow(clippy::disallowed_methods)]
    fn utc_now() -> String {
        let out = Command::new("date")
            .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn event(seq: u32, kind: &str) -> String {
        format!(
            r#"{{"v":1,"seq":{seq},"ts":"{}","run":"r1","type":"{kind}","tone":"info"}}"#,
            utc_now()
        ) + "\n"
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-awake-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_run_without_run_finished_is_running_and_a_finished_one_is_not() {
        let dir = dir("runs");
        assert!(!running_in(&dir, std::time::SystemTime::now()));
        let file = dir.join("r1.jsonl");
        std::fs::write(&file, event(1, "run.started")).unwrap();
        assert!(running_in(&dir, std::time::SystemTime::now()));
        std::fs::write(&file, event(1, "run.started") + &event(2, "run.finished")).unwrap();
        assert!(!running_in(&dir, std::time::SystemTime::now()));
        // Not a run file name: ignored.
        std::fs::write(dir.join("notes.txt"), event(1, "run.started")).unwrap();
        assert!(!running_in(&dir, std::time::SystemTime::now()));
        // A running run in a file untouched for hours is not running.
        std::fs::write(&file, event(1, "run.started")).unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(2 * 3600);
        assert!(!running_in(&dir, later));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
