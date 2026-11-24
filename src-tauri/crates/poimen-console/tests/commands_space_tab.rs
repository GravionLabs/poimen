//! `poimen space ...` and `poimen tab ...` (#1172) against an in-process poimen-server on a temporary socket
//! (never the user's server). Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_console::cli::{self, Command, Request};
use poimen_console::commands::{self, Failure};
use poimen_server::{Config, PtyHost, Server};

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
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir
            .join("state")
            .join("server.json")
            .with_file_name("history"),
        scrollback: 1000,
        home: Some(dir.join("home")),
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
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

fn ok(f: &Fixture, args: &[&str]) -> String {
    f.run(args).unwrap().trim().to_string()
}

fn err(f: &Fixture, args: &[&str]) -> String {
    f.run(args).unwrap_err().0
}

fn tabs(f: &Fixture) -> Vec<(String, String)> {
    let mut t: Vec<_> = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .map(|t| (t.tab_id, t.name))
        .collect();
    t.sort();
    t
}

#[test]
fn space_new_rename_close_by_name_or_id() {
    let f = fixture("space");
    let dir = f.dir.join("other");
    std::fs::create_dir_all(&dir).unwrap();
    let id = ok(
        &f,
        &["space", "new", dir.to_str().unwrap(), "--name", "other"],
    );
    let spaces = f.client.list_spaces().unwrap();
    assert!(spaces.iter().any(|s| s.id == id && s.name == "other"));
    assert!(
        !f.client
            .list_layout()
            .unwrap()
            .tabs
            .iter()
            .all(|t| t.space_id != id)
    );
    f.run(&["space", "rename", "other", "renamed"]).unwrap();
    assert!(
        f.client
            .list_spaces()
            .unwrap()
            .iter()
            .any(|s| s.id == id && s.name == "renamed")
    );
    f.run(&["space", "close", &id]).unwrap();
    assert!(f.client.list_spaces().unwrap().iter().all(|s| s.id != id));
    assert!(err(&f, &["space", "close", "nope"]).contains("not found"));
    assert!(err(&f, &["space", "new", "/does/not/exist"]).contains("not an existing directory"));
}

#[test]
fn space_new_names_the_space_after_the_directory() {
    let f = fixture("spacename");
    let dir = f.dir.join("folder");
    std::fs::create_dir_all(&dir).unwrap();
    let id = ok(&f, &["space", "new", dir.to_str().unwrap()]);
    let s = f
        .client
        .list_spaces()
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(s.name, "folder");
}

fn untrusted_project(f: &Fixture) -> String {
    let dir = f.dir.join("proj");
    std::fs::create_dir_all(dir.join(".poimen")).unwrap();
    std::fs::write(
        dir.join(".poimen/config.json"),
        r#"{"setup": "echo untrusted"}"#,
    )
    .unwrap();
    dir.to_str().unwrap().to_string()
}

#[test]
fn space_new_with_untrusted_commands_exits_with_commands_and_hash() {
    let f = fixture("untrusted");
    let dir = untrusted_project(&f);
    let msg = err(&f, &["space", "new", &dir]);
    assert!(msg.contains("echo untrusted"), "{msg}");
    assert!(msg.contains("hash: "), "{msg}");
    assert!(msg.contains("--trust"), "{msg}");
    assert_eq!(
        f.client.list_spaces().unwrap().len(),
        1,
        "nothing was created"
    );
}

#[test]
fn space_new_trust_and_without_commands_answer_the_question() {
    let f = fixture("trust");
    let dir = untrusted_project(&f);
    let id = ok(&f, &["space", "new", &dir, "--without-commands"]);
    assert!(f.client.list_spaces().unwrap().iter().any(|s| s.id == id));
    let dir2 = f.dir.join("proj");
    let id2 = ok(
        &f,
        &[
            "space",
            "new",
            dir2.to_str().unwrap(),
            "--name",
            "p2",
            "--trust",
        ],
    );
    assert_ne!(id, id2);
    // Trusted now: no question without a flag.
    ok(&f, &["space", "new", &dir, "--name", "p3"]);
}

#[test]
fn tab_new_rename_focus_close() {
    let f = fixture("tab");
    let before = tabs(&f).len();
    let tab = ok(&f, &["tab", "new", "work", "--name", "build"]);
    assert_eq!(tabs(&f).len(), before + 1);
    assert!(tabs(&f).contains(&(tab.clone(), "build".into())));
    f.run(&["tab", "rename", "build", "compile"]).unwrap();
    assert!(tabs(&f).contains(&(tab.clone(), "compile".into())));
    let terminal = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .iter()
        .find(|t| t.tab_id == tab)
        .unwrap()
        .root
        .first_terminal()
        .to_string();
    f.run(&["tab", "focus", &tab, &terminal]).unwrap();
    let focused = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.tab_id == tab)
        .unwrap();
    assert_eq!(focused.active_terminal, terminal);
    f.run(&["tab", "close", "compile"]).unwrap();
    assert_eq!(tabs(&f).len(), before);
    assert!(err(&f, &["tab", "close", "compile"]).contains("not found"));
}

#[test]
fn tab_new_with_a_command_runs_it_in_a_new_tab() {
    let f = fixture("tabcmd");
    let tab = ok(
        &f,
        &[
            "tab", "new", &f.space, "--name", "job", "--", "sh", "-c", "sleep 30",
        ],
    );
    let t = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.tab_id == tab)
        .unwrap();
    assert_eq!(t.name, "job");
    let term = f
        .client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|x| t.root.contains_terminal(&x.id))
        .unwrap();
    assert_eq!(term.args, vec!["-c".to_string(), "sleep 30".to_string()]);
}

#[test]
fn tab_split_adds_a_terminal_to_the_tab() {
    let f = fixture("split");
    let t = f.terminal("sleep 30");
    let new = ok(
        &f,
        &[
            "tab", "split", &t, "down", "--ratio", "0.3", "--", "sh", "-c", "sleep 30",
        ],
    );
    let tab = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .find(|tab| tab.root.contains_terminal(&t))
        .unwrap();
    assert!(tab.root.contains_terminal(&new));
    assert!(err(&f, &["tab", "split", "nope", "right"]).contains("nope"));
}

#[test]
fn closing_with_a_running_agent_needs_yes() {
    let f = fixture("agent");
    let t = f.terminal("sleep 30");
    f.report(&t, 1, AgentStatus::Working);
    let tab = f
        .client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .find(|tab| tab.root.contains_terminal(&t))
        .unwrap();
    let msg = err(&f, &["tab", "close", &tab.tab_id]);
    assert!(
        msg.contains("running agent") && msg.contains("--yes"),
        "{msg}"
    );
    let msg = err(&f, &["space", "close", "work"]);
    assert!(
        msg.contains("running agent") && msg.contains("--yes"),
        "{msg}"
    );
    assert_eq!(f.client.list_spaces().unwrap().len(), 1);
    // Another tab without an agent closes without asking.
    let other = ok(&f, &["tab", "new", "work"]);
    f.run(&["tab", "close", &other]).unwrap();
    f.run(&["tab", "close", &tab.tab_id, "--yes"]).unwrap();
    f.run(&["space", "close", "work", "--yes"]).unwrap();
    assert!(f.client.list_spaces().unwrap().is_empty());
}

#[test]
fn an_ambiguous_tab_name_needs_the_id() {
    let f = fixture("ambig");
    ok(&f, &["tab", "new", "work", "--name", "same"]);
    ok(&f, &["tab", "new", "work", "--name", "same"]);
    assert!(err(&f, &["tab", "close", "same"]).contains("several tabs"));
}
