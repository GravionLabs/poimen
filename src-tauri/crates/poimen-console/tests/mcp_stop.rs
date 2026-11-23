//! `stop_terminal` (#1183) against an in-process poimen-server on a temporary socket. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    ClientInfo, ClientKind, Event, EventsSubscribe, Terminal, TerminalCreate, TerminalStatus,
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
    let dir = std::env::temp_dir().join(format!("poimen-mcpx-{}-{name}", std::process::id()));
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
        "params": {"name": "stop_terminal", "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

fn pause() {
    thread::park_timeout(Duration::from_millis(100));
}

const WAIT: Duration = Duration::from_secs(30);

fn start(f: &Fixture, space: &str, script: &str) -> String {
    f.client
        .create_terminal(&TerminalCreate {
            space_id: space.into(),
            command: Some(vec!["sh".into(), "-c".into(), script.into()]),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn find(f: &Fixture, id: &str) -> Option<Terminal> {
    f.client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
}

/// Polls until the terminal reports an agent.
fn has_agent(f: &Fixture, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while find(f, id).is_some_and(|t| t.agent.is_none()) {
        assert!(Instant::now() < deadline, "the agent report arrives");
        pause();
    }
}

const LOOP: &str = "trap 'exit 0' INT; echo ready; while :; do sleep 0.05; done";

#[test]
fn interrupt_ends_a_process_that_takes_ctrl_c_in_the_own_space() {
    let f = fixture("int");
    let rx = app(&f.client);
    let id = start(&f, &f.own_space, LOOP);
    let r = call(&f.client, Some(&f.caller), WAIT, json!({"terminal": id}));
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["outcome"], "ended", "{r}");
    assert_eq!(r["structuredContent"]["mode"], "interrupt");
    // The pane stays, exited.
    assert_eq!(find(&f, &id).unwrap().status, TerminalStatus::Exited);
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }));
    }
    // A second call finds it gone.
    let r = call(&f.client, Some(&f.caller), WAIT, json!({"terminal": id}));
    assert_eq!(r["structuredContent"]["outcome"], "gone", "{r}");
}

#[test]
fn a_process_that_ignores_ctrl_c_still_runs_and_close_ends_it() {
    let f = fixture("ign");
    let id = start(
        &f,
        &f.own_space,
        "trap '' INT; while :; do sleep 0.05; done",
    );
    let r = call(&f.client, Some(&f.caller), WAIT, json!({"terminal": id}));
    assert_eq!(r["structuredContent"]["outcome"], "running", "{r}");
    assert_eq!(find(&f, &id).unwrap().status, TerminalStatus::Running);
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "mode": "close"}),
    );
    assert_eq!(r["structuredContent"]["outcome"], "ended", "{r}");
    assert!(find(&f, &id).is_none(), "the pane is closed");
}

#[test]
fn close_of_an_unknown_terminal_is_gone() {
    let f = fixture("unknown");
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": "nope", "mode": "close"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["outcome"], "gone");
}

#[test]
fn close_of_a_terminal_with_an_agent_needs_force() {
    let f = fixture("force");
    let id = start(
        &f,
        &f.own_space,
        r"printf '\033]7501;state=working:app=loop\007'; sleep 60",
    );
    has_agent(&f, &id);
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "mode": "close"}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("force"), "{r}");
    assert!(find(&f, &id).is_some(), "nothing was closed");
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "mode": "close", "force": true}),
    );
    assert_eq!(r["structuredContent"]["outcome"], "ended", "{r}");
    assert!(find(&f, &id).is_none());
}

fn answer_once(f: &Fixture, approve: bool) -> thread::JoinHandle<Event> {
    let rx = app(&f.client);
    let client = f.client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let e = rx.recv_timeout(left).expect("the question");
            if let Event::AgentConfirmRequested { ref id, .. } = e {
                client.answer_agent_confirm(id, approve).unwrap();
                return e;
            }
        }
    })
}

#[test]
fn another_space_needs_the_user_and_a_decline_stops_nothing() {
    let f = fixture("decline");
    let id = start(&f, &f.other_space, LOOP);
    let asked = answer_once(&f, false);
    let r = call(&f.client, Some(&f.caller), WAIT, json!({"terminal": id}));
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("declined"), "{r}");
    asked.join().unwrap();
    assert_eq!(find(&f, &id).unwrap().status, TerminalStatus::Running);
}

#[test]
fn another_space_stops_after_approval() {
    let f = fixture("approve");
    let id = start(&f, &f.other_space, LOOP);
    let asked = answer_once(&f, true);
    let r = call(&f.client, Some(&f.caller), WAIT, json!({"terminal": id}));
    assert_eq!(r["structuredContent"]["outcome"], "ended", "{r}");
    let Event::AgentConfirmRequested { text, .. } = asked.join().unwrap() else {
        unreachable!()
    };
    assert_eq!(text, "interrupt");
}

#[test]
fn refused_without_an_app_or_outside_a_poimen_terminal() {
    let f = fixture("refused");
    let id = start(&f, &f.other_space, LOOP);
    let r = call(
        &f.client,
        Some(&f.caller),
        Duration::from_secs(2),
        json!({"terminal": id}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("not open"), "{r}");
    let r = call(&f.client, None, WAIT, json!({"terminal": id}));
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("POIMEN_TERMINAL_ID"), "{r}");
    assert_eq!(find(&f, &id).unwrap().status, TerminalStatus::Running);
}

#[test]
fn bad_arguments_are_tool_errors() {
    let f = fixture("bad");
    for args in [
        json!({}),
        json!({"terminal": f.caller, "mode": "kill"}),
        json!({"terminal": f.caller, "force": "yes"}),
        json!({"terminal": f.caller, "bogus": 1}),
    ] {
        let r = call(&f.client, Some(&f.caller), WAIT, args.clone());
        assert_eq!(r["isError"], true, "{args}: {r}");
    }
}
