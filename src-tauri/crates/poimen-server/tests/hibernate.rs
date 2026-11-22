//! Hibernation of a `done` agent (ADR 0014, #903) against a fake agent: a shell script that
//! reports through the real `poimen-server hook claude` and ends on the harness's exit input
//! (`/exit`). The clock is the `now_ms` the test gives the sweeper.
//!
//! Unix-only like `agent.rs`: the fake agent is a `#!/bin/sh` script.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, AttachMode, Terminal, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};

const LONG: Duration = Duration::from_secs(10);
const MINUTE_MS: u64 = 60_000;

struct Env {
    dir: PathBuf,
    sweep: Box<dyn Fn(u64) + Send + Sync>,
    client: Client,
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

impl Env {
    /// A server with a home whose config is `config` (none: no file).
    fn new(name: &str, config: Option<&str>) -> Self {
        Self::with_resume_timeout(name, config, LONG)
    }

    /// As [`Env::new`]; `{dir}` in the config is the test directory, `{agent}` the fake agent.
    fn with_resume_timeout(name: &str, config: Option<&str>, resume: Duration) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-hib-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let home = dir.join("home");
        if let Some(text) = config {
            let text = text
                .replace("{dir}", &dir.display().to_string())
                .replace("{agent}", &dir.join("fake-agent").display().to_string());
            let path = poimen_core::settings::config_path(&home);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            history_dir: dir
                .join("state")
                .join("server.json")
                .with_file_name("history"),
            scrollback: 1000,
            home: Some(home),
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let server = Server::bind(config.clone()).unwrap();
        server.set_resume_timeout(resume);
        let sweep = Box::new(server.hibernate_sweeper());
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        Self { dir, sweep, client }
    }

    /// The fake agent: reports `SessionStart`, `UserPromptSubmit` and `Stop` for each prompt line;
    /// the line `/exit` is recorded, reports `SessionEnd` and ends the agent.
    fn agent(&self) -> String {
        let hook = env!("CARGO_BIN_EXE_poimen-server");
        let log = self.dir.join("typed.log");
        let report = |event: &str| {
            format!(
                "printf '{{\"hook_event_name\":\"{event}\",\"session_id\":\"fake-1\"}}' | '{hook}' hook claude"
            )
        };
        let script = format!(
            "#!/bin/sh\necho started >> '{}'\n{}\nwhile IFS= read -r line; do\n  echo \"$line\" >> '{}'\n  if [ \"$line\" = /exit ]; then {}; exit 0; fi\n  {}\n  sleep 0.2\n  {}\ndone\n",
            self.dir.join("starts.log").display(),
            report("SessionStart"),
            log.display(),
            report("SessionEnd"),
            report("UserPromptSubmit"),
            report("Stop"),
        );
        let path = self.dir.join("fake-agent");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        path.display().to_string()
    }

    fn starts(&self) -> usize {
        std::fs::read_to_string(self.dir.join("starts.log"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    fn typed(&self) -> String {
        std::fs::read_to_string(self.dir.join("typed.log")).unwrap_or_default()
    }

    /// A terminal whose fake agent is `done` after one prompt.
    fn done_agent(&self) -> String {
        let work = self.dir.join("work").canonicalize().unwrap();
        let space = self
            .client
            .create_space("api", &work.display().to_string())
            .unwrap();
        let id = self
            .client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id;
        self.client
            .start_agent(&id, &self.agent(), &[], LONG)
            .unwrap();
        self.client.prompt_agent(&id, "work", LONG).unwrap();
        wait_until("done", || {
            self.terminal(&id)
                .agent
                .is_some_and(|a| a.status == AgentStatus::Done)
        });
        id
    }

    fn terminal(&self, id: &str) -> Terminal {
        self.client
            .list_terminals(None)
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    fn state_text(&self) -> String {
        std::fs::read_to_string(self.dir.join("state").join("server.json")).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + LONG;
    while !ok() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(20));
    }
}

const ON: &str = r#"{"agents":{"hibernateAfter":30}}"#;

#[test]
fn a_done_agent_is_hibernated_after_the_time_and_gets_one_exit_input() {
    let env = Env::new("after", Some(ON));
    let id = env.done_agent();

    (env.sweep)(now_ms() + 29 * MINUTE_MS);
    assert_eq!(
        env.terminal(&id).hibernated,
        None,
        "29 minutes is too early"
    );
    assert_eq!(env.typed(), "work\n");

    let at = now_ms() + 31 * MINUTE_MS;
    (env.sweep)(at);
    let h = env.terminal(&id).hibernated.expect("hibernated");
    assert_eq!(
        (h.agent.as_str(), h.session_id.as_str(), h.since),
        ("claude", "fake-1", at)
    );
    wait_until("the exit input", || env.typed() == "work\n/exit\n");
    // The agent ended and the hook said so: `hibernated` stays, the terminal and shell too.
    wait_until("gone", || env.terminal(&id).agent.is_none());
    assert!(env.terminal(&id).hibernated.is_some());
    // Another pass types nothing more.
    (env.sweep)(at + 60 * MINUTE_MS);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(env.typed(), "work\n/exit\n");
    // It is in the terminal list a restart reads.
    let text = env.state_text();
    assert!(text.contains("\"hibernated\""), "{text}");
    assert!(text.contains("\"session_id\": \"fake-1\""), "{text}");
}

#[test]
fn it_is_off_without_the_setting_and_with_zero() {
    for (name, config) in [
        ("nokey", Some("{}")),
        ("nofile", None),
        ("zero", Some(r#"{"agents":{"hibernateAfter":0}}"#)),
    ] {
        let env = Env::new(name, config);
        let id = env.done_agent();
        (env.sweep)(now_ms() + 1000 * MINUTE_MS);
        assert_eq!(env.terminal(&id).hibernated, None, "{name}");
        assert_eq!(env.typed(), "work\n", "{name}");
    }
}

#[test]
fn a_controller_keeps_the_agent_awake() {
    let env = Env::new("controller", Some(ON));
    let id = env.done_agent();
    let attached = env.client.attach(&id, AttachMode::Control, false).unwrap();
    (env.sweep)(now_ms() + 60 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None);
    assert_eq!(env.typed(), "work\n");
    drop(attached);
}

fn report(env: &Env, id: &str, state: &str) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_poimen-server"))
        .args([
            "report", "agent", "--state", state, "--source", "test", "--agent", "claude",
        ])
        .env("POIMEN_TERMINAL_ID", id)
        .env("POIMEN_SOCKET", env.dir.join("run").join("server.sock"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn a_working_or_blocked_agent_is_never_hibernated() {
    for state in ["working", "blocked"] {
        let env = Env::new(state, Some(ON));
        let id = env.done_agent();
        report(&env, &id, state);
        wait_until(state, || {
            env.terminal(&id)
                .agent
                .is_some_and(|a| a.status != AgentStatus::Done)
        });
        (env.sweep)(now_ms() + 600 * MINUTE_MS);
        assert_eq!(env.terminal(&id).hibernated, None, "{state}");
        assert_eq!(env.typed(), "work\n", "{state}");
    }
}

#[test]
fn a_harness_without_exit_input_or_resume_args_is_left_alone() {
    let env = Env::new(
        "noexit",
        Some(r#"{"agents":{"hibernateAfter":1},"harnesses":{"claude":{"command":["claude"]}}}"#),
    );
    let id = env.done_agent();
    (env.sweep)(now_ms() + 60 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None);
    assert_eq!(env.typed(), "work\n");
}

#[test]
fn a_report_other_than_gone_ends_the_hibernation() {
    let env = Env::new("back", Some(ON));
    let id = env.done_agent();
    // The agent keeps running (the exit input is not typed into a fake that ignores it): report
    // the user coming back with a prompt.
    (env.sweep)(now_ms() + 31 * MINUTE_MS);
    assert!(env.terminal(&id).hibernated.is_some());
    wait_until("gone", || env.terminal(&id).agent.is_none());
    report(&env, &id, "working");
    wait_until("cleared", || env.terminal(&id).hibernated.is_none());
}

#[test]
fn hibernated_survives_a_server_restart() {
    let env = Env::new("restart", Some(ON));
    let id = env.done_agent();
    (env.sweep)(now_ms() + 31 * MINUTE_MS);
    wait_until("gone", || env.terminal(&id).agent.is_none());
    let before = env.terminal(&id).hibernated.expect("hibernated");
    env.client.shutdown_keeping_terminals().unwrap();
    thread::sleep(Duration::from_millis(500));

    let config = Config {
        socket: env.dir.join("run2").join("server.sock"),
        state_file: env.dir.join("state").join("server.json"),
        history_dir: env
            .dir
            .join("state")
            .join("server.json")
            .with_file_name("history"),
        scrollback: 1000,
        home: Some(env.dir.join("home")),
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let terminals = Client::new(&config.socket).list_terminals(None).unwrap();
    let t = terminals.iter().find(|t| t.id == id).expect("adopted");
    assert_eq!(t.hibernated.as_ref(), Some(&before));
}

/// Harness `claude` that runs the fake agent again on a resume.
const RESUMING: &str = r#"{"agents":{"hibernateAfter":30},"harnesses":{"claude":{"command":["{agent}"],"exitInput":"/exit","resumeArgs":["--resume","{session}"]}}}"#;

fn hibernated(env: &Env) -> String {
    let id = env.done_agent();
    (env.sweep)(now_ms() + 31 * MINUTE_MS);
    wait_until("gone", || env.terminal(&id).agent.is_none());
    assert!(env.terminal(&id).hibernated.is_some());
    id
}

#[test]
fn a_controller_attach_resumes_the_hibernated_agent_once() {
    let env = Env::new("resume", Some(RESUMING));
    let id = hibernated(&env);
    assert_eq!(env.starts(), 1);

    let first = env.client.attach(&id, AttachMode::Control, false).unwrap();
    // A second controller in a row, while the first resume runs or has run.
    let second = env.client.attach(&id, AttachMode::Control, true).unwrap();
    wait_until("resumed", || {
        env.terminal(&id).hibernated.is_none() && env.terminal(&id).agent.is_some()
    });
    thread::sleep(Duration::from_millis(500));
    assert_eq!(env.starts(), 2, "one resume only");
    drop((first, second));
    let third = env.client.attach(&id, AttachMode::Control, false).unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(env.starts(), 2, "nothing is hibernated any more");
    drop(third);
}

#[test]
fn an_observer_attach_resumes_nothing() {
    let env = Env::new("observer", Some(RESUMING));
    let id = hibernated(&env);
    let observer = env.client.attach(&id, AttachMode::Observe, false).unwrap();
    thread::sleep(Duration::from_millis(800));
    assert!(env.terminal(&id).hibernated.is_some());
    assert_eq!(env.starts(), 1);
    drop(observer);
}

#[test]
fn a_resume_without_a_report_ends_the_hibernation_and_names_the_failure() {
    // `true` ignores its arguments and reports nothing.
    let config = r#"{"agents":{"hibernateAfter":30},"harnesses":{"claude":{"command":["true"],"exitInput":"/exit","resumeArgs":["--resume","{session}"]}}}"#;
    let env = Env::with_resume_timeout("timeout", Some(config), Duration::from_millis(800));
    let id = hibernated(&env);
    let (tx, rx) = std::sync::mpsc::channel();
    let stream = env.client.subscribe().unwrap();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    let _attached = env.client.attach(&id, AttachMode::Control, false).unwrap();
    wait_until("ended", || env.terminal(&id).hibernated.is_none());
    let mut failed = None;
    let deadline = Instant::now() + LONG;
    while failed.is_none() && Instant::now() < deadline {
        if let Ok(poimen_client::protocol::Event::TerminalResumeFailed { reason, .. }) =
            rx.recv_timeout(Duration::from_millis(200))
        {
            failed = Some(reason);
        }
    }
    assert!(failed.expect("a failure event").contains("did not report"));
    // The shell stays.
    assert_eq!(
        env.terminal(&id).status,
        poimen_client::protocol::TerminalStatus::Running
    );
}

#[test]
fn a_second_hibernation_is_resumed_again() {
    let env = Env::new("twice", Some(RESUMING));
    let id = hibernated(&env);
    let first = env.client.attach(&id, AttachMode::Control, false).unwrap();
    wait_until("resumed", || env.terminal(&id).hibernated.is_none());
    drop(first);
    env.client.prompt_agent(&id, "more", LONG).unwrap();
    wait_until("done again", || {
        env.terminal(&id)
            .agent
            .is_some_and(|a| a.status == AgentStatus::Done)
    });
    (env.sweep)(now_ms() + 100 * MINUTE_MS);
    wait_until("hibernated again", || {
        env.terminal(&id).hibernated.is_some()
    });
    wait_until("gone", || env.terminal(&id).agent.is_none());
    let _second = env.client.attach(&id, AttachMode::Control, false).unwrap();
    wait_until("resumed again", || env.terminal(&id).hibernated.is_none());
    assert_eq!(env.starts(), 3);
}

#[test]
fn input_after_done_blocks_hibernation_until_the_next_done() {
    let env = Env::new("input", Some(ON));
    let id = env.done_agent();
    // A draft typed into the agent's input box after `done` (no Enter).
    let since = env.terminal(&id).agent.unwrap().status_since.unwrap();
    wait_until("a later millisecond", || now_ms() > since);
    env.client.input(&id, "draft").unwrap();
    (env.sweep)(now_ms() + 600 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None, "input after done");
    assert_eq!(env.typed(), "work\n");

    // The next turn ends in a new `done`; the idle time counts from it.
    let before = env.terminal(&id).agent.unwrap().status_since;
    env.client.prompt_agent(&id, "more", LONG).unwrap();
    wait_until("done again", || {
        env.terminal(&id)
            .agent
            .is_some_and(|a| a.status == AgentStatus::Done && a.status_since != before)
    });
    (env.sweep)(now_ms() + 31 * MINUTE_MS);
    assert!(env.terminal(&id).hibernated.is_some(), "hibernated again");
}

/// A connection that attaches by hand, so a test can wait for the answer of a request the client
/// API does not wait for: requests of one connection are handled in order.
struct Raw {
    writer: std::os::unix::net::UnixStream,
    reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    next: u32,
}

impl Raw {
    fn attached(env: &Env, terminal_id: &str) -> Self {
        use std::os::unix::net::UnixStream;
        let socket = UnixStream::connect(env.dir.join("run").join("server.sock")).unwrap();
        let reader = std::io::BufReader::new(socket.try_clone().unwrap());
        let mut raw = Self {
            writer: socket,
            reader,
            next: 0,
        };
        let result = raw.call(
            "terminal.attach",
            serde_json::json!({"terminal_id": terminal_id, "mode": "observe"}),
        );
        assert!(result.get("error").is_none(), "{result}");
        raw
    }

    /// Sends the request and reads until its response (stream records are skipped).
    fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::{BufRead, Write};
        self.next += 1;
        let id = format!("r{}", self.next);
        let line = serde_json::json!({"id": id, "method": method, "params": params});
        writeln!(self.writer, "{line}").unwrap();
        loop {
            let mut text = String::new();
            assert!(self.reader.read_line(&mut text).unwrap() > 0, "closed");
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            if value["id"] == id.as_str() {
                return value;
            }
        }
    }

    fn set_draft(&mut self, terminal_id: &str, draft: bool) {
        let answer = self.call(
            "terminal.set_draft",
            serde_json::json!({"terminal_id": terminal_id, "draft": draft}),
        );
        assert!(answer.get("error").is_none(), "{answer}");
    }
}

#[test]
fn a_prompt_draft_blocks_hibernation_until_it_is_cleared() {
    let env = Env::new("draft", Some(ON));
    let id = env.done_agent();
    let mut raw = Raw::attached(&env, &id);
    raw.set_draft(&id, true);
    (env.sweep)(now_ms() + 600 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None, "a draft is held");
    assert_eq!(env.typed(), "work\n");

    // Cleared: the idle time counts as for any `done` terminal (it never restarted).
    raw.set_draft(&id, false);
    (env.sweep)(now_ms() + 31 * MINUTE_MS);
    assert!(env.terminal(&id).hibernated.is_some(), "hibernated");
}

#[test]
fn a_prompt_draft_needs_an_attachment_and_ends_with_the_connection_that_set_it() {
    let env = Env::new("draft-gone", Some(ON));
    let id = env.done_agent();
    // A request for another terminal is refused.
    let mut raw = Raw::attached(&env, &id);
    let wrong = raw.call(
        "terminal.set_draft",
        serde_json::json!({"terminal_id": "other", "draft": true}),
    );
    assert_eq!(wrong["error"]["code"], "invalid_request", "{wrong}");

    raw.set_draft(&id, true);
    (env.sweep)(now_ms() + 600 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None);
    drop(raw);
    wait_until("hibernated after the client left", || {
        (env.sweep)(now_ms() + 600 * MINUTE_MS);
        env.terminal(&id).hibernated.is_some()
    });
}

#[test]
fn drafts_of_two_clients_are_held_apart() {
    let env = Env::new("draft-two", Some(ON));
    let id = env.done_agent();
    let (mut a, mut b) = (Raw::attached(&env, &id), Raw::attached(&env, &id));
    a.set_draft(&id, true);
    b.set_draft(&id, true);
    a.set_draft(&id, false);
    (env.sweep)(now_ms() + 600 * MINUTE_MS);
    assert_eq!(env.terminal(&id).hibernated, None, "b still holds one");
    b.set_draft(&id, false);
    (env.sweep)(now_ms() + 600 * MINUTE_MS);
    assert!(env.terminal(&id).hibernated.is_some());
}

#[test]
fn a_repeated_done_keeps_the_idle_time() {
    let env = Env::new("repeat", Some(ON));
    let id = env.done_agent();
    let first = env.terminal(&id).agent.unwrap();
    wait_until("a later millisecond", || {
        now_ms() > first.reported_at.unwrap()
    });
    stop_hook(&env, &id);
    wait_until("the report", || {
        env.terminal(&id).agent.unwrap().reported_at != first.reported_at
    });
    assert_eq!(
        env.terminal(&id).agent.unwrap().status_since,
        first.status_since
    );
}

/// A `Stop` hook report, as the agent sends it after a turn.
fn stop_hook(env: &Env, id: &str) {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_poimen-server"))
        .args(["hook", "claude"])
        .env("POIMEN_TERMINAL_ID", id)
        .env("POIMEN_SOCKET", env.dir.join("run").join("server.sock"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"Stop","session_id":"fake-1"}"#)
        .unwrap();
    assert!(child.wait().unwrap().success());
}
