//! `read_terminal` (#1154) against an in-process poimen-server on a temporary socket: the last
//! lines of a terminal as text, the limit, a cut answer, no change of the controller. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AttachMode, TerminalCreate};
use poimen_console::mcp;
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    space: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcp-rt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir
            .join("state")
            .join("server.json")
            .with_file_name("history"),
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
    Fixture { dir, client, space }
}

fn shell(f: &Fixture, script: &str) -> String {
    f.client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            name: Some("out".into()),
            command: Some(vec!["sh".into(), "-c".into(), script.into()]),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn read(f: &Fixture, args: Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "read_terminal", "arguments": args}});
    let mut out = Vec::new();
    mcp::serve(&f.client, None, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    serde_json::from_str::<Value>(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap()
        ["result"]
        .clone()
}

/// Reads until the text ends with `last` (the output arrives asynchronously).
fn read_until(f: &Fixture, id: &str, extra: Value, last: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut args = json!({"terminal": id});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = read(f, args);
        let done = r["structuredContent"]["text"]
            .as_str()
            .is_some_and(|t| t.ends_with(last));
        if done || Instant::now() > deadline {
            return r;
        }
        thread::yield_now();
    }
}

#[test]
fn returns_the_last_lines_as_text_and_whether_the_terminal_runs() {
    let f = fixture("lines");
    let id = shell(
        &f,
        "printf '\\033[31mone\\033[0m\\ntwo\\nthree\\n'; sleep 30",
    );
    let r = read_until(&f, &id, json!({}), "three");
    assert!(r.get("isError").is_none(), "{r}");
    let s = &r["structuredContent"];
    assert_eq!(s["text"], "one\ntwo\nthree");
    assert_eq!(s["lines"], 3);
    assert_eq!(s["running"], true);
    assert!(s.get("note").is_none());

    let r = read_until(&f, &id, json!({"lines": 2}), "three");
    assert_eq!(r["structuredContent"]["text"], "two\nthree");
}

#[test]
fn observing_leaves_the_controller_alone() {
    let f = fixture("observe");
    let id = shell(&f, "echo hi; sleep 30");
    let held = f.client.attach(&id, AttachMode::Control, false).unwrap();
    read_until(&f, &id, json!({}), "hi");
    // Still the controller: a second control attach without take-over is refused.
    assert!(f.client.attach(&id, AttachMode::Control, false).is_err());
    drop(held);
}

#[test]
fn a_long_answer_is_cut_with_a_note() {
    let f = fixture("cut");
    // 400 lines of 200 bytes: more than the 64 KiB limit.
    let id = shell(
        &f,
        "i=0; while [ $i -lt 400 ]; do printf '%0200d\\n' $i; i=$((i+1)); done; echo end; sleep 30",
    );
    let r = read_until(&f, &id, json!({"lines": 500}), "end");
    let s = &r["structuredContent"];
    assert!(s["text"].as_str().unwrap().len() <= 64 * 1024);
    assert!(s["text"].as_str().unwrap().ends_with("end"));
    assert!(s["lines"].as_u64().unwrap() < 401);
    assert!(s["note"].as_str().unwrap().contains("cut"));
}

#[test]
fn an_unknown_terminal_or_a_bad_limit_is_a_tool_error() {
    let f = fixture("errors");
    assert_eq!(read(&f, json!({"terminal": "nope"}))["isError"], true);
    assert_eq!(read(&f, json!({}))["isError"], true);
    let id = shell(&f, "sleep 30");
    assert_eq!(
        read(&f, json!({"terminal": id, "lines": 501}))["isError"],
        true
    );
    assert_eq!(
        read(&f, json!({"terminal": id, "lines": 0}))["isError"],
        true
    );
}
