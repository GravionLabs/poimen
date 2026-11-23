//! `start_shell` (#1181) against an in-process poimen-server on a temporary socket. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    ClientInfo, ClientKind, Event, EventsSubscribe, TerminalCreate, TerminalStatus,
};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    own_space: String,
    other_space: String,
    caller: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcps-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let make_space = |name: &str| {
        let path = dir.join(name).canonicalize().unwrap();
        client
            .create_space(name, &path.display().to_string())
            .unwrap()
            .id
    };
    let own_space = make_space("work");
    let other_space = make_space("other");
    let caller = client
        .create_terminal(&TerminalCreate {
            space_id: own_space.clone(),
            name: Some("lead".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    Fixture {
        dir,
        client,
        own_space,
        other_space,
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

fn call(client: &Client, terminal: Option<&str>, wait: Duration, args: Value) -> Value {
    let env = Env {
        terminal_id: terminal,
        home: None,
        confirm_wait: wait,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "start_shell", "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

/// Polls `read_terminal` of the new terminal until its text has `needle`.
fn screen_has(client: &Client, terminal: &str, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "read_terminal", "arguments": {"terminal": terminal}}});
        let mut out = Vec::new();
        let env = Env {
            terminal_id: None,
            home: None,
            confirm_wait: Duration::from_secs(1),
        };
        mcp::serve_with(client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
        let r: Value =
            serde_json::from_str(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap();
        if r["result"]["structuredContent"]["text"]
            .as_str()
            .is_some_and(|t| t.contains(needle))
        {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

const WAIT: Duration = Duration::from_secs(30);

#[test]
fn runs_the_command_in_the_own_space_and_the_pane_stays_after_it_ends() {
    let f = fixture("own");
    let rx = app(&f.client);
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"command": ["sh", "-c", "echo $GREETING from $(pwd); exit 3"],
               "name": "loop", "env": {"GREETING": "hello"}}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    let id = r["structuredContent"]["terminal"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(r["structuredContent"]["space"], f.own_space);
    let cwd = f.dir.join("work").canonicalize().unwrap();
    assert!(
        screen_has(&f.client, &id, &format!("hello from {}", cwd.display())),
        "the command's output is readable"
    );
    // The command ended: the terminal is still listed, exited, with its exit code.
    let deadline = Instant::now() + Duration::from_secs(10);
    let t = loop {
        let t = f
            .client
            .list_terminals(None)
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .expect("the terminal stays listed");
        if t.status == TerminalStatus::Exited || Instant::now() > deadline {
            break t;
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        (t.status, t.exit_code, t.name.as_str()),
        (TerminalStatus::Exited, Some(3), "loop")
    );
    assert_eq!(t.started_by.as_deref(), Some(f.caller.as_str()));
    // No confirmation was asked in the own space.
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }));
    }
}

#[test]
fn cwd_is_honored() {
    let f = fixture("cwd");
    let other = f.dir.join("other").canonicalize().unwrap();
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"command": ["pwd"], "cwd": other.display().to_string()}),
    );
    let id = r["structuredContent"]["terminal"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(screen_has(&f.client, &id, &other.display().to_string()));
}

#[test]
fn another_space_needs_the_user_and_a_decline_creates_nothing() {
    let f = fixture("other");
    let rx = app(&f.client);
    let answering = {
        let client = f.client.clone();
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let e = rx.recv_timeout(left).expect("the question");
                if let Event::AgentConfirmRequested { id, text, .. } = e {
                    client.answer_agent_confirm(&id, false).unwrap();
                    return text;
                }
            }
        })
    };
    let before = f.client.list_terminals(None).unwrap().len();
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"command": ["echo", "hi"], "space": f.other_space}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("declined"), "{r}");
    assert_eq!(answering.join().unwrap(), "echo hi");
    assert_eq!(f.client.list_terminals(None).unwrap().len(), before);
}

#[test]
fn another_space_runs_after_approval() {
    let f = fixture("approve");
    let rx = app(&f.client);
    let client = f.client.clone();
    thread::spawn(move || {
        for e in rx {
            if let Event::AgentConfirmRequested { id, .. } = e {
                client.answer_agent_confirm(&id, true).unwrap();
                return;
            }
        }
    });
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"command": ["echo", "approved-run"], "space": f.other_space}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["space"], f.other_space);
    let id = r["structuredContent"]["terminal"].as_str().unwrap();
    assert!(screen_has(&f.client, id, "approved-run"));
}

#[test]
fn refused_without_an_app_or_outside_a_poimen_terminal() {
    let f = fixture("refused");
    let r = call(
        &f.client,
        Some(&f.caller),
        Duration::from_secs(2),
        json!({"command": ["echo", "x"], "space": f.other_space}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("not open"), "{r}");
    let r = call(
        &f.client,
        None,
        WAIT,
        json!({"command": ["echo", "x"], "space": f.own_space}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("POIMEN_TERMINAL_ID"), "{r}");
}

#[test]
fn bad_arguments_are_tool_errors() {
    let f = fixture("bad");
    for args in [
        json!({}),
        json!({"command": "node x.mts"}),
        json!({"command": []}),
        json!({"command": [1]}),
        json!({"command": ["echo"], "env": {"A=B": "x"}}),
        json!({"command": ["echo"], "bogus": 1}),
        json!({"command": ["echo"], "space": "nope"}),
    ] {
        let r = call(&f.client, Some(&f.caller), WAIT, args.clone());
        assert_eq!(r["isError"], true, "{args}: {r}");
    }
}
