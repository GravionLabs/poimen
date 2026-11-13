//! Progress events of runs (`run.events`, #692): a subscription with `run_events` gets the events
//! of the run files in the repositories of the server's spaces, first from the start, then each
//! new one once; a subscription without it never sees them. Against a real server on a temp
//! socket, with a temp git repository.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{Event, EventsSubscribe};
use poimen_server::{Config, PtyHost, Server};
use serde_json::Value;

struct Env {
    repo: PathBuf,
    client: Client,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-runs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let repo = repo.canonicalize().unwrap();
        let git = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(git.success());
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let server = Server::bind(config.clone()).unwrap();
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        client
            .create_space("api", &repo.display().to_string())
            .unwrap();
        Self { repo, client }
    }

    fn events_file(&self, run: &str) -> PathBuf {
        let dir = self.repo.join(".git/.agents/events");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{run}.jsonl"))
    }

    /// A subscription; `runs` asks for `run_events`. Its events arrive on the receiver.
    fn subscribe(&self, runs: bool) -> Receiver<Event> {
        let stream = self
            .client
            .subscribe_with(EventsSubscribe {
                client: None,
                run_events: runs,
            })
            .unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for event in stream.flatten() {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        rx
    }
}

fn line(run: &str, seq: u32, ty: &str) -> String {
    format!(
        r#"{{"v":1,"seq":{seq},"ts":"2026-11-13T10:00:0{seq}Z","run":"{run}","type":"{ty}","tone":"info"}}"#
    ) + "\n"
}

fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

/// The `(run, seq)` of the run events that arrive until `n` are there (10 s at most).
fn run_seqs(rx: &Receiver<Event>, n: usize) -> Vec<(String, i64)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    while got.len() < n {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Event::RunEvents { run, events, root }) => {
                assert!(root.ends_with("/repo"), "{root}");
                got.extend(
                    events
                        .iter()
                        .map(|e: &Value| (run.clone(), e["seq"].as_i64().unwrap())),
                );
            }
            Ok(_) => {}
            Err(_) => panic!("only {got:?} of {n} run events arrived"),
        }
    }
    got
}

/// No run event within a few polls.
fn no_run_events(rx: &Receiver<Event>) {
    let deadline = Instant::now() + Duration::from_millis(1500);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Ok(Event::RunEvents { .. }) = rx.recv_timeout(left) {
            panic!("unexpected run event");
        }
    }
}

fn pair(run: &str, seq: i64) -> (String, i64) {
    (run.to_owned(), seq)
}

#[test]
fn a_run_subscription_gets_the_events_from_the_start_then_each_new_one_once() {
    let env = Env::new("follow");
    let file = env.events_file("r1");
    append(
        &file,
        &(line("r1", 1, "run.started") + &line("r1", 2, "progress")),
    );
    let rx = env.subscribe(true);
    assert_eq!(run_seqs(&rx, 2), [pair("r1", 1), pair("r1", 2)]);
    // A half-written line waits until it is complete.
    let third = line("r1", 3, "task.started");
    append(&file, &third[..12]);
    no_run_events(&rx);
    append(&file, &third[12..]);
    assert_eq!(run_seqs(&rx, 1), [pair("r1", 3)]);
    // A new run file of the same repository.
    append(&env.events_file("r2"), &line("r2", 1, "run.started"));
    assert_eq!(run_seqs(&rx, 1), [pair("r2", 1)]);
    no_run_events(&rx);
}

#[test]
fn a_new_subscription_gets_the_replay_and_a_plain_one_no_run_events() {
    let env = Env::new("replay");
    let file = env.events_file("r1");
    append(&file, &line("r1", 1, "run.started"));
    let plain = env.subscribe(false);
    let first = env.subscribe(true);
    assert_eq!(run_seqs(&first, 1), [pair("r1", 1)]);
    append(&file, &line("r1", 2, "progress"));
    assert_eq!(run_seqs(&first, 1), [pair("r1", 2)]);
    // A reconnect: the events of the run from the start, then the new ones.
    let again = env.subscribe(true);
    assert_eq!(run_seqs(&again, 2), [pair("r1", 1), pair("r1", 2)]);
    append(&file, &line("r1", 3, "progress"));
    assert_eq!(run_seqs(&again, 1), [pair("r1", 3)]);
    assert_eq!(run_seqs(&first, 1), [pair("r1", 3)]);
    no_run_events(&plain);
}
