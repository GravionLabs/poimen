//! `poimen ls|status|new|send|prompt` against an in-process poimen-server on a temporary socket
//! (never the user's server). Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_console::cli::{self, Command, Request};
use poimen_console::commands::{self, Failure};
use poimen_server::{Config, Server};
use serde_json::Value;

struct Fixture {
    dir: PathBuf,
    client: Client,
    space: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-cmd-{}-{name}", std::process::id()));
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
        .create_space("work", &work.display().to_string())
        .unwrap()
        .id;
    Fixture { dir, client, space }
}

impl Fixture {
    fn terminal(&self, script: &str) -> String {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: self.space.clone(),
                name: Some("shell".into()),
                command: Some(vec!["sh".into(), "-c".into(), script.into()]),
                ..Default::default()
            })
            .unwrap()
            .id
    }

    fn report(&self, terminal: &str, seq: u64, status: AgentStatus) {
        self.client
            .request(
                "terminal.report_agent",
                serde_json::json!({"terminal_id": terminal, "seq": seq,
                    "status": status, "agent": "claude"}),
            )
            .unwrap();
    }

    fn run(&self, args: &[&str]) -> Result<String, Failure> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let Ok(Command::Request(r)) = cli::parse(&args) else {
            panic!("not a request: {args:?}")
        };
        self.run_request(&r, Duration::from_millis(300))
    }

    fn run_request(&self, r: &Request, wait: Duration) -> Result<String, Failure> {
        let mut out = Vec::new();
        commands::run(&self.client, r, wait, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }
}

fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ls_lists_spaces_terminals_and_agent_status() {
    let f = fixture("ls");
    let t = f.terminal("sleep 30");
    f.report(&t, 1, AgentStatus::Working);
    let text = f.run(&["ls"]).unwrap();
    let mut lines = text.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with(&format!("{}  work  ", f.space))
    );
    assert_eq!(
        lines.next().unwrap(),
        format!("  {t}  shell  running  claude:working")
    );
    assert_eq!(lines.next(), None);
}

#[test]
fn ls_json_nests_terminals_in_spaces() {
    let f = fixture("lsjson");
    let t = f.terminal("sleep 30");
    let v: Value = serde_json::from_str(&f.run(&["ls", "--json"]).unwrap()).unwrap();
    assert_eq!(v[0]["id"], f.space.as_str());
    assert_eq!(v[0]["name"], "work");
    assert_eq!(v[0]["terminals"][0]["id"], t.as_str());
    assert_eq!(v[0]["terminals"][0]["status"], "running");
}

#[test]
fn status_text_and_json() {
    let f = fixture("status");
    let t = f.terminal("sleep 30");
    assert_eq!(
        f.run(&["status", &t]).unwrap(),
        format!("{t}  shell  running  -\n")
    );
    f.report(&t, 1, AgentStatus::Blocked);
    let v: Value = serde_json::from_str(&f.run(&["status", "--json", &t]).unwrap()).unwrap();
    assert_eq!(v["agent"]["status"], "blocked");
    assert_eq!(v["agent"]["agent"], "claude");
}

#[test]
fn status_of_an_unknown_terminal_fails() {
    let f = fixture("nostatus");
    let Failure(m) = f.run(&["status", "nope"]).unwrap_err();
    assert!(m.contains("not found"), "{m}");
}

#[test]
fn new_creates_a_terminal_in_a_space_by_name_or_id() {
    let f = fixture("new");
    let id = f
        .run(&["new", "work", "--name", "build", "--", "sleep", "30"])
        .unwrap();
    let id = id.trim();
    let t = f.client.list_terminals(None).unwrap();
    let t = t.iter().find(|t| t.id == id).unwrap();
    assert_eq!(
        (t.name.as_str(), t.space_id.as_str()),
        ("build", f.space.as_str())
    );
    assert!(f.run(&["new", &f.space, "--", "sleep", "30"]).is_ok());
    assert_eq!(f.client.list_terminals(None).unwrap().len(), 2);
}

#[test]
fn new_in_an_unknown_space_fails() {
    let f = fixture("nospace");
    let Failure(m) = f.run(&["new", "missing"]).unwrap_err();
    assert!(m.contains("not found"), "{m}");
}

#[test]
fn send_types_the_text_and_enter() {
    let f = fixture("send");
    let out = f.dir.join("typed");
    let t = f.terminal(&format!(
        "read l; echo \"got:$l\" > '{}'; sleep 30",
        out.display()
    ));
    f.run(&["send", &t, "--enter", "hello", "world"]).unwrap();
    eventually("typed line", || {
        std::fs::read_to_string(&out).is_ok_and(|s| s == "got:hello world\n")
    });
}

#[test]
fn prompt_is_refused_without_agent_while_working_and_blocked() {
    let f = fixture("refuse");
    let t = f.terminal("sleep 30");
    let Failure(m) = f.run(&["prompt", &t, "go"]).unwrap_err();
    assert!(m.contains("no agent"), "{m}");
    f.report(&t, 1, AgentStatus::Working);
    let Failure(m) = f.run(&["prompt", &t, "go"]).unwrap_err();
    assert!(m.contains("working"), "{m}");
    f.report(&t, 2, AgentStatus::Blocked);
    let Failure(m) = f.run(&["prompt", &t, "go"]).unwrap_err();
    assert!(m.contains("blocked"), "{m}");
}

#[test]
fn prompt_types_the_text_when_the_agent_is_idle() {
    let f = fixture("prompt");
    let out = f.dir.join("typed");
    let t = f.terminal(&format!(
        "read l; echo \"$l\" > '{}'; sleep 30",
        out.display()
    ));
    f.report(&t, 1, AgentStatus::Idle);
    let client = f.client.clone();
    let id = t.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        client
            .request(
                "terminal.report_agent",
                serde_json::json!({"terminal_id": id, "seq": 2, "status": "working"}),
            )
            .unwrap();
    });
    let text = f
        .run_request(
            &Request::Prompt {
                terminal: t,
                text: "fix it".into(),
            },
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(text, "started: agent is working\n");
    eventually("typed prompt", || {
        std::fs::read_to_string(&out).is_ok_and(|s| s == "fix it\n")
    });
}

#[test]
fn an_unreachable_server_is_an_error_naming_it() {
    let client = Client::new(std::env::temp_dir().join("poimen-cmd-none").join("x.sock"));
    let mut out = Vec::new();
    let Failure(m) = commands::run(
        &client,
        &Request::Ls { json: false },
        Duration::ZERO,
        &mut out,
    )
    .unwrap_err();
    assert!(m.contains("not reachable"), "{m}");
}
