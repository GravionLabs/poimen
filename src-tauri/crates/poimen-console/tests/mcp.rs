//! `poimen mcp` against an in-process poimen-server on a temporary socket (never the user's
//! server). Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
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
        home: None,
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
        ["list_spaces", "list_terminals", "whoami", "list_runs"]
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
