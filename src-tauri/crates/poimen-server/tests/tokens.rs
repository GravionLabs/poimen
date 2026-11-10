//! Tokens per terminal (`terminal.report_tokens`, `terminal.clear_tokens`) and the
//! `poimen-server report` CLI, against a real server on a temp socket.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{Event, TerminalCreate};
use poimen_server::{Config, Server};
use serde_json::{Value, json};

struct Env {
    dir: PathBuf,
    client: Client,
    terminal: String,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-tokens-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
        };
        let server = Server::bind(config.clone()).unwrap();
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        let work = dir.join("work").canonicalize().unwrap();
        let space = client
            .create_space("api", &work.display().to_string())
            .unwrap();
        let terminal = client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id;
        Self {
            dir,
            client,
            terminal,
        }
    }

    fn report(&self, tokens: Value, ttl: Option<u64>, source: &str) -> Result<Value, String> {
        let mut params = json!({"terminal_id": self.terminal, "tokens": tokens, "source": source});
        if let Some(ttl) = ttl {
            params["ttl_s"] = json!(ttl);
        }
        self.client
            .request("terminal.report_tokens", params)
            .map_err(|e| e.to_string())
    }

    fn clear(&self, source: &str) -> Value {
        self.client
            .request(
                "terminal.clear_tokens",
                json!({"terminal_id": self.terminal, "source": source}),
            )
            .unwrap()
    }

    fn tokens(&self) -> BTreeMap<String, String> {
        let list = self.client.list_terminals(None).unwrap();
        list.into_iter()
            .find(|t| t.id == self.terminal)
            .unwrap()
            .tokens
    }

    fn cli(&self, terminal_env: Option<&str>, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_poimen-server"));
        cmd.arg("report")
            .args(args)
            .env("POIMEN_SOCKET", self.client.socket())
            .env_remove("POIMEN_TERMINAL_ID");
        if let Some(id) = terminal_env {
            cmd.env("POIMEN_TERMINAL_ID", id);
        }
        cmd.output().unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn events(client: &Client) -> Receiver<Event> {
    let stream = client.subscribe().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });
    rx
}

fn next_tokens(rx: &Receiver<Event>) -> BTreeMap<String, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("tokens event did not arrive") {
            Event::TerminalTokensChanged { tokens, .. } => return tokens,
            _ => continue,
        }
    }
}

#[test]
fn reported_tokens_are_listed_and_null_removes() {
    let env = Env::new("set");
    env.report(
        json!({"deliver": "on", "deliver_step": "build"}),
        None,
        "deliver",
    )
    .unwrap();
    assert_eq!(
        env.tokens(),
        map(&[("deliver", "on"), ("deliver_step", "build")])
    );
    env.report(
        json!({"deliver_step": "test", "deliver": null}),
        None,
        "deliver",
    )
    .unwrap();
    assert_eq!(env.tokens(), map(&[("deliver_step", "test")]));
}

#[test]
fn a_change_emits_an_event_with_all_tokens_and_a_no_op_does_not() {
    let env = Env::new("event");
    let rx = events(&env.client);
    env.report(json!({"a": "1"}), None, "s").unwrap();
    assert_eq!(next_tokens(&rx), map(&[("a", "1")]));
    // Same value again: no event; the next real change is the next one received.
    env.report(json!({"a": "1"}), None, "s").unwrap();
    env.report(json!({"b": "2"}), None, "s").unwrap();
    assert_eq!(next_tokens(&rx), map(&[("a", "1"), ("b", "2")]));
}

#[test]
fn a_ttl_expires_the_tokens_of_that_report_only() {
    let env = Env::new("ttl");
    let rx = events(&env.client);
    env.report(json!({"keep": "k"}), None, "s").unwrap();
    env.report(json!({"temp": "t"}), Some(1), "s").unwrap();
    assert_eq!(env.tokens().len(), 2);
    assert_eq!(next_tokens(&rx).len(), 1);
    assert_eq!(next_tokens(&rx).len(), 2);
    // The sweeper tells the subscribers when the time is up.
    assert_eq!(next_tokens(&rx), map(&[("keep", "k")]));
    assert_eq!(env.tokens(), map(&[("keep", "k")]));
}

#[test]
fn clearing_a_source_leaves_the_tokens_of_others() {
    let env = Env::new("clear");
    env.report(json!({"a": "1", "b": "2"}), None, "one")
        .unwrap();
    env.report(json!({"c": "3"}), None, "two").unwrap();
    assert_eq!(env.clear("one")["cleared"], 2);
    assert_eq!(env.tokens(), map(&[("c", "3")]));
    assert_eq!(env.clear("one")["cleared"], 0);
    // A later report by another source takes the token over.
    env.report(json!({"c": "4"}), None, "one").unwrap();
    env.clear("two");
    assert_eq!(env.tokens(), map(&[("c", "4")]));
}

#[test]
fn invalid_reports_are_rejected_and_change_nothing() {
    let env = Env::new("invalid");
    for tokens in [
        json!({"bad name": "x"}),
        json!({"": "x"}),
        json!({"a".repeat(33): "x"}),
    ] {
        let err = env.report(tokens, None, "s").unwrap_err();
        assert!(err.contains("invalid token name"), "{err}");
    }
    let many: serde_json::Map<String, Value> =
        (0..17).map(|i| (format!("k{i}"), json!("v"))).collect();
    assert!(env.report(Value::Object(many), None, "s").is_err());
    assert!(env.report(json!({"a": "1"}), Some(0), "s").is_err());
    assert!(env.report(json!({"a": "1"}), None, " ").is_err());
    let sixteen: serde_json::Map<String, Value> =
        (0..16).map(|i| (format!("k{i}"), json!("v"))).collect();
    env.report(Value::Object(sixteen), None, "s").unwrap();
    assert_eq!(env.tokens().len(), 16);
    let unknown = env
        .client
        .request(
            "terminal.report_tokens",
            json!({"terminal_id": "t999", "tokens": {}, "source": "s"}),
        )
        .unwrap_err();
    assert!(unknown.to_string().contains("t999"));
}

#[test]
fn the_cli_reports_with_the_terminal_of_the_environment() {
    let env = Env::new("cli");
    let out = env.cli(
        Some(&env.terminal),
        &[
            "tokens",
            "--source",
            "deliver",
            "deliver_step=review",
            "deliver_pr=#12",
        ],
    );
    assert!(out.status.success());
    assert_eq!(
        env.tokens(),
        map(&[("deliver_pr", "#12"), ("deliver_step", "review")])
    );
    // An explicit --terminal works without the environment; `name=` removes.
    let out = env.cli(
        None,
        &["tokens", "--terminal", &env.terminal, "deliver_pr="],
    );
    assert!(out.status.success());
    assert_eq!(env.tokens(), map(&[("deliver_step", "review")]));
    let out = env.cli(Some(&env.terminal), &["clear", "--source", "deliver"]);
    assert!(out.status.success());
    assert!(env.tokens().is_empty());
}

#[test]
fn the_cli_never_breaks_a_script() {
    let env = Env::new("cli-quiet");
    // Outside a poimen terminal: no effect, exit 0.
    let out = env.cli(None, &["tokens", "a=b"]);
    assert!(out.status.success());
    assert!(env.tokens().is_empty());
    // A server that does not answer: a note, still exit 0.
    let gone = Command::new(env!("CARGO_BIN_EXE_poimen-server"))
        .args(["report", "tokens", "a=b"])
        .env("POIMEN_SOCKET", env.dir.join("nobody.sock"))
        .env("POIMEN_TERMINAL_ID", "t1")
        .output()
        .unwrap();
    assert!(gone.status.success());
    // Only a usage error fails, with code 2.
    let bad = env.cli(None, &["tokens", "not a pair"]);
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn the_cli_reports_an_agent_lifecycle() {
    let env = Env::new("cli-agent");
    let out = env.cli(
        Some(&env.terminal),
        &[
            "agent", "--state", "blocked", "--source", "s", "--agent", "aider",
        ],
    );
    assert!(out.status.success());
    let t = env.client.list_terminals(None).unwrap().remove(0);
    let agent = t.agent.unwrap();
    assert_eq!(agent.agent.as_deref(), Some("aider"));
    assert_eq!(
        serde_json::to_value(agent.status).unwrap(),
        json!("blocked")
    );
}

#[test]
fn tokens_end_with_the_process() {
    let env = Env::new("exit");
    env.report(json!({"a": "1"}), None, "s").unwrap();
    let rx = events(&env.client);
    env.client.input(&env.terminal, "exit\n").unwrap();
    assert!(next_tokens(&rx).is_empty());
    assert!(env.tokens().is_empty());
}
