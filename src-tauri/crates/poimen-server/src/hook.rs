//! Running an agent hook (`poimen-server hook claude|copilot`): read the payload, report it, and
//! leave a trace in the hook log when a report does not get through.
//!
//! A hook never fails or holds up its agent, so a failure cannot surface there: Claude Code and
//! Copilot CLI drop the output of a hook that exits 0. Every report that does not reach the
//! terminal (no terminal id in a poimen terminal, a payload that is not JSON or never arrives, no
//! server, an unknown terminal, a stale report, no answer in time) is one line in `hooks.log`
//! next to `server.json` (see [`crate::state_file_path`]) and on stderr. A run outside a poimen
//! terminal, in a terminal of the other channel (stable or rc), and an event poimen does not map,
//! are not failures and leave no line.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use poimen_client::{Client, ClientError};
use poimen_protocol::{ENV_SOCKET, ENV_TERMINAL_ID, ErrorCode, TerminalReportAgent};
use serde_json::Value;

/// How long a hook may take in all, well inside the agents' hook timeouts.
pub const HOOK_BUDGET: Duration = Duration::from_millis(1500);
/// Most bytes read from stdin.
const MAX_PAYLOAD: u64 = 1 << 20;
/// Size at which `hooks.log` moves to `hooks.log.1` (replacing an older one).
const MAX_LOG: u64 = 256 * 1024;
const LOG_NAME: &str = "hooks.log";

/// What a hook needs from its environment.
#[derive(Debug, Clone)]
pub struct HookEnv {
    /// `$POIMEN_TERMINAL_ID`.
    pub terminal_id: Option<String>,
    /// Whether `$POIMEN_SOCKET` is set, which every poimen terminal has: the hook runs inside one.
    pub in_poimen: bool,
    /// Whether `$POIMEN_SOCKET` is the default socket of the other channel (stable or rc,
    /// [`poimen_protocol::foreign_socket`]): the terminal belongs to the other installation,
    /// whose own hook reports for it.
    pub foreign: bool,
    pub socket: PathBuf,
    /// The hook log; `None` keeps no log.
    pub log: Option<PathBuf>,
}

impl HookEnv {
    /// The environment of this process: terminal id, socket and the log next to `server.json`.
    pub fn from_env() -> Self {
        Self {
            terminal_id: std::env::var(ENV_TERMINAL_ID).ok(),
            in_poimen: std::env::var_os(ENV_SOCKET).is_some_and(|s| !s.is_empty()),
            foreign: poimen_protocol::foreign_socket(),
            socket: poimen_protocol::terminal_socket_path(),
            log: Some(log_path(&crate::state_file_path())),
        }
    }
}

/// `hooks.log` next to the state file.
pub fn log_path(state_file: &Path) -> PathBuf {
    state_file.with_file_name(LOG_NAME)
}

/// What became of one hook run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not in a poimen terminal: nothing to do.
    Outside,
    /// The event is not one poimen maps (a subagent's, an unregistered one).
    Ignored,
    /// The server took the report.
    Reported,
    /// The report did not get through; the text says why.
    Failed(String),
}

/// Runs a hook: reads one JSON payload from `stdin` (without waiting for its end), turns it into
/// a report with `build` and sends it, all within [`HOOK_BUDGET`]. `what` names the hook in the
/// log (`hook copilot agentStop`). A failure is written to the log and stderr; nothing else is
/// ever printed.
pub fn run(
    stdin: impl Read + Send + 'static,
    env: &HookEnv,
    what: &str,
    build: impl FnOnce(&str, u64, &Value) -> Option<TerminalReportAgent> + Send + 'static,
) -> Outcome {
    let outcome = report(stdin, env, build);
    if let Outcome::Failed(why) = &outcome {
        let line = log_line(now_secs(), what, why, env);
        eprintln!("{line}");
        if let Some(log) = &env.log {
            append(log, &line);
        }
    }
    outcome
}

fn report(
    stdin: impl Read + Send + 'static,
    env: &HookEnv,
    build: impl FnOnce(&str, u64, &Value) -> Option<TerminalReportAgent> + Send + 'static,
) -> Outcome {
    if env.foreign {
        return Outcome::Outside;
    }
    let Some(terminal_id) = env.terminal_id.clone().filter(|t| !t.is_empty()) else {
        return if env.in_poimen {
            Outcome::Failed(format!(
                "{ENV_TERMINAL_ID} is not set although {ENV_SOCKET} is, so the report has no \
                 terminal"
            ))
        } else {
            Outcome::Outside
        };
    };
    let socket = env.socket.clone();
    let read = Arc::new(AtomicBool::new(false));
    let (done, wait) = mpsc::channel();
    {
        let read = Arc::clone(&read);
        thread::spawn(move || {
            let payload = read_payload(stdin);
            read.store(true, Ordering::SeqCst);
            let outcome = match payload {
                Err(why) => Outcome::Failed(why),
                Ok(payload) => match build(&terminal_id, crate::claude::now_nanos(), &payload) {
                    None => Outcome::Ignored,
                    Some(request) => send(&socket, request),
                },
            };
            let _ = done.send(outcome);
        });
    }
    match wait.recv_timeout(HOOK_BUDGET) {
        Ok(outcome) => outcome,
        Err(_) if !read.load(Ordering::SeqCst) => Outcome::Failed(format!(
            "no complete JSON payload on stdin within {} ms",
            HOOK_BUDGET.as_millis()
        )),
        Err(_) => Outcome::Failed(format!(
            "the server did not answer within {} ms",
            HOOK_BUDGET.as_millis()
        )),
    }
}

/// The first JSON value on `stdin`. It does not wait for the end of stdin: an agent (or a shell
/// between it and the hook) that keeps stdin open after the payload must not cost the report.
fn read_payload(stdin: impl Read) -> Result<Value, String> {
    let mut values = serde_json::Deserializer::from_reader(stdin.take(MAX_PAYLOAD)).into_iter();
    match values.next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(e)) => Err(format!("the payload on stdin is not JSON: {e}")),
        None => Err("no payload on stdin".into()),
    }
}

fn send(socket: &Path, request: TerminalReportAgent) -> Outcome {
    let terminal_id = request.terminal_id.clone();
    let params = serde_json::to_value(request).unwrap_or(Value::Null);
    match Client::new(socket).request("terminal.report_agent", params) {
        Ok(result) if result.get("accepted") == Some(&Value::Bool(false)) => Outcome::Failed(
            "the server dropped the report as stale (its seq is not above the last one)".into(),
        ),
        Ok(_) => Outcome::Reported,
        Err(ClientError::Server {
            code: ErrorCode::NotFound,
            message,
        }) => Outcome::Failed(format!(
            "the server does not know the terminal ({message}); a terminal of an earlier server \
             run reports nowhere, start the agent again in a new terminal"
        )),
        Err(e) => Outcome::Failed(format!("{e} (terminal {terminal_id})")),
    }
}

/// One line of the hook log: UTC time, version, hook, reason, terminal and socket.
fn log_line(secs: u64, what: &str, why: &str, env: &HookEnv) -> String {
    format!(
        "{} poimen-server {} {what}: {why} (terminal {}, socket {})",
        utc(secs),
        env!("CARGO_PKG_VERSION"),
        env.terminal_id
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or("-"),
        env.socket.display()
    )
}

/// Appends `line` to the log, moving a full log to `<name>.1` first. Never fails the hook.
fn append(log: &Path, line: &str) {
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(log).is_ok_and(|m| m.len() >= MAX_LOG) {
        let mut old = log.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(log, PathBuf::from(old));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(file, "{line}");
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `YYYY-MM-DDTHH:MM:SSZ` of seconds since the epoch (proleptic Gregorian, days to civil date).
fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_protocol::AgentStatus;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-hook-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env(terminal: Option<&str>, in_poimen: bool, log: Option<PathBuf>) -> HookEnv {
        HookEnv {
            terminal_id: terminal.map(str::to_owned),
            in_poimen,
            foreign: false,
            socket: PathBuf::from("/nonexistent/poimen/s.sock"),
            log,
        }
    }

    #[test]
    fn a_terminal_of_the_other_channel_is_left_to_its_own_hook() {
        let dir = temp("foreign");
        let log = dir.join(LOG_NAME);
        let foreign = HookEnv {
            foreign: true,
            ..env(Some("t1"), true, Some(log.clone()))
        };
        // Neither sent (the server is unreachable, which would fail) nor logged.
        assert_eq!(
            run(payload("{}"), &foreign, "hook claude", build),
            Outcome::Outside
        );
        assert!(!log.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn build(t: &str, seq: u64, _: &Value) -> Option<TerminalReportAgent> {
        Some(TerminalReportAgent {
            terminal_id: t.into(),
            seq,
            status: AgentStatus::Done,
            agent: Some("copilot".into()),
            session_id: None,
            message: None,
        })
    }

    fn payload(text: &str) -> std::io::Cursor<Vec<u8>> {
        std::io::Cursor::new(text.as_bytes().to_vec())
    }

    #[test]
    fn outside_a_poimen_terminal_nothing_happens_and_nothing_is_logged() {
        let dir = temp("outside");
        let log = dir.join(LOG_NAME);
        let outcome = run(
            payload("{}"),
            &env(None, false, Some(log.clone())),
            "hook claude",
            build,
        );
        assert_eq!(outcome, Outcome::Outside);
        let outcome = run(
            payload("{}"),
            &env(Some(""), false, Some(log.clone())),
            "hook claude",
            build,
        );
        assert_eq!(outcome, Outcome::Outside);
        assert!(!log.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_failure_is_one_line_with_hook_terminal_and_reason() {
        let dir = temp("failures");
        let log = dir.join("state").join(LOG_NAME);
        let with_log = |t: Option<&str>| env(t, true, Some(log.clone()));
        let failed = |o: Outcome| matches!(o, Outcome::Failed(_));

        assert!(failed(run(
            payload("{}"),
            &with_log(None),
            "hook copilot agentStop",
            build
        )));
        assert!(failed(run(
            payload("nope"),
            &with_log(Some("t1")),
            "hook claude",
            build
        )));
        assert!(failed(run(
            payload(""),
            &with_log(Some("t1")),
            "hook claude",
            build
        )));
        assert!(failed(run(
            payload("{}"),
            &with_log(Some("t2")),
            "hook claude",
            build
        )));
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(lines[0].contains("hook copilot agentStop: POIMEN_TERMINAL_ID is not set"));
        assert!(lines[0].contains("(terminal -, socket /nonexistent/poimen/s.sock)"));
        assert!(lines[1].contains("not JSON") && lines[1].contains("terminal t1"));
        assert!(lines[2].contains("no payload on stdin"));
        assert!(lines[3].contains("not reachable") && lines[3].contains("terminal t2"));
        assert!(lines.iter().all(|l| l.contains(env!("CARGO_PKG_VERSION"))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unmapped_event_is_not_a_failure() {
        let dir = temp("ignored");
        let log = dir.join(LOG_NAME);
        let outcome = run(
            payload("{}"),
            &env(Some("t1"), true, Some(log.clone())),
            "hook claude",
            |_, _, _| None,
        );
        assert_eq!(outcome, Outcome::Ignored);
        assert!(!log.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A stdin that delivers `data` and then never ends.
    struct OpenStdin(std::io::Cursor<Vec<u8>>);

    impl Read for OpenStdin {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.read(buf)? {
                0 => {
                    thread::sleep(Duration::from_secs(30));
                    Ok(0)
                }
                n => Ok(n),
            }
        }
    }

    #[test]
    fn the_payload_is_read_without_waiting_for_the_end_of_stdin() {
        let started = std::time::Instant::now();
        let stdin = OpenStdin(payload(r#"{"sessionId":"s1"} "#));
        let outcome = run(
            stdin,
            &env(Some("t1"), true, None),
            "hook claude",
            |t, _, p| {
                assert_eq!((t, p), ("t1", &json!({"sessionId": "s1"})));
                None
            },
        );
        assert_eq!(outcome, Outcome::Ignored);
        assert!(started.elapsed() < HOOK_BUDGET);
        // Only a part of a payload, then nothing: the budget ends the wait.
        let outcome = run(
            OpenStdin(payload(r#"{"sessionId""#)),
            &env(Some("t1"), true, None),
            "hook claude",
            build,
        );
        assert_eq!(
            outcome,
            Outcome::Failed("no complete JSON payload on stdin within 1500 ms".into())
        );
    }

    #[test]
    fn a_full_log_moves_aside_once() {
        let dir = temp("rotate");
        let log = dir.join(LOG_NAME);
        std::fs::write(&log, vec![b'x'; usize::try_from(MAX_LOG).unwrap()]).unwrap();
        append(&log, "new");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "new\n");
        assert_eq!(
            std::fs::metadata(dir.join("hooks.log.1")).unwrap().len(),
            MAX_LOG
        );
        append(&log, "next");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "new\nnext\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn utc_times() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_794_095_999), "2026-11-07T23:59:59Z");
    }

    #[test]
    fn the_log_sits_next_to_the_state_file() {
        assert_eq!(
            log_path(Path::new("/s/poimen/server.json")),
            PathBuf::from("/s/poimen/hooks.log")
        );
    }
}
