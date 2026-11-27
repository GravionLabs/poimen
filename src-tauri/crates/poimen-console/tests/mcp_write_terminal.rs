//! `write_terminal` (#1184) against an in-process poimen-server on a temporary socket. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
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
    let dir = std::env::temp_dir().join(format!("poimen-mcpw-{}-{name}", std::process::id()));
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
        events_dir: None,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "write_terminal", "arguments": args}});
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

fn start(f: &Fixture, space: &str, by: Option<&str>) -> String {
    f.client
        .create_terminal(&TerminalCreate {
            space_id: space.into(),
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                "while read l; do echo \"got:$l\"; done".into(),
            ]),
            started_by: by.map(str::to_owned),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn screen(f: &Fixture, id: &str) -> String {
    let env = Env {
        terminal_id: Some(&f.caller),
        home: None,
        confirm_wait: WAIT,
        events_dir: None,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_terminal", "arguments": {"terminal": id}}});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let v: Value =
        serde_json::from_str(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap();
    v["result"]["structuredContent"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn wait_for(f: &Fixture, id: &str, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !screen(f, id).contains(needle) {
        assert!(Instant::now() < deadline, "`{needle}` shows up");
        pause();
    }
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
fn a_terminal_the_caller_started_takes_text_without_a_question() {
    let f = fixture("own");
    let rx = app(&f.client);
    let id = start(&f, &f.own_space, Some(&f.caller));
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "one", "enter": true}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["confirmed"], false);
    wait_for(&f, &id, "got:one");
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }));
    }
}

#[test]
fn without_enter_the_text_is_typed_but_not_run() {
    let f = fixture("noenter");
    let id = start(&f, &f.own_space, Some(&f.caller));
    call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "abc"}),
    );
    wait_for(&f, &id, "abc");
    assert!(!screen(&f, &id).contains("got:"));
    call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "d", "enter": true}),
    );
    wait_for(&f, &id, "got:abcd");
}

#[test]
fn any_other_terminal_asks_even_in_the_own_space_and_shows_the_text() {
    let f = fixture("ask");
    let id = start(&f, &f.own_space, None);
    let asked = answer_once(&f, true);
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "two", "enter": true}),
    );
    assert_eq!(r["structuredContent"]["confirmed"], true, "{r}");
    let Event::AgentConfirmRequested { text, .. } = asked.join().unwrap() else {
        unreachable!()
    };
    assert_eq!(text, "two");
    wait_for(&f, &id, "got:two");
}

#[test]
fn a_decline_types_nothing() {
    let f = fixture("decline");
    let id = start(&f, &f.other_space, None);
    let asked = answer_once(&f, false);
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "nope", "enter": true}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("declined"), "{r}");
    asked.join().unwrap();
    pause();
    assert!(!screen(&f, &id).contains("nope"));
}

#[test]
fn refused_without_an_app_or_outside_a_poimen_terminal() {
    let f = fixture("refused");
    let id = start(&f, &f.own_space, None);
    let r = call(
        &f.client,
        Some(&f.caller),
        Duration::from_secs(2),
        json!({"terminal": id, "text": "x"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("not open"), "{r}");
    let own = start(&f, &f.own_space, Some(&f.caller));
    let r = call(&f.client, None, WAIT, json!({"terminal": own, "text": "x"}));
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("POIMEN_TERMINAL_ID"), "{r}");
}

#[test]
fn control_characters_are_refused_but_newline_and_tab_pass() {
    let f = fixture("ctrl");
    let id = start(&f, &f.own_space, Some(&f.caller));
    for bad in ["a\u{3}", "\u{1b}[A", "x\r", "\u{7f}"] {
        let r = call(
            &f.client,
            Some(&f.caller),
            WAIT,
            json!({"terminal": id, "text": bad}),
        );
        assert_eq!(r["isError"], true, "{bad:?}: {r}");
        assert!(text(&r).contains("control character"), "{r}");
    }
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "l1\nl2"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    wait_for(&f, &id, "got:l1");
}

#[test]
fn a_terminal_with_an_agent_is_refused() {
    let f = fixture("agent");
    let id = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.own_space.clone(),
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                r"printf '\033]7501;state=working:app=loop\007'; sleep 60".into(),
            ]),
            started_by: Some(f.caller.clone()),
            ..Default::default()
        })
        .unwrap()
        .id;
    let deadline = Instant::now() + Duration::from_secs(10);
    while f
        .client
        .list_terminals(None)
        .unwrap()
        .iter()
        .any(|t| t.id == id && t.agent.is_none())
    {
        assert!(Instant::now() < deadline, "the agent report arrives");
        pause();
    }
    let r = call(
        &f.client,
        Some(&f.caller),
        WAIT,
        json!({"terminal": id, "text": "x"}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("prompt_agent"), "{r}");
}

#[test]
fn bad_arguments_are_tool_errors() {
    let f = fixture("bad");
    for args in [
        json!({}),
        json!({"terminal": f.caller}),
        json!({"terminal": f.caller, "text": ""}),
        json!({"terminal": f.caller, "text": "x", "enter": "yes"}),
        json!({"terminal": f.caller, "text": "x", "bogus": 1}),
        json!({"terminal": "nope", "text": "x"}),
    ] {
        let r = call(&f.client, Some(&f.caller), WAIT, args.clone());
        assert_eq!(r["isError"], true, "{args}: {r}");
    }
}
