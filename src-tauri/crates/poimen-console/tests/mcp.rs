//! `poimen mcp` against an in-process poimen-server on a temporary socket (never the user's
//! server). Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_console::mcp;
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    space: String,
    terminal: String,
    work: PathBuf,
}

impl Fixture {
    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
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
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space.clone(),
            name: Some("shell".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 30".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": terminal, "seq": 1, "status": AgentStatus::Working,
                "agent": "claude"}),
        )
        .unwrap();
    client
        .request(
            "terminal.report_tokens",
            json!({"terminal_id": terminal, "tokens": {"ctx": "42%"}, "source": "t"}),
        )
        .unwrap();
    Fixture {
        dir,
        client,
        space,
        terminal,
        work,
    }
}

/// Sends the messages (one per line) and returns the replies.
fn session(client: &Client, terminal: Option<&str>, messages: &[Value]) -> Vec<Value> {
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let mut out = Vec::new();
    mcp::serve(client, terminal, Cursor::new(input), &mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn call(client: &Client, terminal: Option<&str>, name: &str, args: Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    session(client, terminal, &[msg]).remove(0)["result"].clone()
}

fn offline_client() -> Client {
    Client::new(std::env::temp_dir().join(format!("poimen-mcp-none-{}.sock", std::process::id())))
}

#[test]
fn initialize_and_tools_list() {
    let client = offline_client();
    let replies = session(
        &client,
        None,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-03-26", "capabilities": {}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}),
        ],
    );
    // The notification gets no answer.
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(
        replies[0]["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    let names: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "list_spaces",
            "list_terminals",
            "whoami",
            "list_runs",
            "start_agent",
            "prompt_agent",
            "wait_agent",
            "report_done",
            "read_agent"
        ]
    );
    assert_eq!(replies[2]["error"]["code"], -32601);
}

#[test]
fn garbage_input_is_an_error_reply_not_a_crash() {
    let mut out = Vec::new();
    mcp::serve(
        &offline_client(),
        None,
        Cursor::new("not json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n"),
        &mut out,
    )
    .unwrap();
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["error"]["code"], -32700);
    assert_eq!(lines[1]["id"], 7);
}

#[test]
fn list_spaces_and_terminals() {
    let f = fixture("lists");
    let r = call(&f.client, None, "list_spaces", json!({}));
    let space = &r["structuredContent"]["spaces"][0];
    assert_eq!(space["id"], f.space.as_str());
    assert_eq!(space["name"], "work");
    assert!(space["directory"].as_str().unwrap().ends_with("work"));

    let r = call(&f.client, None, "list_terminals", json!({"space": f.space}));
    let t = &r["structuredContent"]["terminals"][0];
    assert_eq!(t["id"], f.terminal.as_str());
    assert_eq!(t["space"], f.space.as_str());
    assert_eq!(t["name"], "shell");
    assert_eq!(t["agent"]["kind"], "claude");
    assert_eq!(t["agent"]["status"], "working");
    assert_eq!(t["tokens"]["ctx"], "42%");

    let r = call(&f.client, None, "list_terminals", json!({"space": "nope"}));
    assert_eq!(
        r["structuredContent"]["terminals"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn whoami_inside_and_outside() {
    let f = fixture("whoami");
    let r = call(&f.client, Some(&f.terminal), "whoami", json!({}));
    let v = &r["structuredContent"];
    assert_eq!(v["in_poimen"], true);
    assert_eq!(v["terminal"]["id"], f.terminal.as_str());
    assert_eq!(v["space"]["id"], f.space.as_str());
    assert_eq!(v["space"]["name"], "work");

    for outside in [None, Some("")] {
        let r = call(&f.client, outside, "whoami", json!({}));
        assert_eq!(r["structuredContent"], json!({"in_poimen": false}));
        assert!(r.get("isError").is_none());
    }
}

#[test]
fn errors_are_tool_errors() {
    let f = fixture("errors");
    let r = call(&f.client, Some("t999"), "whoami", json!({}));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"].as_str().unwrap().contains("t999"));

    // No server on the socket.
    let r = call(&offline_client(), None, "list_spaces", json!({}));
    assert_eq!(r["isError"], true);
    assert!(
        r["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("poimen-server:")
    );

    let r = call(&f.client, None, "list_runs", json!({}));
    assert_eq!(r["isError"], true);

    // An unknown tool is a protocol error.
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "nope"}});
    assert_eq!(session(&f.client, None, &[msg])[0]["error"]["code"], -32602);
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn list_runs_reads_the_agent_events_of_a_repository() {
    let f = fixture("runs");
    let repo = f.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let events = repo.join(".git/.agents/events");
    std::fs::create_dir_all(&events).unwrap();
    let lines = [
        json!({"v": 1, "seq": 1, "ts": "2026-01-01T00:00:00Z", "type": "run.started",
            "run": "r1", "subject": {"kind": "github.milestone", "id": "M1"}}),
        json!({"v": 1, "seq": 2, "ts": "2026-01-01T00:01:00Z", "type": "run.finished",
            "run": "r1", "outcome": "succeeded"}),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(events.join("r1.jsonl"), text).unwrap();
    // A half-written line and a foreign file are ignored.
    std::fs::write(events.join("notes.txt"), "x").unwrap();

    let r = call(
        &f.client,
        None,
        "list_runs",
        json!({"repository": repo.display().to_string()}),
    );
    assert!(r.get("isError").is_none(), "{r}");
    let runs = r["structuredContent"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["run"], "r1");
    assert_eq!(runs[0]["status"], "finished");
    assert_eq!(runs[0]["outcome"], "succeeded");

    // A repository without events: an empty list.
    let empty = f.dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q"]);
    let r = call(
        &f.client,
        None,
        "list_runs",
        json!({"repository": empty.display().to_string()}),
    );
    assert_eq!(r["structuredContent"]["runs"], json!([]));

    // Outside a repository: a tool error.
    let plain = f.dir.join("work");
    let r = call(
        &f.client,
        None,
        "list_runs",
        json!({"repository": plain.display().to_string()}),
    );
    assert_eq!(r["isError"], true);
}

fn report(client: &Client, terminal: &str, seq: u64, status: &str, message: Option<&str>) {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": terminal, "seq": seq, "status": status, "agent": "claude",
                "session_id": "s-1", "message": message}),
        )
        .unwrap();
}

fn tool_text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn wait_agent_returns_at_once_for_a_finished_or_blocked_agent() {
    let f = fixture("wait-final");
    report(&f.client, &f.terminal, 2, "done", None);
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["structuredContent"]["status"], "done");
    assert_eq!(r["structuredContent"]["timed_out"], false);

    report(&f.client, &f.terminal, 3, "blocked", Some("Allow rm -rf?"));
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["structuredContent"]["status"], "blocked");
    assert_eq!(r["structuredContent"]["message"], "Allow rm -rf?");
}

#[test]
fn wait_agent_times_out_with_the_current_status() {
    let f = fixture("wait-timeout");
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal, "timeout_s": 0.3}),
    );
    assert_eq!(r["structuredContent"]["status"], "working");
    assert_eq!(r["structuredContent"]["timed_out"], true);
}

#[test]
fn wait_agent_wakes_on_the_status_event() {
    let f = fixture("wait-event");
    // Reports `done` (a new seq each time) until the wait returned, so one lands after the wait
    // subscribed whatever the interleaving.
    let stop = Arc::new(AtomicBool::new(false));
    let reporter = {
        let (client, terminal, stop) = (f.client.clone(), f.terminal.clone(), stop.clone());
        let seq = AtomicU64::new(10);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let n = seq.fetch_add(2, Ordering::Relaxed);
                report(&client, &terminal, n, "working", None);
                report(&client, &terminal, n + 1, "done", None);
            }
        })
    };
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal, "timeout_s": 20}),
    );
    stop.store(true, Ordering::Relaxed);
    reporter.join().unwrap();
    assert_eq!(r["structuredContent"]["status"], "done");
    assert_eq!(r["structuredContent"]["timed_out"], false);
}

#[test]
fn wait_agent_reports_a_gone_agent_and_an_unknown_terminal_as_an_error() {
    let f = fixture("wait-gone");
    let stop = Arc::new(AtomicBool::new(false));
    let reporter = {
        let (client, terminal, stop) = (f.client.clone(), f.terminal.clone(), stop.clone());
        thread::spawn(move || {
            let mut seq = 10;
            while !stop.load(Ordering::Relaxed) {
                report(&client, &terminal, seq, "gone", None);
                seq += 1;
            }
        })
    };
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal, "timeout_s": 20}),
    );
    stop.store(true, Ordering::Relaxed);
    reporter.join().unwrap();
    assert_eq!(r["structuredContent"]["status"], "gone");

    let r = call(&f.client, None, "wait_agent", json!({"terminal": "nope"}));
    assert_eq!(r["isError"], true);
    assert!(tool_text(&r).contains("not found"));
    let r = call(&f.client, None, "wait_agent", json!({}));
    assert_eq!(r["isError"], true);
}

fn transcript_of(f: &Fixture, session: &str) -> std::fs::File {
    let dir = poimen_core::transcripts::projects_root(&f.home()).join(
        poimen_core::transcripts::project_dir_name(&f.work.display().to_string()),
    );
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{session}.jsonl")))
        .unwrap()
}

#[test]
fn read_agent_returns_the_last_assistant_message_with_the_session() {
    let f = fixture("read");
    let mut file = transcript_of(&f, "s-1");
    for line in [
        r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"Do it."}}"#,
        r#"{"type":"assistant","uuid":"a1","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"First answer."}]}}"#,
        r#"{"type":"assistant","uuid":"a2","message":{"id":"m2","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
        r#"{"type":"assistant","uuid":"a3","message":{"id":"m3","role":"assistant","content":[{"type":"text","text":"All done."}]}}"#,
    ] {
        writeln!(file, "{line}").unwrap();
    }
    report(&f.client, &f.terminal, 2, "done", None);
    let r = call(
        &f.client,
        None,
        "read_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["structuredContent"]["message"], "All done.");
    assert_eq!(r["structuredContent"]["session"], "s-1");
    // A second read gives the same answer (it does not consume the transcript).
    let r = call(
        &f.client,
        None,
        "read_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["structuredContent"]["message"], "All done.");
}

#[test]
fn read_agent_without_a_transcript_is_a_tool_error_that_says_so() {
    let f = fixture("read-none");
    // The agent reported no session: no transcript.
    f.client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": f.terminal, "seq": 5, "status": "done", "agent": "claude"}),
        )
        .unwrap();
    let r = call(
        &f.client,
        None,
        "read_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["isError"], true);
    assert!(tool_text(&r).contains("no agent with a readable transcript"));

    // A session whose file is not there yet.
    report(&f.client, &f.terminal, 6, "done", None);
    let r = call(
        &f.client,
        None,
        "read_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["isError"], true);
    assert!(tool_text(&r).contains("not there yet"));

    // A transcript without an assistant message.
    let mut file = transcript_of(&f, "s-1");
    writeln!(
        file,
        r#"{{"type":"user","uuid":"u1","message":{{"role":"user","content":"hi"}}}}"#
    )
    .unwrap();
    let r = call(
        &f.client,
        None,
        "read_agent",
        json!({"terminal": f.terminal}),
    );
    assert_eq!(r["isError"], true);
    assert!(tool_text(&r).contains("no assistant message"));

    let r = call(&f.client, None, "read_agent", json!({"terminal": "nope"}));
    assert_eq!(r["isError"], true);
}

#[test]
fn the_read_tools_need_no_consent_from_another_space() {
    // No calling terminal at all: a write would be refused, these read.
    let f = fixture("read-any-space");
    report(&f.client, &f.terminal, 2, "done", None);
    let r = call(
        &f.client,
        None,
        "wait_agent",
        json!({"terminal": f.terminal}),
    );
    assert!(r.get("isError").is_none());
    let _ = &f.space;
}
