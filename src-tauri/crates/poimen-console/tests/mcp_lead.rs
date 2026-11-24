//! `record_run` and `read_run` (#1212, ADR 0018) against an in-process poimen-server: a lead
//! writes its run, reads it back, and a new lead session in another terminal resumes it after the
//! first one ended. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::TerminalCreate;
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    repo: PathBuf,
    space: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpr-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let ok = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(&repo)
        .args(["init", "-q"])
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git init");
    let repo = repo.canonicalize().unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir
            .join("state")
            .join("server.json")
            .with_file_name("history"),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let space = client
        .create_space("work", &repo.display().to_string())
        .unwrap()
        .id;
    Fixture {
        dir,
        client,
        repo,
        space,
    }
}

fn call(
    client: &Client,
    terminal: Option<&str>,
    home: Option<PathBuf>,
    name: &str,
    args: Value,
) -> Value {
    let env = Env {
        terminal_id: terminal,
        home,
        confirm_wait: Duration::from_secs(30),
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn terminal(f: &Fixture, name: &str) -> String {
    f.client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            name: Some(name.into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn tool(f: &Fixture, terminal: &str, name: &str, args: Value) -> Value {
    call(&f.client, Some(terminal), None, name, args)
}

fn data(result: &Value) -> Value {
    assert_ne!(result["isError"], json!(true), "{result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap_or(Value::String(text.into()))
}

fn error(result: &Value) -> String {
    assert_eq!(result["isError"], json!(true), "{result}");
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn a_lead_writes_its_run_and_reads_it_back() {
    let f = fixture("write");
    let lead = terminal(&f, "lead");
    let started = data(&tool(
        &f,
        &lead,
        "record_run",
        json!({"event": "start", "title": "ship M016", "plan": {"repositories": [{"path": "/a", "milestone": "M016"}]}}),
    ));
    let run = started["run"].as_str().unwrap().to_owned();
    assert!(run.starts_with("lead-"));
    for args in [
        json!({"event": "member", "task": "a#1", "target": "/a", "terminal": "t9", "deliver_run": "deliver-1"}),
        json!({"event": "dependency", "task": "a#1", "blocked_by": "b#2"}),
        json!({"event": "grant", "grant": {"id": "g1", "spaces": ["s1"], "tools": ["start_agent"], "max_agents": 2}}),
        json!({"event": "attention", "id": "w1", "task": "a#1", "terminal": "t9", "text": "asks which db"}),
        json!({"event": "decision", "task": "a#1", "text": "use sqlite"}),
        json!({"event": "resolved", "id": "w1"}),
        json!({"event": "note", "text": "waiting"}),
    ] {
        data(&tool(&f, &lead, "record_run", args));
    }
    let read = data(&tool(&f, &lead, "read_run", json!({})));
    assert_eq!(read["run"], json!(run));
    assert_eq!(read["yours"], json!(true));
    assert_eq!(read["plan"]["repositories"][0]["milestone"], json!("M016"));
    assert_eq!(read["members"][0]["terminal"], json!("t9"));
    assert_eq!(read["members"][0]["deliver_run"], json!("deliver-1"));
    assert_eq!(read["dependencies"][0]["blocked_by"], json!("b#2"));
    assert_eq!(read["grant"]["max_agents"], json!(2));
    assert_eq!(read["decisions"][0]["text"], json!("use sqlite"));
    // The Runs view sees the same run: one open task, no attention left.
    assert_eq!(read["state"]["status"], json!("running"));
    assert_eq!(read["state"]["tasks"][0]["task"], json!("a#1"));
    assert_eq!(read["state"]["attentions"], json!([]));
    let listed = data(&tool(
        &f,
        &lead,
        "list_runs",
        json!({"repository": f.repo.display().to_string()}),
    ));
    assert_eq!(listed["runs"][0]["run"], json!(run));
}

#[test]
fn a_new_lead_in_another_terminal_resumes_after_the_writer_ended() {
    let f = fixture("resume");
    let old = terminal(&f, "lead-1");
    let run = data(&tool(
        &f,
        &old,
        "record_run",
        json!({"event": "start", "title": "plan"}),
    ))["run"]
        .as_str()
        .unwrap()
        .to_owned();
    data(&tool(
        &f,
        &old,
        "record_run",
        json!({"event": "plan", "plan": {"order": ["a", "b"]}}),
    ));
    data(&tool(
        &f,
        &old,
        "record_run",
        json!({"event": "member", "task": "a#1", "target": "/a"}),
    ));
    f.client.close_terminal(&old).unwrap();

    let new = terminal(&f, "lead-2");
    // Without a run id the new session sees the open runs and who wrote them.
    let none = data(&tool(&f, &new, "read_run", json!({})));
    assert_eq!(none["run"], Value::Null);
    assert_eq!(none["open_runs"][0]["run"], json!(run));
    assert_eq!(none["open_runs"][0]["terminal"], json!(old));
    // It cannot write into the run before it took it over.
    let refused = error(&tool(
        &f,
        &new,
        "record_run",
        json!({"event": "note", "run": run, "text": "x"}),
    ));
    assert!(refused.contains("another lead terminal"), "{refused}");

    data(&tool(
        &f,
        &new,
        "record_run",
        json!({"event": "resume", "run": run}),
    ));
    let read = data(&tool(&f, &new, "read_run", json!({})));
    assert_eq!(read["run"], json!(run));
    assert_eq!(read["yours"], json!(true));
    assert_eq!(read["plan"]["order"], json!(["a", "b"]));
    assert_eq!(read["state"]["tasks"][0]["task"], json!("a#1"));
    data(&tool(
        &f,
        &new,
        "record_run",
        json!({"event": "member_done", "task": "a#1", "outcome": "succeeded"}),
    ));
    data(&tool(
        &f,
        &new,
        "record_run",
        json!({"event": "finish", "outcome": "succeeded", "text": "done"}),
    ));
    let after = data(&tool(&f, &new, "read_run", json!({"run": run})));
    assert_eq!(after["state"]["status"], json!("finished"));
    let late = error(&tool(
        &f,
        &new,
        "record_run",
        json!({"event": "note", "run": run, "text": "x"}),
    ));
    assert!(late.contains("finished"), "{late}");
}

#[test]
fn bad_calls_are_refused() {
    let f = fixture("bad");
    let lead = terminal(&f, "lead");
    assert!(
        error(&tool(
            &f,
            &lead,
            "record_run",
            json!({"event": "note", "text": "x"})
        ))
        .contains("no open lead run")
    );
    data(&tool(
        &f,
        &lead,
        "record_run",
        json!({"event": "start", "title": "t"}),
    ));
    assert!(
        error(&tool(
            &f,
            &lead,
            "record_run",
            json!({"event": "start", "title": "t"})
        ))
        .contains("already")
    );
    assert!(
        error(&tool(
            &f,
            &lead,
            "record_run",
            json!({"event": "member_done", "task": "zz", "outcome": "failed"})
        ))
        .contains("unknown task")
    );
    assert!(
        error(&tool(
            &f,
            &lead,
            "record_run",
            json!({"event": "note", "run": "../x", "text": "x"})
        ))
        .contains("not a lead run id")
    );
    assert!(
        error(&tool(&f, &lead, "record_run", json!({"event": "bogus"}))).contains("unknown event")
    );
    let outside = call(
        &f.client,
        None,
        None,
        "record_run",
        json!({"event": "start", "title": "t"}),
    );
    assert!(error(&outside).contains("POIMEN_TERMINAL_ID"));
}
