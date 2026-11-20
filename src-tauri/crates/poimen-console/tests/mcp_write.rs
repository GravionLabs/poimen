//! The write tools of `poimen mcp` (#615) against an in-process poimen-server on a temporary
//! socket, with a stand-in for the app that answers the confirmations. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    AgentStatus, ClientInfo, ClientKind, Event, EventsSubscribe, TerminalCreate, TerminalStatus,
};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    /// The caller's space and terminal.
    own_space: String,
    caller: String,
    /// Another terminal of the caller's space, idle.
    sibling: String,
    /// A terminal of another space, idle.
    other_space: String,
    other: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn report(client: &Client, terminal: &str, seq: u64, status: AgentStatus) {
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
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
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
    let make_space = |name: &str| {
        let path = dir.join(name).canonicalize().unwrap();
        client
            .create_space(name, &path.display().to_string())
            .unwrap()
            .id
    };
    let terminal = |space: &str, name: &str| {
        client
            .create_terminal(&TerminalCreate {
                space_id: space.to_owned(),
                name: Some(name.into()),
                command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
                ..Default::default()
            })
            .unwrap()
            .id
    };
    let own_space = make_space("work");
    let other_space = make_space("other");
    let caller = terminal(&own_space, "lead");
    let sibling = terminal(&own_space, "sibling");
    let other = terminal(&other_space, "remote");
    report(&client, &caller, 1, AgentStatus::Working);
    report(&client, &sibling, 1, AgentStatus::Idle);
    report(&client, &other, 1, AgentStatus::Idle);
    Fixture {
        dir,
        client,
        own_space,
        caller,
        sibling,
        other_space,
        other,
    }
}

/// An app stand-in: subscribed as `app`, it sends every event on the channel.
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

fn next_matching(rx: &Receiver<Event>, pick: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("the event in time");
        if pick(&event) {
            return event;
        }
    }
}

/// Answers the next confirmation with `approve` and hands the request on for checks.
fn answer_next(
    client: &Client,
    rx: Receiver<Event>,
    approve: bool,
) -> thread::JoinHandle<(Event, Receiver<Event>)> {
    let client = client.clone();
    thread::spawn(move || {
        let asked = next_matching(&rx, |e| matches!(e, Event::AgentConfirmRequested { .. }));
        let Event::AgentConfirmRequested { id, .. } = &asked else {
            unreachable!()
        };
        client.answer_agent_confirm(id, approve).unwrap();
        (asked, rx)
    })
}

/// After a moment the prompted agent reports `working`, which ends the wait of `prompt_agent`.
fn work_soon(client: &Client, terminal: &str) {
    let (client, terminal) = (client.clone(), terminal.to_owned());
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        report(&client, &terminal, 2, AgentStatus::Working);
    });
}

fn call(client: &Client, env: &Env, name: &str, args: Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(client, env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn env<'a>(terminal: Option<&'a str>, home: Option<PathBuf>, wait: Duration) -> Env<'a> {
    Env {
        terminal_id: terminal,
        home,
        confirm_wait: wait,
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

const WAIT: Duration = Duration::from_secs(30);

#[test]
fn own_space_needs_no_confirmation() {
    let f = fixture("own");
    let rx = app(&f.client);
    work_soon(&f.client, &f.sibling);
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.sibling, "text": "hello"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["sent"], true);
    // No question reached the app; the write itself is listed, unconfirmed.
    let wrote = next_matching(&rx, |e| {
        matches!(
            e,
            Event::AgentConfirmRequested { .. } | Event::AgentWrote { .. }
        )
    });
    let Event::AgentWrote {
        caller_name,
        space_name,
        text,
        confirmed,
        ..
    } = wrote
    else {
        panic!("a confirmation was asked in the own space");
    };
    assert_eq!(
        (
            caller_name.as_str(),
            space_name.as_str(),
            text.as_str(),
            confirmed
        ),
        ("lead", "work", "hello", false)
    );
}

#[test]
fn another_space_runs_after_the_user_approved() {
    let f = fixture("approve");
    let rx = app(&f.client);
    let answered = answer_next(&f.client, rx, true);
    work_soon(&f.client, &f.other);
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.other, "text": "do the thing"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    let (asked, rx) = answered.join().unwrap();
    let Event::AgentConfirmRequested {
        caller_name,
        space_id,
        space_name,
        terminal_id,
        text,
        ..
    } = asked
    else {
        unreachable!()
    };
    assert_eq!(caller_name, "lead");
    assert_eq!(
        (space_id, space_name.as_str()),
        (f.other_space.clone(), "other")
    );
    assert_eq!(terminal_id.as_deref(), Some(f.other.as_str()));
    assert_eq!(text, "do the thing");
    let wrote = next_matching(&rx, |e| matches!(e, Event::AgentWrote { .. }));
    assert!(matches!(
        wrote,
        Event::AgentWrote {
            confirmed: true,
            ..
        }
    ));
}

#[test]
fn a_declined_write_is_refused_and_nothing_is_typed() {
    let f = fixture("decline");
    let answered = answer_next(&f.client, app(&f.client), false);
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.other, "text": "no"}),
    );
    answered.join().unwrap();
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("declined"), "{r}");
    // The agent was not prompted: it never turned to working.
    let t = f
        .client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|t| t.id == f.other)
        .unwrap();
    assert_eq!(t.agent.unwrap().status, AgentStatus::Idle);
}

#[test]
fn an_unanswered_write_times_out() {
    let f = fixture("timeout");
    let _silent = app(&f.client);
    let started = Instant::now();
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, Duration::from_millis(300)),
        "prompt_agent",
        json!({"terminal": f.other, "text": "hello?"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("did not answer"), "{r}");
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[test]
fn without_an_app_another_space_is_refused() {
    let f = fixture("noapp");
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.other, "text": "hi"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("not open"), "{r}");
}

#[test]
fn a_subscription_that_does_not_show_confirmations_is_no_app() {
    let f = fixture("link");
    // Like the link of a machine: kind app, but it never shows a question.
    let _link = f
        .client
        .subscribe_as(Some(ClientInfo {
            kind: ClientKind::App,
            pid: std::process::id(),
        }))
        .unwrap();
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.other, "text": "hi"}),
    );
    assert!(text(&r).contains("not open"), "{r}");
}

#[test]
fn without_a_calling_terminal_every_write_is_refused() {
    let f = fixture("nocaller");
    let _app = app(&f.client);
    let r = call(
        &f.client,
        &env(None, None, WAIT),
        "prompt_agent",
        json!({"terminal": f.sibling, "text": "hi"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("POIMEN_TERMINAL_ID"), "{r}");
    let r = call(
        &f.client,
        &env(None, None, WAIT),
        "start_agent",
        json!({"space": f.own_space, "harness": "claude"}),
    );
    assert_eq!(r["isError"], true);
}

#[test]
fn a_working_agent_is_refused_like_agent_prompt() {
    let f = fixture("working");
    report(&f.client, &f.sibling, 2, AgentStatus::Working);
    let r = call(
        &f.client,
        &env(Some(&f.caller), None, WAIT),
        "prompt_agent",
        json!({"terminal": f.sibling, "text": "hi"}),
    );
    assert_eq!(r["isError"], true);
    assert!(text(&r).contains("working"), "{r}");
}

/// A home whose config has a harness that only sleeps; the new terminal reports an agent soon.
fn sleeper_home(f: &Fixture) -> PathBuf {
    let home = f.dir.join("home");
    let config = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        config,
        r#"{"harnesses": {"sleeper": {"command": ["sleep", "20"]}}}"#,
    )
    .unwrap();
    home
}

/// Reports an agent for the terminal named `name` once it exists in `space` (any space: `None`).
fn agent_appears(client: &Client, space: Option<&str>, name: &str) {
    let (client, space, name) = (client.clone(), space.map(str::to_owned), name.to_owned());
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let found = client
                .list_terminals(space.as_deref())
                .unwrap()
                .into_iter()
                .find(|t| t.name == name && t.status == TerminalStatus::Running);
            if let Some(t) = found {
                thread::sleep(Duration::from_millis(300));
                report(&client, &t.id, 1, AgentStatus::Idle);
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
}

#[test]
fn start_agent_in_the_own_space_and_in_another_with_approval() {
    let f = fixture("start");
    let home = sleeper_home(&f);
    let rx = app(&f.client);

    agent_appears(&f.client, Some(&f.own_space), "helper");
    let r = call(
        &f.client,
        &env(Some(&f.caller), Some(home.clone()), WAIT),
        "start_agent",
        json!({"harness": "sleeper", "name": "helper"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["space"], f.own_space.as_str());
    let started = f.client.list_terminals(None).unwrap();
    let helper = started.iter().find(|t| t.name == "helper").unwrap();
    assert_eq!(helper.started_by.as_deref(), Some(f.caller.as_str()));
    let caller = started.iter().find(|t| t.id == f.caller).unwrap();
    assert_eq!(caller.started_by, None);

    let answered = answer_next(&f.client, rx, true);
    agent_appears(&f.client, Some(&f.other_space), "remote-helper");
    let r = call(
        &f.client,
        &env(Some(&f.caller), Some(home), WAIT),
        "start_agent",
        json!({"space": f.other_space, "harness": "sleeper", "name": "remote-helper"}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    let (asked, _) = answered.join().unwrap();
    let Event::AgentConfirmRequested {
        text, terminal_id, ..
    } = asked
    else {
        unreachable!()
    };
    assert_eq!(text, "sleep 20");
    assert_eq!(terminal_id, None);
}

#[test]
fn a_declined_start_creates_no_terminal() {
    let f = fixture("start-declined");
    let home = sleeper_home(&f);
    let answered = answer_next(&f.client, app(&f.client), false);
    let before = f.client.list_terminals(None).unwrap().len();
    let r = call(
        &f.client,
        &env(Some(&f.caller), Some(home), WAIT),
        "start_agent",
        json!({"space": f.other_space, "harness": "sleeper"}),
    );
    answered.join().unwrap();
    assert_eq!(r["isError"], true);
    assert_eq!(f.client.list_terminals(None).unwrap().len(), before);
}

#[test]
fn start_agent_names_an_unknown_harness_and_a_missing_space() {
    let f = fixture("start-bad");
    let e = env(Some(&f.caller), None, WAIT);
    let r = call(&f.client, &e, "start_agent", json!({"harness": "nope"}));
    assert!(text(&r).contains("unknown harness"), "{r}");
    let r = call(
        &f.client,
        &e,
        "start_agent",
        json!({"harness": "claude", "space": "s999"}),
    );
    assert!(text(&r).contains("not found"), "{r}");
}

/// A repository with the local branch `topic` at `dir/repo`.
fn repo_with_topic(f: &Fixture) -> PathBuf {
    let repo = f.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let ok = poimen_core::gui_path::command("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "init"]);
    git(&["branch", "topic"]);
    repo.canonicalize().unwrap()
}

#[test]
fn start_agent_in_a_worktree_asks_even_in_the_own_space_and_reuses_the_branch() {
    let f = fixture("worktree");
    let home = sleeper_home(&f);
    let repo = repo_with_topic(&f);
    let repo_space = f
        .client
        .create_space("repo", &repo.display().to_string())
        .unwrap()
        .id;
    let rx = app(&f.client);
    let e = env(Some(&f.caller), Some(home), WAIT);

    let answered = answer_next(&f.client, rx, true);
    agent_appears(&f.client, None, "wt");
    let r = call(
        &f.client,
        &e,
        "start_agent",
        json!({"space": repo_space, "harness": "sleeper", "name": "wt",
               "worktree": {"branch": "topic"}}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    let wt = &r["structuredContent"]["worktree"];
    assert_eq!(wt["branch"], "topic");
    assert_eq!(wt["created"], true);
    let path = wt["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).join(".git").exists(), "{path}");
    let new_space = r["structuredContent"]["space"].as_str().unwrap();
    assert_ne!(new_space, repo_space);
    let spaces = f.client.list_spaces().unwrap();
    assert_eq!(spaces.iter().find(|s| s.id == new_space).unwrap().cwd, path);
    let (asked, rx) = answered.join().unwrap();
    let Event::AgentConfirmRequested { text, .. } = asked else {
        unreachable!()
    };
    assert!(text.contains("branch topic"), "{text}");

    // The branch has a worktree now: it is reused, never changed.
    let answered = answer_next(&f.client, rx, true);
    agent_appears(&f.client, None, "wt2");
    let r = call(
        &f.client,
        &e,
        "start_agent",
        json!({"space": repo_space, "harness": "sleeper", "name": "wt2",
               "worktree": {"branch": "topic"}}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(r["structuredContent"]["worktree"]["created"], false);
    assert_eq!(r["structuredContent"]["worktree"]["path"], path);
    answered.join().unwrap();
}

#[test]
fn a_declined_worktree_start_creates_no_worktree_and_bad_arguments_are_refused() {
    let f = fixture("worktree-declined");
    let home = sleeper_home(&f);
    let repo = repo_with_topic(&f);
    let repo_space = f
        .client
        .create_space("repo", &repo.display().to_string())
        .unwrap()
        .id;
    let rx = app(&f.client);
    let e = env(Some(&f.caller), Some(home), WAIT);

    let answered = answer_next(&f.client, rx, false);
    let r = call(
        &f.client,
        &e,
        "start_agent",
        json!({"space": repo_space, "harness": "sleeper", "worktree": {"branch": "topic"}}),
    );
    assert_eq!(r["isError"], true, "{r}");
    answered.join().unwrap();
    assert_eq!(f.client.list_spaces().unwrap().len(), 3);
    let listed = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "list"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&listed.stdout).lines().count(), 1);

    for bad in [
        json!({"issue": 1, "branch": "topic"}),
        json!({}),
        json!({"branch": "a{nr}"}),
        json!("topic"),
    ] {
        let r = call(
            &f.client,
            &e,
            "start_agent",
            json!({"harness": "sleeper", "worktree": bad}),
        );
        assert_eq!(r["isError"], true, "{bad}: {r}");
        assert!(text(&r).contains("worktree"), "{}", text(&r));
    }
}
