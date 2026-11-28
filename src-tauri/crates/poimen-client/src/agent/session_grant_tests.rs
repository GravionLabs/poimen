//! A covered worktree start is trusted only from a server whose `agent.grant_check` knows
//! `worktree` (#1452): a fake server that always answers `covered` and speaks `protocol`.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use poimen_protocol::{AgentWriteAction, GRANT_WORKTREE_PROTOCOL};
use serde_json::{Value, json};

use super::{Authorized, WriteAsk, WriteError, authorize_write_as};
use crate::Client;
use crate::test_socket::temp_socket;
use crate::transport::Listener;

/// Answers every request by method until the test ends; returns the methods it was asked.
fn fake_server(path: &Path, protocol: u32) -> Arc<Mutex<Vec<String>>> {
    let listener = Listener::bind(path).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    thread::spawn(move || {
        while let Ok(conn) = listener.accept() {
            let seen = seen.clone();
            thread::spawn(move || {
                let mut out = conn.clone();
                for line in BufReader::new(conn).lines() {
                    let Ok(line) = line else { return };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let method = req["method"].as_str().unwrap().to_owned();
                    seen.lock().unwrap().push(method.clone());
                    let result = match method.as_str() {
                        "terminal.list" => json!({"terminals": [{"id": "t1", "space_id": "s1",
                            "name": "lead", "cwd": "/w", "cols": 80, "rows": 24,
                            "status": "running"}]}),
                        "agent.grant_check" => json!({"verdict": "covered", "grant": "g1"}),
                        "ping" => json!({"version": "0.0.0", "protocol": protocol, "pid": 1}),
                        "agent.confirm" => json!({"approved": false, "reason": "declined"}),
                        _ => Value::Null,
                    };
                    let answer = json!({"id": req["id"], "result": result});
                    if writeln!(out, "{answer}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    asked
}

fn ask(worktree: bool) -> WriteAsk<'static> {
    WriteAsk {
        space_id: "s1",
        action: AgentWriteAction::StartAgent,
        terminal_id: None,
        text: "worker",
        always_ask: false,
        worktree,
    }
}

const WAIT: Duration = Duration::from_secs(2);

#[test]
fn an_older_server_covering_a_worktree_start_is_not_trusted_and_the_start_asks() {
    let (_dir, path) = temp_socket("grant-old");
    let asked = fake_server(&path, GRANT_WORKTREE_PROTOCOL - 1);
    let client = Client::new(&path);
    let r = authorize_write_as(&client, Some("t1"), &ask(true), WAIT);
    assert!(
        matches!(&r, Err(WriteError::Refused(m)) if m.contains("declined")),
        "{r:?}"
    );
    assert!(asked.lock().unwrap().iter().any(|m| m == "agent.confirm"));
    // Any other write of the same grant is covered as before, without a ping.
    let r = authorize_write_as(&client, Some("t1"), &ask(false), WAIT).unwrap();
    assert_eq!(r, Authorized::Covered { grant: "g1".into() });
}

#[test]
fn a_server_that_knows_worktree_grants_covers_the_start() {
    let (_dir, path) = temp_socket("grant-new");
    let asked = fake_server(&path, GRANT_WORKTREE_PROTOCOL);
    let client = Client::new(&path);
    let r = authorize_write_as(&client, Some("t1"), &ask(true), WAIT).unwrap();
    assert_eq!(r, Authorized::Covered { grant: "g1".into() });
    assert!(!asked.lock().unwrap().iter().any(|m| m == "agent.confirm"));
}
