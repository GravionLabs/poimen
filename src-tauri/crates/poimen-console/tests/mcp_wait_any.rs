//! `wait_any` (#1210) against an in-process poimen-server: terminals and runs, at once and by
//! event, the report, the others and the tool errors. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
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

fn agent_status(client: &Client, terminal: &str, seq: u64, status: AgentStatus) {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": terminal, "seq": seq, "status": status, "agent": "claude"}),
        )
        .unwrap();
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpw-{}-{name}", std::process::id()));
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
    f: &Fixture,
    terminal: Option<&str>,
    home: Option<PathBuf>,
    name: &str,
    args: Value,
) -> Value {
    let env = Env {
        terminal_id: terminal,
        home,
        confirm_wait: Duration::from_secs(30),
        events_dir: Some(f.dir.join("events")),
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
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

fn events_dir(f: &Fixture) -> PathBuf {
    let dir = f.dir.join("events");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn append(dir: &std::path::Path, run: &str, events: &[Value]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{run}.jsonl")))
        .unwrap();
    for e in events {
        writeln!(file, "{e}").unwrap();
    }
}

fn ev(run: &str, seq: u32, kind: &str, extra: Value) -> Value {
    let mut v = json!({"v": 1, "seq": seq, "ts": poimen_protocol::files::utc_now(), "run": run,
        "type": kind, "tone": "info"});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn wait(f: &Fixture, args: Value) -> Value {
    call(f, None, None, "wait_any", args)["structuredContent"].clone()
}

#[test]
fn a_target_already_done_returns_at_once_with_report_and_others() {
    let f = fixture("now");
    let (a, b) = (terminal(&f, "a"), terminal(&f, "b"));
    agent_status(&f.client, &a, 1, AgentStatus::Working);
    agent_status(&f.client, &b, 1, AgentStatus::Working);
    f.client
        .request(
            "terminal.report_done",
            json!({"terminal_id": b, "outcome": "succeeded", "summary": "Shipped."}),
        )
        .unwrap();
    agent_status(&f.client, &b, 2, AgentStatus::Done);
    let r = wait(&f, json!({"terminals": [a, b], "timeout_s": 5}));
    assert_eq!(r["timed_out"], false, "{r}");
    assert_eq!(r["fired"]["terminal"], b.as_str());
    assert_eq!(r["fired"]["status"], "done");
    assert_eq!(r["fired"]["report"]["summary"], "Shipped.");
    assert_eq!(r["others"][0]["terminal"], a.as_str());
    assert_eq!(r["others"][0]["status"], "working");
}

#[test]
fn a_blocked_agent_wakes_the_wait_with_its_message() {
    let f = fixture("blocked");
    let (a, b) = (terminal(&f, "a"), terminal(&f, "b"));
    agent_status(&f.client, &a, 1, AgentStatus::Working);
    agent_status(&f.client, &b, 1, AgentStatus::Working);
    // Reports until the wait returned, so one lands after it subscribed.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reporter = {
        let (client, b, stop) = (f.client.clone(), b.clone(), stop.clone());
        thread::spawn(move || {
            let mut seq = 10;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                client
                    .request(
                        "terminal.report_agent",
                        json!({"terminal_id": b, "seq": seq, "status": "blocked",
                            "agent": "claude", "message": "Allow rm?"}),
                    )
                    .unwrap();
                client
                    .request(
                        "terminal.report_agent",
                        json!({"terminal_id": b, "seq": seq + 1, "status": "working",
                            "agent": "claude"}),
                    )
                    .unwrap();
                seq += 2;
            }
        })
    };
    let r = wait(&f, json!({"terminals": [a, b], "timeout_s": 20}));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    reporter.join().unwrap();
    assert_eq!(r["fired"]["terminal"], b.as_str(), "{r}");
    assert_eq!(r["fired"]["status"], "blocked");
    assert_eq!(r["fired"]["message"], "Allow rm?");
}

#[test]
fn a_closed_terminal_is_gone() {
    let f = fixture("gone");
    let a = terminal(&f, "a");
    agent_status(&f.client, &a, 1, AgentStatus::Working);
    let closer = {
        let (client, a) = (f.client.clone(), a.clone());
        thread::spawn(move || {
            // Close until the wait took it (a closed terminal answers an error afterwards).
            while client
                .list_terminals(None)
                .unwrap()
                .iter()
                .any(|t| t.id == a)
            {
                let _ = client.request("terminal.close", json!({"terminal_id": a}));
                thread::yield_now();
            }
        })
    };
    let r = wait(&f, json!({"terminals": [a], "timeout_s": 20}));
    closer.join().unwrap();
    assert_eq!(r["fired"]["status"], "gone", "{r}");
}

#[test]
fn a_run_that_finishes_fires_and_the_terminal_stays_among_the_others() {
    let f = fixture("run");
    let a = terminal(&f, "a");
    agent_status(&f.client, &a, 1, AgentStatus::Working);
    let dir = events_dir(&f);
    append(&dir, "loop-1", &[ev("loop-1", 1, "run.started", json!({}))]);
    let repo = f.repo.display().to_string();
    // Appends a finish until the wait returned, so one lands after the wait started watching.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (dir, stop) = (dir.clone(), stop.clone());
        thread::spawn(move || {
            let mut seq = 2;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                append(
                    &dir,
                    "loop-1",
                    &[ev(
                        "loop-1",
                        seq,
                        "run.finished",
                        json!({"outcome": "succeeded", "reason": "All merged."}),
                    )],
                );
                seq += 1;
                thread::yield_now();
            }
        })
    };
    let r = wait(
        &f,
        json!({"terminals": [a], "runs": [{"repository": repo, "run": "loop-1"}], "timeout_s": 20}),
    );
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();
    assert_eq!(r["fired"]["type"], "run", "{r}");
    assert_eq!(r["fired"]["status"], "finished");
    assert_eq!(r["fired"]["outcome"], "succeeded");
    assert_eq!(r["others"][0]["terminal"], a.as_str());
}

#[test]
fn a_run_that_needs_you_fires_at_once_and_a_quiet_one_times_out() {
    let f = fixture("needs");
    let dir = events_dir(&f);
    append(
        &dir,
        "r-need",
        &[
            ev("r-need", 1, "run.started", json!({})),
            ev(
                "r-need",
                2,
                "attention",
                json!({"id": "a1", "reason": "review", "text": "Pick one", "needsYou": true}),
            ),
        ],
    );
    append(
        &dir,
        "r-quiet",
        &[ev("r-quiet", 1, "run.started", json!({}))],
    );
    let repo = f.repo.display().to_string();
    let r = wait(
        &f,
        json!({"runs": [{"repository": repo, "run": "r-quiet"}, {"repository": repo, "run": "r-need"}]}),
    );
    assert_eq!(r["fired"]["run"], "r-need", "{r}");
    assert_eq!(r["fired"]["status"], "needs-you");
    assert_eq!(r["fired"]["attentions"][0], "Pick one");
    let r = wait(
        &f,
        json!({"runs": [{"repository": repo, "run": "r-quiet"}], "timeout_s": 0}),
    );
    assert_eq!(r["timed_out"], true, "{r}");
    assert_eq!(r["fired"], Value::Null);
    assert_eq!(r["others"][0]["status"], "running");
}

#[test]
fn bad_arguments_are_tool_errors() {
    let f = fixture("bad");
    let repo = f.repo.display().to_string();
    for args in [
        json!({}),
        json!({"terminals": []}),
        json!({"terminals": ["nope"]}),
        json!({"runs": [{"repository": repo, "run": "nope"}]}),
        json!({"runs": [{"repository": repo, "run": "../x"}]}),
        json!({"runs": [{"repository": repo}]}),
        json!({"terminals": "x"}),
        json!({"terminals": ["x"], "other": 1}),
    ] {
        let r = call(&f, None, None, "wait_any", args.clone());
        assert_eq!(r["isError"], true, "{args}: {r}");
    }
}
