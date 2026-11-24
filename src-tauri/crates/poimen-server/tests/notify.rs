//! `notify` (#1017): a command runs once per transition to `blocked` or `done`, a failing one
//! only reaches `server.log`. Unix-only: the commands are `/bin/sh`.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::TerminalCreate;
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

const LONG: Duration = Duration::from_secs(10);

struct Env {
    dir: PathBuf,
    client: Client,
    terminal: String,
}

/// Lines of a command's output, read on a thread and delivered on a channel: a wait blocks on
/// the channel (with a limit), never on the clock.
fn lines_of(mut reader: impl BufRead + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
            if tx.send(line.trim_end().to_owned()).is_err() {
                return;
            }
            line.clear();
        }
    });
    rx
}

fn next_line(rx: &Receiver<String>, what: &str) -> String {
    rx.recv_timeout(LONG)
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

impl Env {
    fn new(name: &str, notify: &serde_json::Value) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-notify-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let home = dir.join("home");
        let config_path = poimen_core::settings::config_path(&home);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, json!({ "notify": notify }).to_string()).unwrap();
        let state = dir.join("state").join("server.json");
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            history_dir: state.with_file_name("history"),
            state_file: state,
            scrollback: 1000,
            home: Some(home),
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

    fn report(&self, seq: u64, status: &str, message: &str) {
        self.client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": status,
                       "agent": "claude", "message": message}),
            )
            .unwrap();
    }
}

/// A named pipe the commands write to: opening it for reading blocks until a command writes.
fn fifo(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("poimen-notify-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    assert!(
        poimen_core::gui_path::command("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    path
}

fn write_command(file: &Path) -> serde_json::Value {
    json!([
        "sh",
        "-c",
        format!("echo \"$0\" > '{}'", file.display()),
        "{status}|{agent}|{space}|{text}"
    ])
}

#[test]
fn a_command_runs_once_per_transition_with_the_fields() {
    let out = fifo("out");
    let env = Env::new(
        "once",
        &json!({ "blocked": { "command": write_command(&out) },
                 "done": { "command": write_command(&out) } }),
    );
    let read = |what: &str| {
        let file = std::fs::File::open(&out).unwrap();
        next_line(&lines_of(BufReader::new(file)), what)
    };
    env.report(1, "working", "");
    env.report(2, "blocked", "Allow Bash?");
    env.report(3, "blocked", "Allow Bash?");
    assert_eq!(
        read("the blocked notification"),
        "blocked|claude|api|Allow Bash?"
    );
    env.report(4, "working", "");
    env.report(5, "done", "");
    // A second blocked call would arrive here, before the done one.
    assert_eq!(read("the done notification"), "done|claude|api|");
    let _ = std::fs::remove_dir_all(&env.dir);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_failing_or_hanging_command_is_logged_and_never_blocks_the_server() {
    let env = Env::new(
        "fail",
        &json!({ "timeoutSeconds": 1, "blocked": { "command": ["sleep", "30"] },
                 "done": { "command": ["sh", "-c", "echo nope >&2; exit 4"] } }),
    );
    let started = Instant::now();
    env.report(1, "blocked", "q");
    env.report(2, "done", "");
    assert!(started.elapsed() < Duration::from_secs(3), "reports waited");
    assert!(env.client.ping().is_ok());
    let log = env.dir.join("state").join("server.log");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log);
    let mut tail = poimen_core::gui_path::command("tail")
        .args(["-F"])
        .arg(&log)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let rx = lines_of(BufReader::new(tail.stdout.take().unwrap()));
    let (mut timed_out, mut failed) = (false, false);
    while !(timed_out && failed) {
        let line = next_line(&rx, "both failures in the log");
        timed_out |= line.contains("timed out");
        failed |= line.contains("nope");
    }
    let _ = tail.kill();
    let _ = tail.wait();
    let _ = std::fs::remove_dir_all(&env.dir);
}
