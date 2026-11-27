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
use poimen_client::protocol::{Event, EventsSubscribe, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::Value;

struct Env {
    repo: PathBuf,
    /// The shared event directory of the server.
    shared: PathBuf,
    client: Client,
    /// Removes the directory of the server when the test ends.
    _dir: tempfile::TempDir,
}

impl Env {
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("poimen-runs-{name}-"))
            .tempdir()
            .unwrap();
        let dir = guard.path().to_path_buf();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let repo = repo.canonicalize().unwrap();
        git(&repo, &["init", "-q"]);
        git(
            &repo,
            &["remote", "add", "origin", "https://github.com/o/api.git"],
        );
        let shared = dir.join("events");
        std::fs::create_dir_all(&shared).unwrap();
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
        server.set_events_dir(Some(shared.clone()));
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        client
            .create_space("api", &repo.display().to_string())
            .unwrap();
        Self {
            repo,
            shared,
            client,
            _dir: guard,
        }
    }

    fn events_file(&self, run: &str) -> PathBuf {
        let dir = self.repo.join(".git/.agents/events");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{run}.jsonl"))
    }

    fn shared_file(&self, run: &str) -> PathBuf {
        self.shared.join(format!("{run}.jsonl"))
    }

    /// A subscription; `runs` asks for `run_events`. Its events arrive on the receiver.
    fn subscribe(&self, runs: bool) -> Receiver<Event> {
        let stream = self
            .client
            .subscribe_with(EventsSubscribe {
                client: None,
                run_events: runs,
                confirms: false,
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

fn git(dir: &Path, args: &[&str]) {
    let git = Command::new("git").args(args).current_dir(dir).status();
    assert!(git.unwrap().success());
}

/// A `run.started` line with the `origin` object `origin`.
fn started(run: &str, origin: &str) -> String {
    format!(
        r#"{{"v":1,"seq":1,"ts":"2026-11-13T10:00:01Z","run":"{run}","type":"run.started","tone":"info","origin":{origin}}}"#
    ) + "\n"
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

/// A run file of the subject `M16` (or none) whose last change was `days` days ago.
fn old_run(env: &Env, run: &str, subject: Option<&str>, days: u64) {
    let subject = subject.map_or(String::new(), |id| {
        format!(r#","subject":{{"kind":"github.milestone","id":"{id}"}}"#)
    });
    let file = env.events_file(run);
    let first = format!(
        r#"{{"v":1,"seq":1,"ts":"2026-11-13T10:00:01Z","run":"{run}","type":"run.started","tone":"info"{subject}}}"#
    ) + "\n";
    std::fs::write(&file, first + &line(run, 2, "progress")).unwrap();
    let f = std::fs::File::options().write(true).open(&file).unwrap();
    f.set_modified(std::time::SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

#[test]
fn earlier_runs_of_a_shown_subject_are_sent_and_other_old_runs_are_not() {
    let env = Env::new("subject");
    old_run(&env, "earlier", Some("M16"), 5);
    old_run(&env, "other", Some("M15"), 5);
    old_run(&env, "bare", None, 5);
    old_run(&env, "ancient", Some("M16"), 20);
    let rx = env.subscribe(true);
    // No run of M16 is recent: nothing yet.
    no_run_events(&rx);
    old_run(&env, "current", Some("M16"), 0);
    let mut got = run_seqs(&rx, 4);
    got.sort();
    assert_eq!(
        got,
        [
            pair("current", 1),
            pair("current", 2),
            pair("earlier", 1),
            pair("earlier", 2)
        ]
    );
}

#[test]
fn runs_of_the_shared_and_the_legacy_directory_arrive_once_with_the_repositorys_root() {
    let env = Env::new("shared");
    let worktree = format!(r#"{{"worktree":"{}"}}"#, env.repo.display());
    // By the worktree, by the remote's name, and one of a repository outside the spaces.
    append(&env.shared_file("tree"), &started("tree", &worktree));
    append(
        &env.shared_file("name"),
        &started("name", r#"{"repo":"o/api","worktree":"/gone"}"#),
    );
    append(
        &env.shared_file("foreign"),
        &started("foreign", r#"{"repo":"o/other"}"#),
    );
    // The same run in both places: the legacy copy lags one event behind.
    let both = started("both", &worktree) + &line("both", 2, "progress");
    append(&env.events_file("both"), &both);
    append(
        &env.shared_file("both"),
        &(both.clone() + &line("both", 3, "progress")),
    );
    append(&env.events_file("old"), &line("old", 1, "run.started"));
    let rx = env.subscribe(true);
    let mut got = run_seqs(&rx, 6);
    got.sort();
    let want = [
        pair("both", 1),
        pair("both", 2),
        pair("both", 3),
        pair("name", 1),
        pair("old", 1),
        pair("tree", 1),
    ];
    assert_eq!(got, want);
    // The writer of the old place appends what the shared file already has: not sent again.
    append(&env.events_file("both"), &line("both", 3, "progress"));
    no_run_events(&rx);
    let again = env.subscribe(true);
    let mut replay = run_seqs(&again, 6);
    replay.sort();
    assert_eq!(replay, want);
    no_run_events(&again);
}

#[test]
fn a_run_without_origin_belongs_to_the_repository_of_its_terminal() {
    let env = Env::new("terminal");
    append(&env.shared_file("mine"), &line("mine", 1, "run.started"));
    append(
        &env.shared_file("nobody"),
        &line("nobody", 1, "run.started"),
    );
    let rx = env.subscribe(true);
    no_run_events(&rx);
    let space = env.client.list_spaces().unwrap().remove(0);
    env.client
        .create_terminal(&TerminalCreate {
            space_id: space.id,
            command: Some(vec!["sh".into()]),
            run: Some("mine".into()),
            ..TerminalCreate::default()
        })
        .unwrap();
    assert_eq!(run_seqs(&rx, 1), [pair("mine", 1)]);
    no_run_events(&rx);
}
