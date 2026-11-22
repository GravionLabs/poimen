//! The time of the last accepted agent report and of the last status change (#897).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{AgentInfo, Event, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

struct Env {
    dir: PathBuf,
    client: Client,
    terminal: String,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-times-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            history_dir: dir
                .join("state")
                .join("server.json")
                .with_file_name("history"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
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

    fn report(&self, seq: u64, status: &str) -> bool {
        let r = self
            .client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": status}),
            )
            .unwrap();
        r["accepted"].as_bool().unwrap()
    }

    fn agent(&self) -> AgentInfo {
        self.client
            .list_terminals(None)
            .unwrap()
            .remove(0)
            .agent
            .expect("agent")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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

fn status_events(rx: &Receiver<Event>) -> usize {
    rx.try_iter()
        .filter(|e| matches!(e, Event::AgentStatusChanged { .. }))
        .count()
}

#[test]
fn a_report_sets_both_times_and_a_repeat_moves_only_the_report_time() {
    let env = Env::new("repeat");
    let rx = events(&env.client);
    assert!(env.report(1, "working"));
    let first = env.agent();
    let (reported, since) = (first.reported_at.unwrap(), first.status_since.unwrap());
    assert_eq!(reported, since);
    thread::sleep(Duration::from_millis(50));
    assert!(env.report(2, "working"));
    let again = env.agent();
    assert_eq!(again.status_since, Some(since));
    assert!(again.reported_at.unwrap() > reported);
    thread::sleep(Duration::from_millis(100));
    // One event for the change, none for the repeat.
    assert_eq!(status_events(&rx), 1);
}

#[test]
fn a_status_change_moves_both_times() {
    let env = Env::new("change");
    env.report(1, "working");
    let first = env.agent();
    thread::sleep(Duration::from_millis(50));
    env.report(2, "done");
    let next = env.agent();
    assert!(next.status_since.unwrap() > first.status_since.unwrap());
    assert_eq!(next.status_since, next.reported_at);
}

#[test]
fn a_stale_report_changes_neither_time() {
    let env = Env::new("stale");
    env.report(5, "working");
    let before = env.agent();
    thread::sleep(Duration::from_millis(30));
    assert!(!env.report(5, "done"));
    assert!(!env.report(4, "working"));
    assert_eq!(env.agent(), before);
}

#[test]
fn the_times_are_in_the_server_file() {
    let env = Env::new("stored");
    env.report(1, "working");
    let agent = env.agent();
    let text = std::fs::read_to_string(env.dir.join("state").join("server.json")).unwrap();
    assert!(
        text.contains(&format!("\"reported_at\": {}", agent.reported_at.unwrap())),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "\"status_since\": {}",
            agent.status_since.unwrap()
        )),
        "{text}"
    );
}

#[test]
fn repeated_reports_write_the_file_at_most_every_ten_seconds() {
    let env = Env::new("throttle");
    env.report(1, "working");
    let file = env.dir.join("state").join("server.json");
    let stored = std::fs::read_to_string(&file).unwrap();
    for seq in 2..6 {
        thread::sleep(Duration::from_millis(20));
        env.report(seq, "working");
    }
    // The server answers from memory with the newer time; the file keeps the first write.
    assert!(env.agent().reported_at.unwrap() > first_reported(&stored));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), stored);
}

fn first_reported(text: &str) -> u64 {
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    v["terminals"][0]["agent"]["reported_at"].as_u64().unwrap()
}
