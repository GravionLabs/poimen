//! `report_done` (#980) against an in-process poimen-server: the stored report, the `run.finished`
//! in the user's agent-events, the line typed into an idle starter (and held back while it
//! works), `wait_agent` returning the report and the preamble of `start_agent`. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

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

fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        thread::yield_now();
    }
    panic!("{what}");
}

#[test]
fn a_report_is_stored_closes_the_run_and_tells_the_idle_starter() {
    let f = fixture("report");
    let typed = f.dir.join("typed.txt");
    let starter = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            name: Some("lead".into()),
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                format!("cat > {}", typed.display()),
            ]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let helper = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            name: Some("helper".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            started_by: Some(starter.clone()),
            ..Default::default()
        })
        .unwrap()
        .id;
    agent_status(&f.client, &starter, 1, AgentStatus::Working);
    agent_status(&f.client, &helper, 1, AgentStatus::Working);

    let r = call(
        &f,
        Some(&helper),
        None,
        "report_done",
        json!({"outcome": "succeeded", "summary": "Fixed it.", "files": ["a.rs"]}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["starter"], starter.as_str());
    let run = r["structuredContent"]["run"].as_str().expect("a run id");

    // Stored on the terminal.
    let listed = f.client.list_terminals(None).unwrap();
    let report = listed
        .iter()
        .find(|t| t.id == helper)
        .and_then(|t| t.report.clone())
        .expect("a stored report");
    assert_eq!(report.outcome, "succeeded");
    assert_eq!(report.summary, "Fixed it.");
    assert_eq!(report.files, ["a.rs"]);

    // The run in the shared directory is finished with the summary.
    let runs = call(
        &f,
        None,
        None,
        "list_runs",
        json!({"repository": f.repo.display().to_string()}),
    );
    let runs = runs["structuredContent"]["runs"]
        .as_array()
        .unwrap()
        .clone();
    let mine = runs.iter().find(|r| r["run"] == run).expect("the run");
    assert_eq!(mine["status"], "finished");
    assert_eq!(mine["outcome"], "succeeded");
    assert_eq!(mine["reason"], "Fixed it.");
    assert_eq!(mine["host"]["poimen.terminal"], helper.as_str());

    // The starter works: the line waits.
    // The report call has returned: the starter works, so nothing was typed.
    assert_eq!(std::fs::read_to_string(&typed).unwrap_or_default(), "");
    agent_status(&f.client, &starter, 2, AgentStatus::Idle);
    eventually("the line in the starter", || {
        std::fs::read_to_string(&typed)
            .is_ok_and(|t| t.contains(&format!("terminal {helper} (helper) reported succeeded")))
    });
    // Once: a second report is refused, and the summary was never typed.
    let again = call(
        &f,
        Some(&helper),
        None,
        "report_done",
        json!({"outcome": "failed", "summary": "Again."}),
    );
    assert_eq!(again["isError"], true, "{again}");
    assert!(
        !std::fs::read_to_string(&typed)
            .unwrap()
            .contains("Fixed it.")
    );

    // The server refuses a second report too, whoever asks.
    let direct = f.client.request(
        "terminal.report_done",
        json!({"terminal_id": helper, "outcome": "failed", "summary": "Again."}),
    );
    assert!(direct.is_err());

    // wait_agent returns the report.
    agent_status(&f.client, &helper, 2, AgentStatus::Done);
    let waited = call(
        &f,
        None,
        None,
        "wait_agent",
        json!({"terminal": helper, "timeout_s": 5}),
    );
    assert_eq!(waited["structuredContent"]["status"], "done", "{waited}");
    assert_eq!(
        waited["structuredContent"]["report"]["summary"],
        "Fixed it."
    );
}

#[test]
fn a_report_appends_to_the_running_run_of_the_terminal() {
    let f = fixture("append");
    let t = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let events = f.dir.join("events");
    std::fs::create_dir_all(&events).unwrap();
    std::fs::write(
        events.join("loop-1.jsonl"),
        format!(
            "{}\n",
            json!({"v":1,"seq":1,"ts":poimen_protocol::files::utc_now(),"run":"loop-1",
                "type":"run.started","tone":"info","host":{"poimen.terminal":t},
                "origin":{"worktree":f.repo.display().to_string()}})
        ),
    )
    .unwrap();
    let r = call(
        &f,
        Some(&t),
        None,
        "report_done",
        json!({"outcome": "failed", "summary": "Broke."}),
    );
    assert_eq!(r["structuredContent"]["run"], "loop-1", "{r}");
    let text = std::fs::read_to_string(events.join("loop-1.jsonl")).unwrap();
    let last: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["type"], "run.finished");
    assert_eq!(last["seq"], 2);
    assert_eq!(last["tone"], "danger");
}

#[test]
fn bad_reports_and_callers_outside_poimen_are_refused() {
    let f = fixture("refuse");
    let t = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    for (caller, args) in [
        (None, json!({"outcome": "succeeded", "summary": "x"})),
        (
            Some(t.as_str()),
            json!({"outcome": "great", "summary": "x"}),
        ),
        (Some(t.as_str()), json!({"outcome": "failed"})),
        (
            Some(t.as_str()),
            json!({"outcome": "failed", "summary": "x", "files": "a"}),
        ),
    ] {
        let r = call(&f, caller, None, "report_done", args.clone());
        assert_eq!(r["isError"], true, "{args} {r}");
    }
    let listed = f.client.list_terminals(None).unwrap();
    assert!(listed.iter().all(|t| t.report.is_none()));
}

#[test]
fn start_agent_appends_the_preamble_to_the_prompt() {
    let f = fixture("preamble");
    let lead = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            name: Some("lead".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let home = f.dir.join("home");
    let config = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let out = f.dir.join("prompt.txt");
    let harness = json!({"harnesses": {"echoer": {
        "command": ["sh", "-c", format!("printf '%s' \"$1\" > {}; sleep 20", out.display()), "sh"],
        "promptArgs": ["{prompt}"]
    }}});
    std::fs::write(config, harness.to_string()).unwrap();
    let client = f.client.clone();
    let reported = out.clone();
    thread::spawn(move || {
        // Like a real agent, report only once the command line has run (it writes the prompt
        // file). Earlier, `start_agent` may still be checking the terminal for an agent and
        // would refuse it as already running.
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if reported.exists()
                && let Some(t) = client
                    .list_terminals(None)
                    .unwrap()
                    .into_iter()
                    .find(|t| t.name == "kid")
            {
                agent_status(&client, &t.id, 1, AgentStatus::Idle);
                return;
            }
            thread::yield_now();
        }
    });
    let r = call(
        &f,
        Some(&lead),
        Some(home),
        "start_agent",
        json!({"harness": "echoer", "name": "kid", "prompt": "Do the thing."}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    let kid = f
        .client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|t| t.name == "kid")
        .unwrap();
    eventually("the prompt in the file", || out.exists());
    eventually("the whole prompt", || {
        std::fs::read_to_string(&out).is_ok_and(|t| t.contains("report_done"))
    });
    let prompt = std::fs::read_to_string(&out).unwrap();
    assert!(prompt.starts_with("Do the thing."), "{prompt}");
    assert!(prompt.contains("report_done"), "{prompt}");
    assert_eq!(kid.started_by.as_deref(), Some(lead.as_str()));
}

fn shell_terminal(f: &Fixture) -> String {
    f.client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn git(f: &Fixture, args: &[&str]) {
    let out = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(&f.repo)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

fn done(f: &Fixture, terminal: &str) -> String {
    let r = call(
        f,
        Some(terminal),
        None,
        "report_done",
        json!({"outcome": "succeeded", "summary": "Ok."}),
    );
    r["structuredContent"]["run"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"))
        .to_owned()
}

fn lines(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_new_run_is_written_to_the_shared_directory_with_its_origin() {
    let f = fixture("origin");
    git(&f, &["symbolic-ref", "HEAD", "refs/heads/feat/x"]);
    git(
        &f,
        &["remote", "add", "origin", "git@github.com:example/a.git"],
    );
    let t = shell_terminal(&f);
    let run = done(&f, &t);
    assert!(run.starts_with("agent-"), "{run}");
    let events = lines(&f.dir.join("events").join(format!("{run}.jsonl")));
    let origin = &events[0]["origin"];
    assert_eq!(origin["repo"], "example/a", "{origin}");
    assert_eq!(origin["branch"], "feat/x");
    assert_eq!(origin["worktree"], f.repo.display().to_string());
    assert!(origin["host"].as_str().is_some_and(|h| !h.is_empty()));
    assert!(!f.repo.join(".git/.agents").exists(), "no legacy write");
}

#[test]
fn the_same_terminal_id_in_two_repositories_gives_two_run_ids() {
    // Two servers, so both terminals have the first id; both report within the same second or so.
    let (a, b) = (fixture("twin-a"), fixture("twin-b"));
    let (ta, tb) = (shell_terminal(&a), shell_terminal(&b));
    assert_eq!(ta, tb, "the same terminal id");
    assert_ne!(done(&a, &ta), done(&b, &tb));
}

#[test]
fn a_run_of_another_repository_or_machine_with_the_terminal_id_is_left_alone() {
    let f = fixture("foreign");
    let t = shell_terminal(&f);
    let events = f.dir.join("events");
    std::fs::create_dir_all(&events).unwrap();
    let repo = f.repo.display().to_string();
    let foreign = [
        ("other-repo", json!({"repo": "example/other"})),
        (
            "other-host",
            json!({"worktree": repo, "host": "some-other-machine"}),
        ),
    ];
    let mut before = Vec::new();
    for (run, origin) in foreign {
        let path = events.join(format!("{run}.jsonl"));
        let line = json!({"v":1,"seq":1,"ts":poimen_protocol::files::utc_now(),"run":run,
            "type":"run.started","tone":"info","host":{"poimen.terminal":t},"origin":origin});
        std::fs::write(&path, format!("{line}\n")).unwrap();
        before.push((path, format!("{line}\n")));
    }
    let run = done(&f, &t);
    assert!(run.starts_with("agent-"), "{run}");
    for (path, text) in before {
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}

#[test]
fn a_run_started_in_the_legacy_directory_is_finished_in_the_shared_one() {
    let f = fixture("legacy");
    let t = shell_terminal(&f);
    let legacy = f.repo.join(".git/.agents/events");
    std::fs::create_dir_all(&legacy).unwrap();
    let text = format!(
        "{}\n",
        json!({"v":1,"seq":1,"ts":poimen_protocol::files::utc_now(),"run":"old-1",
            "type":"run.started","tone":"info","host":{"poimen.terminal":t}})
    );
    std::fs::write(legacy.join("old-1.jsonl"), &text).unwrap();
    assert_eq!(done(&f, &t), "old-1");
    // The legacy file is not written; the shared one names the repository.
    assert_eq!(
        std::fs::read_to_string(legacy.join("old-1.jsonl")).unwrap(),
        text
    );
    let shared = lines(&f.dir.join("events").join("old-1.jsonl"));
    assert_eq!(shared[0]["type"], "run.started");
    assert_eq!(
        shared[0]["origin"]["worktree"],
        f.repo.display().to_string()
    );
    assert_eq!(shared[1]["type"], "run.finished");
    assert_eq!(shared[1]["seq"], 2);
    let listed = call(&f, None, None, "list_runs", json!({}));
    assert_eq!(
        listed["structuredContent"]["runs"][0]["status"], "finished",
        "{listed}"
    );
}

#[test]
fn outside_a_repository_and_for_the_run_the_server_gave_the_terminal() {
    let f = fixture("plain");
    // A terminal outside a repository: the run names the machine only.
    let plain = f.dir.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let space = f
        .client
        .create_space("plain", &plain.display().to_string())
        .unwrap()
        .id;
    let outside = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: space,
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let events = f.dir.join("events");
    let run = done(&f, &outside);
    let origin = lines(&events.join(format!("{run}.jsonl")))[0]["origin"].clone();
    let keys: Vec<_> = origin.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["host"], "{origin}");

    // The run the server gave the terminal is continued, also without origin.
    let scripted = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            run: Some("script-1".into()),
            ..Default::default()
        })
        .unwrap()
        .id;
    let line = json!({"v":1,"seq":1,"ts":poimen_protocol::files::utc_now(),"run":"script-1",
        "type":"run.started","tone":"info","host":{"poimen.terminal":scripted}});
    std::fs::write(events.join("script-1.jsonl"), format!("{line}\n")).unwrap();
    assert_eq!(done(&f, &scripted), "script-1");
}
