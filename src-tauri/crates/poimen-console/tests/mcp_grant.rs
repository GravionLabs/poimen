//! The scoped write grant (#1211, ADR 0018) through `poimen mcp` against an in-process
//! poimen-server, with a stand-in for the app. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    AgentGrantAnswer, AgentGrantRequest, AgentGrantRevoke, AgentStatus, AgentWriteAction,
    ClientInfo, ClientKind, ConfirmReason, Event, EventsSubscribe, TerminalCreate,
};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    /// The lead's space and terminal.
    own: String,
    lead: String,
    /// Two other spaces.
    api: String,
    web: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["work", "api", "web"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
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
    let (own, api, web) = (make_space("work"), make_space("api"), make_space("web"));
    let lead = sleeper(&client, &own, "lead", None);
    Fixture {
        dir,
        client,
        own,
        lead,
        api,
        web,
    }
}

fn sleeper(client: &Client, space: &str, name: &str, by: Option<&str>) -> String {
    client
        .create_terminal(&TerminalCreate {
            space_id: space.into(),
            name: Some(name.into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            started_by: by.map(str::to_owned),
            ..Default::default()
        })
        .unwrap()
        .id
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

const WAIT: Duration = Duration::from_secs(30);

fn call(f: &Fixture, terminal: &str, tool: &str, args: Value, wait: Duration) -> Value {
    let env = Env {
        terminal_id: Some(terminal),
        home: None,
        confirm_wait: wait,
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

/// What the app does with the next grant request.
fn answer_grant(
    f: &Fixture,
    rx: Receiver<Event>,
    answer: AgentGrantAnswer,
) -> thread::JoinHandle<Event> {
    let client = f.client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let e = rx.recv_timeout(left).expect("the grant request");
            if let Event::AgentGrantRequested { ref id, .. } = e {
                client
                    .answer_agent_grant(&AgentGrantAnswer {
                        id: id.clone(),
                        ..answer
                    })
                    .unwrap();
                return e;
            }
        }
    })
}

fn yes() -> AgentGrantAnswer {
    AgentGrantAnswer {
        id: String::new(),
        approve: true,
        spaces: None,
        max_agents: None,
    }
}

/// The lead's grant over `spaces` for the tools, confirmed in the app; returns the grant id and
/// the events the app sees afterwards.
fn grant(f: &Fixture, spaces: &[&str], tools: &[&str], max: u64) -> (String, Receiver<Event>) {
    let rx = app(&f.client);
    let seen = app(&f.client);
    let asked = answer_grant(f, rx, yes());
    let r = call(
        f,
        &f.lead,
        "request_grant",
        json!({"plan": "ship it", "spaces": spaces, "tools": tools, "max_agents": max}),
        WAIT,
    );
    asked.join().unwrap();
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["granted"], true, "{r}");
    let id = r["structuredContent"]["grant"]["id"].as_str().unwrap();
    (id.to_owned(), seen)
}

fn start_shell(f: &Fixture, from: &str, space: &str) -> Value {
    call(
        f,
        from,
        "start_shell",
        json!({"command": ["sleep", "20"], "space": space}),
        Duration::from_secs(5),
    )
}

/// The next question of an app that answers `approve` once; `None` when none came.
fn answer_confirm(f: &Fixture, approve: bool) -> thread::JoinHandle<Event> {
    let rx = app(&f.client);
    let client = f.client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
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

fn no_question(rx: &Receiver<Event>) {
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }), "{e:?}");
    }
}

#[test]
fn a_write_inside_the_grant_needs_no_dialog_and_is_announced() {
    let f = fixture("inside");
    let (_, seen) = grant(&f, &[&f.api, &f.web], &["start_shell"], 2);
    let r = start_shell(&f, &f.lead, &f.api);
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["confirmed"], Value::Null, "{r}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let e = seen
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("agent.wrote");
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }), "{e:?}");
        if let Event::AgentWrote {
            action,
            space_id,
            confirmed,
            ..
        } = e
        {
            assert_eq!(
                (action, space_id, confirmed),
                (AgentWriteAction::StartShell, f.api.clone(), false)
            );
            break;
        }
    }
}

#[test]
fn a_write_outside_the_grant_asks_per_call() {
    let f = fixture("outside");
    let (_, seen) = grant(&f, &[&f.api], &["start_shell"], 2);
    // Another space.
    let asked = answer_confirm(&f, true);
    let r = start_shell(&f, &f.lead, &f.web);
    assert_eq!(r["isError"], Value::Null, "{r}");
    asked.join().unwrap();
    // Another tool in a granted space.
    let asked = answer_confirm(&f, false);
    let target = sleeper(&f.client, &f.api, "worker", Some(&f.lead));
    let r = call(
        &f,
        &f.lead,
        "stop_terminal",
        json!({"terminal": target, "mode": "close"}),
        Duration::from_secs(5),
    );
    asked.join().unwrap();
    assert_eq!(r["isError"], true, "{r}");
    drop(seen);
}

#[test]
fn write_terminal_is_never_covered() {
    let f = fixture("write-terminal");
    // The tool cannot even be put in a grant.
    let r = call(
        &f,
        &f.lead,
        "request_grant",
        json!({"plan": "p", "spaces": [f.api], "tools": ["write_terminal"], "max_agents": 1}),
        WAIT,
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("write_terminal"), "{r}");
}

#[test]
fn another_terminal_cannot_use_the_grant() {
    let f = fixture("other-terminal");
    grant(&f, &[&f.api], &["start_shell"], 2);
    let other = sleeper(&f.client, &f.own, "other", None);
    let asked = answer_confirm(&f, false);
    let r = start_shell(&f, &other, &f.api);
    asked.join().unwrap();
    assert_eq!(r["isError"], true, "{r}");
}

fn working_agent(f: &Fixture, space: &str, name: &str, seq: u64) -> String {
    let id = sleeper(&f.client, space, name, Some(&f.lead));
    report(f, &id, seq, AgentStatus::Working);
    id
}

fn report(f: &Fixture, id: &str, seq: u64, status: AgentStatus) {
    f.client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": id, "seq": seq, "status": status, "agent": "claude"}),
        )
        .unwrap();
}

#[test]
fn a_start_over_the_limit_is_refused_with_the_reason_and_not_asked() {
    let f = fixture("limit");
    let (_, seen) = grant(&f, &[&f.api], &["start_agent"], 2);
    let first = working_agent(&f, &f.api, "a1", 1);
    working_agent(&f, &f.api, "a2", 1);
    let info = &f.client.agent_grants().unwrap().grants[0];
    assert_eq!((info.running, info.max_agents), (2, 2));
    let r = call(
        &f,
        &f.lead,
        "start_agent",
        json!({"space": f.api, "harness": "claude"}),
        Duration::from_secs(5),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("allows 2 agents"), "{}", text(&r));
    no_question(&seen);
    // One that is done frees a place: the start is no longer refused for the limit.
    report(&f, &first, 2, AgentStatus::Done);
    assert_eq!(f.client.agent_grants().unwrap().grants[0].running, 1);
}

#[test]
fn a_revoked_grant_falls_back_to_the_question() {
    let f = fixture("revoked");
    let (id, _) = grant(&f, &[&f.api], &["start_shell"], 2);
    assert!(
        f.client
            .revoke_agent_grant(&AgentGrantRevoke {
                id: Some(id),
                caller_terminal_id: None
            })
            .unwrap()
    );
    let asked = answer_confirm(&f, true);
    let r = start_shell(&f, &f.lead, &f.api);
    asked.join().unwrap();
    assert_eq!(r["isError"], Value::Null, "{r}");
    // The lead revokes its own grant with the tool; there is none left now.
    let r = call(&f, &f.lead, "revoke_grant", json!({}), WAIT);
    assert_eq!(r["structuredContent"]["revoked"], false, "{r}");
}

#[test]
fn the_lead_revokes_its_own_grant_and_running_agents_stay() {
    let f = fixture("lead-revokes");
    grant(&f, &[&f.api], &["start_shell"], 2);
    let worker = working_agent(&f, &f.api, "w", 1);
    let r = call(&f, &f.lead, "revoke_grant", json!({}), WAIT);
    assert_eq!(r["structuredContent"]["revoked"], true, "{r}");
    assert!(f.client.agent_grants().unwrap().grants.is_empty());
    let alive = f.client.list_terminals(None).unwrap();
    assert!(alive.iter().any(|t| t.id == worker));
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "{what}");
        thread::park_timeout(Duration::from_millis(100));
    }
}

#[test]
fn an_expired_grant_is_gone_and_asks_again() {
    let f = fixture("expired");
    let rx = app(&f.client);
    let asked = answer_grant(&f, rx, yes());
    let result = f
        .client
        .request_agent_grant(
            &AgentGrantRequest {
                caller_terminal_id: f.lead.clone(),
                plan: "short".into(),
                spaces: vec![f.api.clone()],
                tools: vec![AgentWriteAction::StartShell],
                max_agents: 1,
                duration_s: Some(1),
                wait_ms: None,
            },
            WAIT,
        )
        .unwrap();
    asked.join().unwrap();
    assert!(result.granted);
    until("the grant expires", || {
        f.client.agent_grants().unwrap().grants.is_empty()
    });
    let asked = answer_confirm(&f, true);
    let r = start_shell(&f, &f.lead, &f.api);
    asked.join().unwrap();
    assert_eq!(r["isError"], Value::Null, "{r}");
}

#[test]
fn a_grant_ends_with_the_lead_terminal() {
    let f = fixture("lead-gone");
    grant(&f, &[&f.api], &["start_shell"], 1);
    f.client.close_terminal(&f.lead).unwrap();
    until("the grant ends with its lead", || {
        f.client.agent_grants().unwrap().grants.is_empty()
    });
}

#[test]
fn the_user_can_narrow_but_not_widen() {
    let f = fixture("narrow");
    let rx = app(&f.client);
    let client = f.client.clone();
    let (api, web) = (f.api.clone(), f.web.clone());
    let answered = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let e = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("the request");
            if let Event::AgentGrantRequested { ref id, .. } = e {
                let wider = client.answer_agent_grant(&AgentGrantAnswer {
                    id: id.clone(),
                    approve: true,
                    spaces: Some(vec![api.clone(), "s-unasked".into()]),
                    max_agents: None,
                });
                assert!(wider.is_err(), "a space that was not asked for");
                let higher = client.answer_agent_grant(&AgentGrantAnswer {
                    id: id.clone(),
                    approve: true,
                    spaces: None,
                    max_agents: Some(9),
                });
                assert!(higher.is_err(), "a higher limit");
                client
                    .answer_agent_grant(&AgentGrantAnswer {
                        id: id.clone(),
                        approve: true,
                        spaces: Some(vec![api]),
                        max_agents: Some(1),
                    })
                    .unwrap();
                return web;
            }
        }
    });
    let r = call(
        &f,
        &f.lead,
        "request_grant",
        json!({"plan": "p", "spaces": [f.api, f.web], "tools": ["start_shell"], "max_agents": 3}),
        WAIT,
    );
    answered.join().unwrap();
    let granted = &r["structuredContent"]["grant"];
    assert_eq!(granted["max_agents"], 1, "{r}");
    assert_eq!(granted["spaces"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(granted["spaces"][0]["id"], f.api.as_str(), "{r}");
    // The space the user took out asks per call.
    let asked = answer_confirm(&f, false);
    let r = start_shell(&f, &f.lead, &f.web);
    asked.join().unwrap();
    assert_eq!(r["isError"], true, "{r}");
}

#[test]
fn a_decline_a_timeout_and_no_app_all_mean_no() {
    let f = fixture("no");
    let rx = app(&f.client);
    let declined = answer_grant(
        &f,
        rx,
        AgentGrantAnswer {
            approve: false,
            ..yes()
        },
    );
    let ask = json!({"plan": "p", "spaces": [f.api], "tools": ["start_shell"], "max_agents": 1});
    let r = call(&f, &f.lead, "request_grant", ask.clone(), WAIT);
    declined.join().unwrap();
    assert_eq!(r["structuredContent"]["granted"], false, "{r}");
    assert!(f.client.agent_grants().unwrap().grants.is_empty());
    // An app that never answers: a short wait counts as no.
    let silent = app(&f.client);
    let r = call(
        &f,
        &f.lead,
        "request_grant",
        ask.clone(),
        Duration::from_millis(300),
    );
    assert_eq!(r["structuredContent"]["granted"], false, "{r}");
    assert!(
        r["structuredContent"]["note"]
            .as_str()
            .unwrap()
            .contains("in time"),
        "{r}"
    );
    drop(silent);
}

#[test]
fn without_an_app_there_is_no_grant() {
    let f = fixture("no-app");
    let result = f
        .client
        .request_agent_grant(
            &AgentGrantRequest {
                caller_terminal_id: f.lead.clone(),
                plan: "p".into(),
                spaces: vec![f.api.clone()],
                tools: vec![AgentWriteAction::StartShell],
                max_agents: 1,
                duration_s: None,
                wait_ms: None,
            },
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        (result.granted, result.reason),
        (false, ConfirmReason::NoApp)
    );
    let r = call(
        &f,
        &f.lead,
        "request_grant",
        json!({"plan": "p", "spaces": ["nope"], "tools": ["start_shell"], "max_agents": 1}),
        WAIT,
    );
    assert_eq!(r["isError"], true, "an unknown space id is an error: {r}");
}
