//! `grant_lost` (#1454, ADR 0018 amendment #1448) through `poimen mcp` against an in-process
//! poimen-server: a lead whose run records a grant the server does not know is refused without a
//! dialog and its run gets an attention; a grant that ended for a known reason asks as before and
//! names the reason; a lead without a run, or without a grant record, keeps the per-call question.
//! Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    AgentGrantAnswer, AgentGrantRequest, AgentGrantRevoke, AgentWriteAction, ClientInfo,
    ClientKind, Event, EventsSubscribe, TerminalCreate,
};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    /// The lead's terminal, in a space on a git repository (where its run lives).
    lead: String,
    /// Another space.
    api: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpgl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["work", "api"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    let ok = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(dir.join("work"))
        .args(["init", "-q"])
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git init");
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
    let (own, api) = (make_space("work"), make_space("api"));
    let lead = client
        .create_terminal(&TerminalCreate {
            space_id: own,
            name: Some("lead".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    Fixture {
        dir,
        client,
        lead,
        api,
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

fn call(f: &Fixture, tool: &str, args: Value) -> Value {
    let env = Env {
        terminal_id: Some(&f.lead),
        home: None,
        confirm_wait: Duration::from_secs(10),
        events_dir: Some(f.dir.join("events")),
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn ok(result: &Value) -> Value {
    assert_ne!(result["isError"], json!(true), "{result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap_or(Value::String(text.into()))
}

fn error(result: &Value) -> String {
    assert_eq!(result["isError"], json!(true), "{result}");
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

fn start_shell(f: &Fixture) -> Value {
    call(
        f,
        "start_shell",
        json!({"command": ["sleep", "20"], "space": f.api}),
    )
}

/// The lead's run, with `grant` recorded when given.
fn lead_run(f: &Fixture, grant: Option<&str>) -> String {
    let started = ok(&call(
        f,
        "record_run",
        json!({"event": "start", "title": "ship"}),
    ));
    if let Some(id) = grant {
        let g = json!({"id": id, "spaces": [f.api], "tools": ["start_shell"], "max_agents": 2});
        ok(&call(
            f,
            "record_run",
            json!({"event": "grant", "grant": g}),
        ));
    }
    started["run"].as_str().unwrap().to_owned()
}

/// The next question of an app answers `approve`.
fn answer_confirm(f: &Fixture, approve: bool) -> thread::JoinHandle<()> {
    let rx = app(&f.client);
    let client = f.client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if let Event::AgentConfirmRequested { id, .. } = rx.recv_timeout(left).unwrap() {
                client.answer_agent_confirm(&id, approve).unwrap();
                return;
            }
        }
    })
}

/// A grant over the api space confirmed in the app, `duration_s` long.
fn real_grant(f: &Fixture, duration_s: Option<u64>) -> String {
    let rx = app(&f.client);
    let client = f.client.clone();
    let answered = thread::spawn(move || {
        loop {
            let e = rx.recv_timeout(Duration::from_secs(20)).unwrap();
            if let Event::AgentGrantRequested { id, .. } = e {
                let yes = AgentGrantAnswer {
                    id,
                    approve: true,
                    spaces: None,
                    max_agents: None,
                    worktrees: None,
                };
                client.answer_agent_grant(&yes).unwrap();
                return;
            }
        }
    });
    let request = AgentGrantRequest {
        caller_terminal_id: f.lead.clone(),
        plan: "ship".into(),
        spaces: vec![f.api.clone()],
        tools: vec![AgentWriteAction::StartShell],
        max_agents: 2,
        worktrees: false,
        duration_s,
        wait_ms: None,
    };
    let result = f
        .client
        .request_agent_grant(&request, Duration::from_secs(30))
        .unwrap();
    answered.join().unwrap();
    result.grant.expect("granted").id
}

/// The attentions of the lead run `run` with the id.
fn attentions(f: &Fixture, run: &str, id: &str) -> usize {
    let events = ok(&call(
        f,
        "get_events",
        json!({"run": run, "repository": f.dir.join("work"), "type": "attention"}),
    ));
    let list = events["events"].as_array().cloned().unwrap_or_default();
    list.iter().filter(|e| e["id"] == id).count()
}

#[test]
fn a_lost_grant_refuses_without_a_dialog_and_raises_one_attention() {
    let f = fixture("lost");
    let run = lead_run(&f, Some("grant9-0000000000000000"));
    let seen = app(&f.client);
    let first = error(&start_shell(&f));
    assert!(first.starts_with("grant_lost"), "{first}");
    assert!(first.contains("request_grant"), "{first}");
    let second = error(&start_shell(&f));
    assert!(second.starts_with("grant_lost"), "{second}");
    // A marker after both calls: every event before it was published during them.
    let marker = f.dir.join("api").display().to_string();
    f.client.create_space("marker", &marker).unwrap();
    loop {
        match seen.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::AgentConfirmRequested { .. } => panic!("a dialog was shown"),
            Event::SpaceCreated { .. } => break,
            _ => {}
        }
    }
    assert!(
        f.client.list_terminals(None).unwrap().len() == 1,
        "nothing started"
    );

    // A write in the lead's own space never needed a grant or a dialog: it stays free.
    let own = call(&f, "start_shell", json!({"command": ["sleep", "20"]}));
    assert!(ok(&own)["terminal"].is_string());

    let attention = "grant-lost-grant9-0000000000000000";
    assert_eq!(attentions(&f, &run, attention), 1, "raised once per grant");
    let read = ok(&call(&f, "read_run", json!({})));
    let open = read["state"]["attentions"].as_array().cloned();
    let open = open.unwrap_or_default();
    assert!(open.iter().any(|a| a["id"] == attention), "{read}");
}

#[test]
fn a_revoked_or_expired_grant_asks_and_names_the_reason() {
    let f = fixture("ended");
    let id = real_grant(&f, None);
    lead_run(&f, Some(&id));
    let revoke = AgentGrantRevoke {
        id: Some(id.clone()),
        caller_terminal_id: None,
    };
    assert!(f.client.revoke_agent_grant(&revoke).unwrap());
    let asked = answer_confirm(&f, false);
    let refused = error(&start_shell(&f));
    asked.join().unwrap();
    assert!(refused.contains("declined"), "{refused}");
    assert!(
        refused.contains(&format!("{id} ended (revoked)")),
        "{refused}"
    );
    assert!(!refused.contains("grant_lost"), "{refused}");
    // A yes still writes.
    let asked = answer_confirm(&f, true);
    assert!(ok(&start_shell(&f))["terminal"].is_string());
    asked.join().unwrap();

    let short = real_grant(&f, Some(1));
    let g = json!({"id": short, "spaces": [f.api], "tools": ["start_shell"], "max_agents": 2});
    ok(&call(
        &f,
        "record_run",
        json!({"event": "grant", "grant": g}),
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.client.agent_grants().unwrap().grants.is_empty() {
        assert!(Instant::now() < deadline, "the grant expires");
        thread::park_timeout(Duration::from_millis(100));
    }
    let asked = answer_confirm(&f, false);
    let refused = error(&start_shell(&f));
    asked.join().unwrap();
    assert!(
        refused.contains(&format!("{short} ended (expired)")),
        "{refused}"
    );
}

#[test]
fn a_lead_without_a_run_or_a_grant_record_keeps_the_question() {
    let f = fixture("no-run");
    let asked = answer_confirm(&f, true);
    assert!(ok(&start_shell(&f))["terminal"].is_string());
    asked.join().unwrap();
    lead_run(&f, None);
    let asked = answer_confirm(&f, false);
    let refused = error(&start_shell(&f));
    asked.join().unwrap();
    assert!(
        refused.starts_with("refused: the user declined"),
        "{refused}"
    );
}

/// Makes `dir`, its subdirectories and files read-only, or writable again.
fn set_writable(dir: &std::path::Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(dir).unwrap();
    let mode = match (meta.is_dir(), writable) {
        (true, true) => 0o755,
        (true, false) => 0o555,
        (false, true) => 0o644,
        (false, false) => 0o444,
    };
    if meta.is_dir() && writable {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    if meta.is_dir() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            set_writable(&e.path(), writable);
        }
    }
    if !(meta.is_dir() && writable) {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

#[test]
fn an_attention_that_cannot_be_written_leaves_the_refusal_standing() {
    let f = fixture("unwritable");
    lead_run(&f, Some("grant9-1111111111111111"));
    let events = f.dir.join("events");
    set_writable(&events, false);
    let refused = error(&start_shell(&f));
    set_writable(&events, true);
    assert!(refused.starts_with("grant_lost"), "{refused}");
    assert!(refused.contains("was not written"), "{refused}");
    assert_eq!(
        f.client.list_terminals(None).unwrap().len(),
        1,
        "nothing started"
    );
}

#[test]
fn a_worktree_start_with_a_lost_grant_is_refused_before_anything_is_made() {
    let f = fixture("worktree");
    lead_run(&f, Some("grant9-2222222222222222"));
    let spaces = f.client.list_spaces().unwrap().len();
    let r = call(
        &f,
        "start_agent",
        json!({"harness": "claude", "worktree": {"branch": "w1"}}),
    );
    let refused = error(&r);
    assert!(refused.starts_with("grant_lost"), "{refused}");
    assert_eq!(
        f.client.list_spaces().unwrap().len(),
        spaces,
        "no space made"
    );
    let branches = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(f.dir.join("work"))
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&branches.stdout).into_owned();
    assert!(!list.contains("w1"), "no worktree made: {list}");
}
