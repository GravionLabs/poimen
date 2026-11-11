//! The real `poimen-server hook copilot|claude` against a real server on a temp socket (#485):
//! run inside terminals of several spaces and of a space restored after a restart, with a stdin
//! that stays open, and with a server that cannot take the report (the hook log).
//!
//! Unix-only: the terminals run `sh`. The Windows command lines of the hooks are covered by the
//! unit tests of `claude.rs` and `copilot.rs`.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AgentInfo, AgentStatus, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};

const HOOK: &str = env!("CARGO_BIN_EXE_poimen-server");

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-hooks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn config(&self, run: &str) -> Config {
        Config {
            socket: self.dir.join(run).join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        }
    }

    fn start(&self, run: &str) -> Client {
        let config = self.config(run);
        let server = Server::bind(config.clone()).unwrap();
        thread::spawn(move || server.serve());
        Client::new(&config.socket)
    }

    fn work(&self) -> String {
        self.dir
            .join("work")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("state").join("hooks.log")).unwrap_or_default()
    }

    /// Runs the hook binary directly with `payload` on stdin; `open_for` keeps stdin open that
    /// long after the payload. The state directory (and so the hook log) is the test's.
    fn hook(
        &self,
        args: &[&str],
        terminal: Option<&str>,
        socket: &Path,
        payload: &str,
        open_for: Duration,
    ) -> (std::process::ExitStatus, String, Duration) {
        let mut cmd = Command::new(HOOK);
        cmd.args(args)
            .env("POIMEN_SOCKET", socket)
            .env("POIMEN_STATE_DIR", self.dir.join("state"))
            .env_remove("POIMEN_TERMINAL_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(t) = terminal {
            cmd.env("POIMEN_TERMINAL_ID", t);
        }
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        // The hook may exit before it has read its stdin (EPIPE); it reads only the first value.
        let _ = stdin.write_all(payload.as_bytes());
        let writer = thread::spawn(move || {
            thread::sleep(open_for);
            drop(stdin);
        });
        let out = child.wait_with_output().unwrap();
        let took = started.elapsed();
        writer.join().unwrap();
        (
            out.status,
            String::from_utf8_lossy(&out.stdout).into(),
            took,
        )
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sh(client: &Client, space_id: &str) -> String {
    client
        .create_terminal(&TerminalCreate {
            space_id: space_id.into(),
            command: Some(vec!["sh".into()]),
            name: Some("agent".into()),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn agent(client: &Client, id: &str) -> Option<AgentInfo> {
    client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .and_then(|t| t.agent)
}

fn wait_for(client: &Client, id: &str, status: AgentStatus) -> AgentInfo {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(info) = agent(client, id).filter(|a| a.status == status) {
            return info;
        }
        assert!(
            Instant::now() < deadline,
            "terminal {id} never reported {status:?}: {:?}",
            agent(client, id)
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Types the Copilot CLI hook for `event` into the terminal, as Copilot CLI runs it there.
fn type_copilot_hook(client: &Client, id: &str, event: &str, session: &str) {
    let line =
        format!("printf '{{\"sessionId\":\"{session}\"}}' | '{HOOK}' hook copilot {event}\n");
    client.input(id, &line).unwrap();
}

#[test]
fn a_copilot_hook_reports_from_every_terminal_of_every_space() {
    let env = Env::new("spaces");
    let client = env.start("run");
    let mut terminals = Vec::new();
    for name in ["api", "web", "docs"] {
        let space = client.create_space(name, &env.work()).unwrap();
        // The layout's panes: agent, tests, shell.
        terminals.extend((0..3).map(|_| sh(&client, &space.id)));
    }
    for (n, id) in terminals.iter().enumerate() {
        type_copilot_hook(&client, id, "sessionStart", &format!("s{n}"));
    }
    for (n, id) in terminals.iter().enumerate() {
        let info = wait_for(&client, id, AgentStatus::Idle);
        assert_eq!(info.agent.as_deref(), Some("copilot"));
        assert_eq!(info.session_id, Some(format!("s{n}")));
        type_copilot_hook(&client, id, "agentStop", &format!("s{n}"));
    }
    for id in &terminals {
        wait_for(&client, id, AgentStatus::Done);
    }
    assert_eq!(env.log(), "", "nothing failed");
}

#[test]
fn a_terminal_of_a_space_restored_after_a_restart_reports() {
    let env = Env::new("restart");
    let first = env.start("run");
    let space = first.create_space("api", &env.work()).unwrap();
    sh(&first, &space.id);
    // A new server (another socket, same state file), as after a restart: the space is back,
    // its terminals are not, and the app opens new ones in it.
    let second = env.start("run2");
    assert!(second.list_terminals(None).unwrap().is_empty());
    let id = sh(&second, &space.id);
    type_copilot_hook(&second, &id, "userPromptSubmitted", "s1");
    let info = wait_for(&second, &id, AgentStatus::Working);
    assert_eq!(info.agent.as_deref(), Some("copilot"));
}

#[test]
fn a_hook_whose_stdin_stays_open_reports_without_waiting_for_its_end() {
    let env = Env::new("open-stdin");
    let client = env.start("run");
    let space = client.create_space("api", &env.work()).unwrap();
    let id = sh(&client, &space.id);
    let socket = env.config("run").socket;
    let (status, stdout, took) = env.hook(
        &["hook", "copilot", "agentStop"],
        Some(&id),
        &socket,
        r#"{"sessionId":"s1","stopReason":"end_turn"}"#,
        Duration::from_secs(4),
    );
    assert!(status.success());
    assert_eq!(stdout, "");
    assert!(
        took < Duration::from_secs(3),
        "the hook waited for EOF: {took:?}"
    );
    assert_eq!(
        agent(&client, &id).map(|a| a.status),
        Some(AgentStatus::Done)
    );
    let (status, ..) = env.hook(
        &["hook", "claude"],
        Some(&id),
        &socket,
        "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"c1\"}\n",
        Duration::from_secs(4),
    );
    assert!(status.success());
    assert_eq!(
        agent(&client, &id).map(|a| (a.status, a.agent)),
        Some((AgentStatus::Working, Some("claude".into())))
    );
}

#[test]
fn a_hook_that_cannot_report_leaves_a_line_in_the_hook_log() {
    let env = Env::new("log");
    let client = env.start("run");
    let socket = env.config("run").socket;
    let payload = r#"{"sessionId":"s1"}"#;
    let quick = Duration::ZERO;

    // A terminal this server does not know (one of a server that was restarted).
    let (status, stdout, _) = env.hook(
        &["hook", "copilot", "agentStop"],
        Some("t99"),
        &socket,
        payload,
        quick,
    );
    assert!(status.success());
    assert_eq!(stdout, "", "nothing reaches the agent");
    let log = env.log();
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("hook copilot agentStop"), "{log}");
    assert!(log.contains("terminal t99"), "{log}");
    assert!(
        log.contains("does not know the terminal (no terminal t99)"),
        "{log}"
    );

    // No server at the socket.
    let nowhere = env.dir.join("gone").join("server.sock");
    let stop = r#"{"hook_event_name":"Stop","session_id":"c1"}"#;
    env.hook(&["hook", "claude"], Some("t1"), &nowhere, stop, quick);
    let log = env.log();
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(
        log.lines().last().unwrap().contains("not reachable"),
        "{log}"
    );

    // A poimen terminal (the socket is set) without its id.
    env.hook(
        &["hook", "copilot", "agentStop"],
        None,
        &socket,
        payload,
        quick,
    );
    let log = env.log();
    assert_eq!(log.lines().count(), 3, "{log}");
    assert!(log.contains("POIMEN_TERMINAL_ID"), "{log}");

    // Not a hook of ours to report: no line (an event poimen ignores, garbage stays a line).
    let space = client.create_space("api", &env.work()).unwrap();
    let id = sh(&client, &space.id);
    env.hook(
        &["hook", "copilot", "preToolUse"],
        Some(&id),
        &socket,
        payload,
        quick,
    );
    assert_eq!(env.log().lines().count(), 3);
    env.hook(
        &["hook", "copilot", "agentStop"],
        Some(&id),
        &socket,
        "not json",
        quick,
    );
    let log = env.log();
    assert_eq!(log.lines().count(), 4, "{log}");
    assert!(log.lines().last().unwrap().contains("JSON"), "{log}");
}
