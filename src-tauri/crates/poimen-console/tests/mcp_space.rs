//! `create_space` and `list_repositories` (#1209) against an in-process poimen-server on a
//! temporary socket. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{ClientInfo, ClientKind, Event, EventsSubscribe, TerminalCreate};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    caller: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    #[allow(clippy::disallowed_methods)] // a test fixture, no window to hide
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A repository `name` under `dir` with a GitHub `origin`.
fn repo(dir: &Path, name: &str, remote: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["remote", "add", "origin", remote]);
    path.canonicalize().unwrap()
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpsp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
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
        .unwrap();
    let caller = client
        .create_terminal(&TerminalCreate {
            space_id: space.id,
            name: Some("lead".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    Fixture {
        dir,
        client,
        caller,
    }
}

fn app(client: &Client) -> Receiver<Event> {
    let stream = client
        .subscribe_with(EventsSubscribe {
            client: Some(ClientInfo {
                kind: ClientKind::App,
                pid: std::process::id(),
            }),
            confirms: true,
            ..Default::default()
        })
        .unwrap();
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

/// Answers the next question of the app; returns its action and text.
fn answer(
    client: &Client,
    rx: Receiver<Event>,
    approve: bool,
) -> thread::JoinHandle<(String, String)> {
    let client = client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let e = rx.recv_timeout(left).expect("the question");
            if let Event::AgentConfirmRequested {
                id, text, action, ..
            } = e
            {
                client.answer_agent_confirm(&id, approve).unwrap();
                return (format!("{action:?}"), text);
            }
        }
    })
}

fn call(f: &Fixture, terminal: Option<&str>, tool: &str, args: Value) -> Value {
    let env = Env {
        terminal_id: terminal,
        home: Some(f.dir.join("home")),
        confirm_wait: Duration::from_secs(30),
        events_dir: None,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn creates_a_space_after_the_user_confirms_even_in_the_own_space() {
    let f = fixture("create");
    let nixe = repo(&f.dir, "nixe", "git@github.com:acme/nixe.git");
    let rx = app(&f.client);
    let asked = answer(&f.client, rx, true);
    let r = call(
        &f,
        Some(&f.caller),
        "create_space",
        json!({"directory": nixe.display().to_string(), "name": "Nixe"}),
    );
    assert_ne!(r["isError"], true, "{r}");
    let out = &r["structuredContent"];
    assert_eq!(out["created"], true, "{r}");
    assert_eq!(out["space"]["name"], "Nixe");
    assert_eq!(out["space"]["directory"], nixe.display().to_string());
    let (action, shown) = asked.join().unwrap();
    assert_eq!(action, "CreateSpace");
    assert!(shown.contains(&nixe.display().to_string()), "{shown}");
    let spaces = f.client.list_spaces().unwrap();
    assert!(spaces.iter().any(|s| s.id == out["space"]["id"]));
}

#[test]
fn an_existing_space_on_the_directory_is_returned_unchanged_without_a_question() {
    let f = fixture("existing");
    let work = f.dir.join("work").canonicalize().unwrap();
    let before = f.client.list_spaces().unwrap();
    // No app is connected: a question would be refused, so this proves none is asked.
    let r = call(
        &f,
        Some(&f.caller),
        "create_space",
        json!({"directory": work.display().to_string(), "name": "other name"}),
    );
    assert_ne!(r["isError"], true, "{r}");
    assert_eq!(r["structuredContent"]["created"], false, "{r}");
    assert_eq!(r["structuredContent"]["space"]["name"], "work");
    assert_eq!(f.client.list_spaces().unwrap(), before);
}

#[test]
fn a_decline_and_a_missing_app_create_nothing() {
    let f = fixture("decline");
    let nixe = repo(&f.dir, "nixe", "git@github.com:acme/nixe.git");
    let dir = nixe.display().to_string();
    let before = f.client.list_spaces().unwrap().len();
    let r = call(
        &f,
        Some(&f.caller),
        "create_space",
        json!({"directory": dir}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("not open"), "{r}");
    let rx = app(&f.client);
    let asked = answer(&f.client, rx, false);
    let r = call(
        &f,
        Some(&f.caller),
        "create_space",
        json!({"directory": dir}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("declined"), "{r}");
    asked.join().unwrap();
    assert_eq!(f.client.list_spaces().unwrap().len(), before);
}

#[test]
fn an_untrusted_project_file_makes_no_space() {
    let f = fixture("trust");
    let nixe = repo(&f.dir, "nixe", "git@github.com:acme/nixe.git");
    std::fs::create_dir_all(nixe.join(".poimen")).unwrap();
    std::fs::write(
        nixe.join(".poimen/config.json"),
        r#"{"setup": "echo untrusted"}"#,
    )
    .unwrap();
    let rx = app(&f.client);
    let asked = answer(&f.client, rx, true);
    let before = f.client.list_spaces().unwrap().len();
    let r = call(
        &f,
        Some(&f.caller),
        "create_space",
        json!({"directory": nixe.display().to_string()}),
    );
    asked.join().unwrap();
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("not trusted"), "{r}");
    assert_eq!(f.client.list_spaces().unwrap().len(), before);
}

#[test]
fn bad_arguments_and_a_caller_outside_poimen_are_tool_errors() {
    let f = fixture("bad");
    for args in [
        json!({}),
        json!({"directory": "relative/dir"}),
        json!({"directory": "/definitely/not/here"}),
        json!({"directory": "/tmp", "extra": 1}),
    ] {
        let r = call(&f, Some(&f.caller), "create_space", args);
        assert_eq!(r["isError"], true, "{r}");
    }
    let nixe = repo(&f.dir, "nixe", "git@github.com:acme/nixe.git");
    let r = call(
        &f,
        None,
        "create_space",
        json!({"directory": nixe.display().to_string()}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("POIMEN_TERMINAL_ID"), "{r}");
}

#[test]
fn list_repositories_has_spaces_worktrees_and_trusted_projects_once() {
    let f = fixture("list");
    let nixe = repo(&f.dir, "nixe", "git@github.com:acme/nixe.git");
    git(
        &nixe,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "i",
        ],
    );
    let wt = f.dir.join("nixe-wt");
    git(
        &nixe,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task",
            &wt.display().to_string(),
        ],
    );
    let wt = wt.canonicalize().unwrap();
    let space = f
        .client
        .create_space("wt", &wt.display().to_string())
        .unwrap();
    let other = repo(&f.dir, "other", "https://example.com/x/other.git");
    let home = f.dir.join("home");
    std::fs::create_dir_all(home.join(".config/poimen")).unwrap();
    std::fs::write(
        home.join(".config/poimen/config.json"),
        json!({"trustedProjects": {
            other.display().to_string(): "abc",
            "/gone/away": "def"
        }})
        .to_string(),
    )
    .unwrap();
    let r = call(&f, None, "list_repositories", json!({}));
    assert_ne!(r["isError"], true, "{r}");
    let repos = r["structuredContent"]["repositories"].as_array().unwrap();
    assert_eq!(repos.len(), 2, "{r}");
    let nixe_row = repos
        .iter()
        .find(|r| r["path"] == nixe.display().to_string())
        .expect("the main checkout of the worktree space");
    assert_eq!(nixe_row["github"], "acme/nixe");
    assert_eq!(nixe_row["remote"], "git@github.com:acme/nixe.git");
    assert_eq!(nixe_row["spaces"], json!([space.id]));
    let other_row = repos
        .iter()
        .find(|r| r["path"] == other.display().to_string())
        .expect("the trusted project");
    assert_eq!(other_row["github"], Value::Null);
    assert_eq!(other_row["spaces"], json!([]));
}

#[test]
fn both_tools_are_listed_with_their_use() {
    let f = fixture("tools");
    let env = Env {
        terminal_id: None,
        home: None,
        confirm_wait: Duration::from_secs(1),
        events_dir: None,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let v: Value =
        serde_json::from_str(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap();
    for name in ["create_space", "list_repositories"] {
        let tool = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name}"));
        assert!(
            tool["description"].as_str().unwrap().contains("Use"),
            "{name}"
        );
    }
}
