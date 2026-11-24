//! `notify` (#1017): a command runs once per transition to `blocked` or `done`, a failing one
//! only reaches `server.log`. Unix-only: the commands are `/bin/sh`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
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

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < LONG, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
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

    fn lines(&self, file: &Path) -> Vec<String> {
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn append_command(file: &Path) -> serde_json::Value {
    json!([
        "sh",
        "-c",
        format!("echo \"$0\" >> '{}'", file.display()),
        "{status}|{agent}|{space}|{text}"
    ])
}

#[test]
fn a_command_runs_once_per_transition_with_the_fields() {
    let out = std::env::temp_dir().join(format!("poimen-notify-out-{}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let env = Env::new(
        "once",
        &json!({ "blocked": { "command": append_command(&out) },
                 "done": { "command": append_command(&out) } }),
    );
    env.report(1, "working", "");
    env.report(2, "blocked", "Allow Bash?");
    env.report(3, "blocked", "Allow Bash?");
    wait_for("the blocked notification", || !env.lines(&out).is_empty());
    env.report(4, "working", "");
    env.report(5, "done", "");
    wait_for("the done notification", || env.lines(&out).len() == 2);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        env.lines(&out),
        vec!["blocked|claude|api|Allow Bash?", "done|claude|api|"]
    );
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
    wait_for("both failures in the log", || {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        text.contains("timed out") && text.contains("nope")
    });
    let _ = std::fs::remove_dir_all(&env.dir);
}
