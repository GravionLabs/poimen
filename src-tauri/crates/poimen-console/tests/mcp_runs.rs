//! The run tools of `poimen mcp` over the per-user event directory (#1417): `list_runs`,
//! `get_events` and `wait_any` without naming a repository, the filters, the legacy directory and
//! `AGENT_EVENTS_DIR`. Every directory is a temporary one. Unix-only.
#![cfg(unix)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use poimen_client::Client;
use poimen_console::mcp::{self, Env};
use serde_json::{Value, json};

/// A temporary directory with two repositories (`example/a`, `example/b`) and the shared event
/// directory. No server: the run tools read files only.
struct Dirs {
    root: PathBuf,
    a: PathBuf,
    b: PathBuf,
    shared: PathBuf,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn dirs(name: &str) -> Dirs {
    let root = std::env::temp_dir().join(format!("poimen-mcpd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let repo = |name: &str| {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("https://github.com/example/{name}.git");
        for args in [vec!["init", "-q"], vec!["remote", "add", "origin", &url]] {
            let out = poimen_core::gui_path::command("git")
                .arg("-C")
                .arg(&dir)
                .args(&args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        }
        dir
    };
    let (a, b) = (repo("a"), repo("b"));
    let shared = root.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    Dirs { root, a, b, shared }
}

fn request(name: &str, args: &Value) -> String {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    format!("{msg}\n")
}

fn result(out: &[u8]) -> Value {
    let text = String::from_utf8_lossy(out);
    let line = text.lines().next().expect("an answer");
    serde_json::from_str::<Value>(line).unwrap()["result"].clone()
}

fn call(d: &Dirs, name: &str, args: Value) -> Value {
    let client = Client::new(d.root.join("no-server.sock"));
    let env = Env {
        terminal_id: None,
        home: None,
        confirm_wait: mcp::CONFIRM_WAIT,
        events_dir: Some(d.shared.clone()),
    };
    let mut out = Vec::new();
    let input = Cursor::new(request(name, &args));
    mcp::serve_with(&client, &env, input, &mut out).unwrap();
    result(&out)
}

fn ok(d: &Dirs, name: &str, args: Value) -> Value {
    let r = call(d, name, args);
    assert_ne!(r["isError"], json!(true), "{r}");
    r["structuredContent"].clone()
}

fn append(dir: &Path, run: &str, events: &[Value]) {
    std::fs::create_dir_all(dir).unwrap();
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

fn started(run: &str, repo: &str, worktree: Option<&Path>) -> Value {
    let mut origin = json!({"repo": repo, "branch": "main", "host": "box"});
    if let Some(w) = worktree {
        origin["worktree"] = json!(w.display().to_string());
    }
    ev(run, 1, "run.started", json!({"origin": origin}))
}

fn names(listed: &Value) -> Vec<String> {
    let mut names: Vec<String> = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn legacy(repo: &Path) -> PathBuf {
    repo.join(".git/.agents/events")
}

#[test]
fn the_runs_of_all_repositories_are_followed_without_naming_one() {
    let d = dirs("all");
    append(
        &d.shared,
        "run-a",
        &[started("run-a", "example/a", Some(&d.a))],
    );
    append(
        &d.shared,
        "run-b",
        &[started("run-b", "example/b", Some(&d.b))],
    );

    let listed = ok(&d, "list_runs", json!({}));
    assert_eq!(listed["repository"], Value::Null);
    assert_eq!(names(&listed), ["run-a", "run-b"]);
    for run in listed["runs"].as_array().unwrap() {
        let name = if run["run"] == "run-a" { "a" } else { "b" };
        assert_eq!(run["origin"]["repo"], format!("example/{name}"));
        assert_eq!(run["origin"]["branch"], "main");
        assert_eq!(run["origin"]["host"], "box");
        let worktree = d.root.join(name).display().to_string();
        assert_eq!(run["origin"]["worktree"], worktree);
    }
    for run in ["run-a", "run-b"] {
        let got = ok(&d, "get_events", json!({"run": run}));
        assert_eq!(got["events"][0]["run"], run, "{got}");
    }

    // Finishes B until the wait returned, so a line lands after the wait started watching.
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (shared, stop) = (d.shared.clone(), stop.clone());
        thread::spawn(move || {
            let mut seq = 2;
            while !stop.load(Ordering::Relaxed) {
                let end = ev(
                    "run-b",
                    seq,
                    "run.finished",
                    json!({"outcome": "succeeded"}),
                );
                append(&shared, "run-b", &[end]);
                seq += 1;
                thread::yield_now();
            }
        })
    };
    let waited = ok(
        &d,
        "wait_any",
        json!({"runs": [{"run": "run-a"}, {"run": "run-b"}], "timeout_s": 20}),
    );
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert_eq!(waited["fired"]["run"], "run-b", "{waited}");
    assert_eq!(waited["fired"]["status"], "finished");
    assert_eq!(waited["others"][0]["run"], "run-a");
    assert_eq!(waited["others"][0]["status"], "running");
}

#[test]
fn repository_and_repo_keep_only_that_repositorys_runs() {
    let d = dirs("filter");
    // By worktree (its name differs), by name (no worktree here), and another repository's.
    append(
        &d.shared,
        "by-tree",
        &[started("by-tree", "elsewhere/x", Some(&d.a))],
    );
    append(
        &d.shared,
        "by-name",
        &[started("by-name", "example/a", None)],
    );
    append(
        &d.shared,
        "of-b",
        &[started("of-b", "example/b", Some(&d.b))],
    );
    append(
        &legacy(&d.a),
        "old-a",
        &[ev("old-a", 1, "run.started", json!({}))],
    );
    append(
        &legacy(&d.b),
        "old-b",
        &[ev("old-b", 1, "run.started", json!({}))],
    );
    // One run in both places: the shared line wins for seq 1, the legacy one adds seq 2.
    let first = started("both", "example/a", Some(&d.a));
    append(&d.shared, "both", std::slice::from_ref(&first));
    append(
        &legacy(&d.a),
        "both",
        &[
            ev("both", 1, "run.started", json!({"tone": "danger"})),
            ev("both", 2, "run.finished", json!({"outcome": "succeeded"})),
        ],
    );
    let a = d.a.display().to_string();

    let listed = ok(&d, "list_runs", json!({"repository": a}));
    assert_eq!(names(&listed), ["both", "by-name", "by-tree", "old-a"]);
    let both = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run"] == "both")
        .unwrap();
    assert_eq!(both["status"], "finished");
    assert_eq!(both["lastSeq"], 2);

    let got = ok(&d, "get_events", json!({"repository": a, "run": "both"}));
    assert_eq!(got["events"], json!([first, got["events"][1]]), "{got}");
    assert_eq!(got["events"][1]["seq"], 2);

    // `repo` needs no checkout and reads the shared directory only.
    let listed = ok(&d, "list_runs", json!({"repo": "example/a"}));
    assert_eq!(names(&listed), ["both", "by-name"]);
    let listed = ok(&d, "list_runs", json!({"repo": "example/b"}));
    assert_eq!(names(&listed), ["of-b"]);
    // Both: the legacy runs of the repository count under its name.
    let listed = ok(
        &d,
        "list_runs",
        json!({"repository": a, "repo": "example/a"}),
    );
    assert_eq!(names(&listed), ["both", "by-name", "old-a"]);
    let listed = ok(
        &d,
        "list_runs",
        json!({"repository": a, "repo": "example/b"}),
    );
    assert_eq!(names(&listed), Vec::<String>::new());
}

#[test]
fn a_legacy_run_needs_its_repository_and_its_wait_sees_the_shared_directory() {
    let d = dirs("legacy-wait");
    append(
        &legacy(&d.a),
        "old-a",
        &[ev("old-a", 1, "run.started", json!({}))],
    );
    let a = d.a.display().to_string();
    let r = call(
        &d,
        "wait_any",
        json!({"runs": [{"run": "old-a"}], "timeout_s": 0}),
    );
    assert_eq!(r["isError"], true, "{r}");
    // The shared directory does not exist yet: the end written there after the upgrade is seen.
    std::fs::remove_dir_all(&d.shared).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (shared, stop) = (d.shared.clone(), stop.clone());
        thread::spawn(move || {
            let mut seq = 2;
            while !stop.load(Ordering::Relaxed) {
                // Only into a directory that is there: the wait makes it.
                if shared.is_dir() {
                    let end = ev("old-a", seq, "run.finished", json!({"outcome": "failed"}));
                    append(&shared, "old-a", &[end]);
                    seq += 1;
                }
                thread::yield_now();
            }
        })
    };
    let waited = ok(
        &d,
        "wait_any",
        json!({"runs": [{"repository": a, "run": "old-a"}], "timeout_s": 20}),
    );
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert_eq!(waited["fired"]["status"], "finished", "{waited}");
    assert_eq!(waited["fired"]["repository"], a);
}

#[test]
fn an_unknown_or_escaping_run_id_is_an_error_naming_the_directory() {
    let d = dirs("unknown");
    // A file above the shared directory that a path as run id would reach.
    append(
        &d.root,
        "outside",
        &[ev("outside", 1, "run.started", json!({}))],
    );
    let shared = d.shared.display().to_string();
    for run in ["nope", "../outside"] {
        for (tool, args) in [
            ("get_events", json!({"run": run})),
            ("wait_any", json!({"runs": [{"run": run}], "timeout_s": 0})),
        ] {
            let r = call(&d, tool, args);
            assert_eq!(r["isError"], true, "{tool} {run}: {r}");
            let text = r["content"][0]["text"].as_str().unwrap();
            assert!(text.contains(&shared), "{text}");
        }
    }
    for args in [json!({"repo": 1}), json!({"repository": ""})] {
        let r = call(&d, "list_runs", args.clone());
        assert_eq!(r["isError"], true, "{args}: {r}");
    }
}

/// `poimen mcp` itself with this environment, one tool call.
fn binary(d: &Dirs, over: &Path, name: &str, args: Value) -> Value {
    let mut child = poimen_core::gui_path::command(env!("CARGO_BIN_EXE_poimen"))
        .arg("mcp")
        .current_dir(&d.root)
        .env("HOME", d.root.join("home"))
        .env("XDG_STATE_HOME", d.root.join("state"))
        .env("AGENT_EVENTS_DIR", over)
        // A set socket: nothing is started, and the user's server is never asked.
        .env("POIMEN_SOCKET", d.root.join("no-server.sock"))
        .env_remove("POIMEN_TERMINAL_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request(name, &args).as_bytes())
        .unwrap();
    result(&child.wait_with_output().unwrap().stdout)
}

#[test]
fn agent_events_dir_replaces_the_default_directory_in_all_run_tools() {
    let d = dirs("env");
    let over = d.root.join("override");
    let default = d.root.join("state").join("agent-events");
    let finished = |run: &str| {
        [
            started(run, "example/a", None),
            ev(run, 2, "run.finished", json!({"outcome": "succeeded"})),
        ]
    };
    append(&over, "in-override", &finished("in-override"));
    append(&default, "in-default", &finished("in-default"));

    let listed = binary(&d, &over, "list_runs", json!({}));
    assert_eq!(
        names(&listed["structuredContent"]),
        ["in-override"],
        "{listed}"
    );
    let got = binary(&d, &over, "get_events", json!({"run": "in-override"}));
    assert_eq!(got["structuredContent"]["events"][1]["seq"], 2, "{got}");
    let got = binary(&d, &over, "get_events", json!({"run": "in-default"}));
    assert_eq!(got["isError"], true, "{got}");
    let wait = |run: &str| json!({"runs": [{"run": run}], "timeout_s": 5});
    let waited = binary(&d, &over, "wait_any", wait("in-override"));
    assert_eq!(
        waited["structuredContent"]["fired"]["status"], "finished",
        "{waited}"
    );
    let waited = binary(&d, &over, "wait_any", wait("in-default"));
    assert_eq!(waited["isError"], true, "{waited}");
}
