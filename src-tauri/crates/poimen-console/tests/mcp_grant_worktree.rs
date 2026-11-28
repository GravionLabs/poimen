//! Worktree starts under a scoped write grant (#1452, ADR 0018 amendment #1448) through `poimen
//! mcp` against an in-process poimen-server, with a stand-in for the app. Every way a grant must
//! not cover a worktree start is a test of its own. Unix-only.
#![cfg(unix)]

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    AgentGrantAddWorktreeSpace, AgentGrantAnswer, AgentWriteAction, ClientInfo, ClientKind, Event,
    EventsSubscribe, TerminalCreate,
};
use poimen_console::mcp::{self, Env};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

struct Fixture {
    dir: PathBuf,
    client: Client,
    home: PathBuf,
    /// The lead's own space (no repository) and its terminal.
    own: String,
    lead: String,
    /// The main checkout of `repo` (branches `w1`..`w4`, `x`) and of another repository.
    repo: String,
    other: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// The harness `worker` reports itself idle (OSC 7501), and working once a prompt was typed.
const HARNESS: &str = r#"{"harnesses": {"worker": {"command": ["sh", "-c",
    "printf '\\033]7501;state=idle:app=w\\007'; read l; printf '\\033]7501;state=working:app=w\\007'; sleep 60"]}}}"#;

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-mcpgw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["work", "repo", "other"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    let dir = dir.canonicalize().unwrap();
    for repo in ["repo", "other"] {
        let repo = dir.join(repo);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    }
    for b in ["w1", "w2", "w3", "w4", "x"] {
        git(&dir.join("repo"), &["branch", b]);
    }
    let home = dir.join("home");
    let config = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, HARNESS).unwrap();
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
    let space = |name: &str| space_on(&client, &dir.join(name));
    let (own, repo, other) = (space("work"), space("repo"), space("other"));
    let lead = sleeper(&client, &own, "lead");
    Fixture {
        dir,
        client,
        home,
        own,
        lead,
        repo,
        other,
    }
}

fn space_on(client: &Client, dir: &Path) -> String {
    let name = dir.file_name().unwrap().to_string_lossy();
    client
        .create_space(&name, &dir.display().to_string())
        .unwrap()
        .id
}

fn sleeper(client: &Client, space: &str, name: &str) -> String {
    client
        .create_terminal(&TerminalCreate {
            space_id: space.into(),
            name: Some(name.into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id
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

/// A question would wait this long: a covered write that asks fails instead of passing.
const ASK_WAIT: Duration = Duration::from_secs(3);

fn call(f: &Fixture, terminal: &str, tool: &str, args: Value) -> Value {
    let env = Env {
        terminal_id: Some(terminal),
        home: Some(f.home.clone()),
        confirm_wait: ASK_WAIT,
        events_dir: None,
    };
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    let mut out = Vec::new();
    mcp::serve_with(&f.client, &env, Cursor::new(format!("{msg}\n")), &mut out).unwrap();
    let line = String::from_utf8(out).unwrap();
    serde_json::from_str::<Value>(line.lines().next().unwrap()).unwrap()["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

fn yes() -> AgentGrantAnswer {
    AgentGrantAnswer {
        id: String::new(),
        approve: true,
        spaces: None,
        max_agents: None,
        worktrees: None,
    }
}

/// The lead's grant over `spaces`, confirmed in the app with `answer`; the grant id and an app
/// that sees what happens afterwards.
fn grant_with(
    f: &Fixture,
    spaces: &[&str],
    tools: &[&str],
    worktrees: bool,
    answer: AgentGrantAnswer,
) -> (String, Receiver<Event>) {
    let rx = app(&f.client);
    let seen = app(&f.client);
    let client = f.client.clone();
    let asked = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if let Event::AgentGrantRequested { id, .. } = rx.recv_timeout(left).unwrap() {
                client
                    .answer_agent_grant(&AgentGrantAnswer { id, ..answer })
                    .unwrap();
                return;
            }
        }
    });
    let r = call(
        f,
        &f.lead,
        "request_grant",
        json!({"plan": "workers", "spaces": spaces, "tools": tools, "max_agents": 3,
               "worktrees": worktrees}),
    );
    asked.join().unwrap();
    assert_eq!(r["structuredContent"]["granted"], true, "{r}");
    let id = r["structuredContent"]["grant"]["id"].as_str().unwrap();
    (id.to_owned(), seen)
}

fn grant(
    f: &Fixture,
    spaces: &[&str],
    tools: &[&str],
    worktrees: bool,
) -> (String, Receiver<Event>) {
    grant_with(f, spaces, tools, worktrees, yes())
}

const ALL: &[&str] = &["start_agent", "prompt_agent", "stop_terminal"];

fn start_in_worktree(f: &Fixture, from: &str, space: &str, branch: &str) -> Value {
    call(
        f,
        from,
        "start_agent",
        json!({"space": space, "harness": "worker", "name": branch,
               "worktree": {"branch": branch}}),
    )
}

/// The next `agent.wrote` the app sees; a question before it fails the test.
fn next_write(seen: &Receiver<Event>) -> (AgentWriteAction, String, bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match seen.recv_timeout(left).expect("agent.wrote") {
            e @ Event::AgentConfirmRequested { .. } => panic!("a question was asked: {e:?}"),
            Event::AgentWrote {
                action,
                space_id,
                confirmed,
                ..
            } => return (action, space_id, confirmed),
            _ => {}
        }
    }
}

fn no_question(seen: &Receiver<Event>) {
    while let Ok(e) = seen.try_recv() {
        assert!(!matches!(e, Event::AgentConfirmRequested { .. }), "{e:?}");
    }
}

/// The app's answer `approve` to the next question, which must be a `start_agent`.
fn answer_start(f: &Fixture, approve: bool) -> thread::JoinHandle<()> {
    let rx = app(&f.client);
    let client = f.client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if let Event::AgentConfirmRequested { id, action, .. } =
                rx.recv_timeout(left).expect("a question")
            {
                assert_eq!(action, AgentWriteAction::StartAgent);
                client.answer_agent_confirm(&id, approve).unwrap();
                return;
            }
        }
    })
}

fn granted_spaces(f: &Fixture) -> Vec<String> {
    let grants = f.client.agent_grants().unwrap().grants;
    grants
        .first()
        .map(|g| g.spaces.iter().map(|s| s.id.clone()).collect())
        .unwrap_or_default()
}

fn space_count(f: &Fixture) -> usize {
    f.client.list_spaces().unwrap().len()
}

#[test]
fn three_worktree_workers_start_without_a_dialog_and_their_spaces_join_the_grant() {
    let f = fixture("three");
    let (_, seen) = grant(&f, &[&f.repo], ALL, true);
    let mut workers = Vec::new();
    for branch in ["w1", "w2", "w3"] {
        let r = start_in_worktree(&f, &f.lead, &f.repo, branch);
        assert_eq!(r["isError"], Value::Null, "{r}");
        let space = r["structuredContent"]["space"].as_str().unwrap().to_owned();
        assert_ne!(space, f.repo);
        assert_eq!(r["structuredContent"]["worktree"]["branch"], branch, "{r}");
        assert_eq!(
            next_write(&seen),
            (AgentWriteAction::StartAgent, space.clone(), false),
            "announced, unconfirmed"
        );
        let terminal = r["structuredContent"]["terminal"].as_str().unwrap();
        workers.push((terminal.to_owned(), space));
    }
    let spaces = granted_spaces(&f);
    assert_eq!(spaces.len(), 4, "{spaces:?}");
    assert!(
        workers.iter().all(|(_, s)| spaces.contains(s)),
        "{spaces:?}"
    );

    // A fourth is over `max_agents`: refused with the reason, nothing asked, nothing made.
    let before = space_count(&f);
    let r = start_in_worktree(&f, &f.lead, &f.repo, "w4");
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("allows 3 agents"), "{}", text(&r));
    assert_eq!(space_count(&f), before);
    no_question(&seen);

    // The workers' spaces are in the grant: prompting and stopping them asks nothing.
    let (terminal, space) = &workers[0];
    let r = call(
        &f,
        &f.lead,
        "prompt_agent",
        json!({"terminal": terminal, "text": "go"}),
    );
    assert_eq!(r["structuredContent"]["sent"], true, "{r}");
    assert_eq!(
        next_write(&seen),
        (AgentWriteAction::PromptAgent, space.clone(), false)
    );
    let r = call(
        &f,
        &f.lead,
        "stop_terminal",
        json!({"terminal": terminal, "mode": "close", "force": true}),
    );
    assert_eq!(r["isError"], Value::Null, "{r}");
    assert_eq!(
        next_write(&seen),
        (AgentWriteAction::StopTerminal, space.clone(), false)
    );
}

/// The start asks, the user says no: refused, and no space is made.
fn asks(f: &Fixture, from: &str, space: &str) {
    let before = space_count(f);
    let asked = answer_start(f, false);
    let r = start_in_worktree(f, from, space, "x");
    asked.join().unwrap();
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("declined"), "{}", text(&r));
    assert_eq!(space_count(f), before);
}

#[test]
fn a_grant_without_worktrees_does_not_cover_a_worktree_start() {
    let f = fixture("no-worktrees");
    grant(&f, &[&f.repo], ALL, false);
    asks(&f, &f.lead, &f.repo);
}

#[test]
fn a_worktree_line_the_user_unchecked_does_not_cover_a_worktree_start() {
    let f = fixture("unchecked");
    let off = AgentGrantAnswer {
        worktrees: Some(false),
        ..yes()
    };
    grant_with(&f, &[&f.repo], ALL, true, off);
    asks(&f, &f.lead, &f.repo);
}

#[test]
fn a_worktree_start_from_a_space_outside_the_grant_asks_also_in_the_own_space() {
    let f = fixture("outside");
    grant(&f, &[&f.repo], ALL, true);
    asks(&f, &f.lead, &f.other);
    asks(&f, &f.lead, &f.own);
}

#[test]
fn a_worktree_grant_without_start_agent_does_not_cover_a_worktree_start() {
    let f = fixture("no-start");
    grant(&f, &[&f.repo], &["prompt_agent"], true);
    asks(&f, &f.lead, &f.repo);
}

#[test]
fn another_terminal_cannot_use_the_worktree_grant() {
    let f = fixture("other-terminal");
    grant(&f, &[&f.repo], ALL, true);
    let other = sleeper(&f.client, &f.own, "other");
    asks(&f, &other, &f.repo);
}

#[test]
fn a_failed_join_closes_the_new_space_and_fails_the_start() {
    let f = fixture("join-fails");
    grant(&f, &[&f.repo], ALL, true);
    let (before, terminals) = (
        space_count(&f),
        f.client.list_terminals(None).unwrap().len(),
    );
    // `main` is checked out in the main checkout: the space made on it is no linked worktree.
    let r = start_in_worktree(&f, &f.lead, &f.repo, "main");
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("could not join"), "{}", text(&r));
    assert_eq!(space_count(&f), before, "the new space is closed");
    assert_eq!(f.client.list_terminals(None).unwrap().len(), terminals);
    assert_eq!(granted_spaces(&f), std::slice::from_ref(&f.repo));
}

fn add(f: &Fixture, caller: &str, grant: &str, source: &str, space: &str) -> bool {
    f.client
        .add_worktree_space_to_grant(&AgentGrantAddWorktreeSpace {
            caller_terminal_id: caller.to_owned(),
            grant: grant.to_owned(),
            source_space_id: source.to_owned(),
            space_id: space.to_owned(),
        })
        .is_ok()
}

/// A linked worktree of the repository `repo` (a directory of the fixture) on `branch`, and a
/// space on it.
fn worktree_space(f: &Fixture, repo: &str, branch: &str) -> String {
    let path = f.dir.join(format!("{repo}-{branch}"));
    git(
        &f.dir.join(repo),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
        ],
    );
    space_on(&f.client, &path)
}

#[test]
fn the_add_refuses_another_repository_another_caller_and_a_grant_without_worktrees() {
    let f = fixture("add");
    let (id, _) = grant(&f, &[&f.repo], ALL, true);
    let ours = worktree_space(&f, "repo", "a1");
    let theirs = worktree_space(&f, "other", "b1");
    let other = sleeper(&f.client, &f.own, "other");

    assert!(
        !add(&f, &f.lead, &id, &f.repo, &theirs),
        "another repository"
    );
    assert!(!add(&f, &other, &id, &f.repo, &ours), "another caller");
    assert!(
        !add(&f, &f.lead, &id, &f.other, &ours),
        "a source outside the grant"
    );
    assert!(!add(&f, &f.lead, &id, &f.repo, &f.other), "a main checkout");
    assert!(
        !add(&f, &f.lead, "grant-nope", &f.repo, &ours),
        "an unknown grant"
    );
    assert_eq!(
        granted_spaces(&f),
        std::slice::from_ref(&f.repo),
        "nothing was added"
    );

    assert!(add(&f, &f.lead, &id, &f.repo, &ours));
    assert_eq!(granted_spaces(&f), [f.repo.clone(), ours]);

    // A new grant without worktrees replaces the first: neither adds a space now.
    let (plain, _) = grant(&f, &[&f.repo], ALL, false);
    let next = worktree_space(&f, "repo", "a2");
    assert!(!add(&f, &f.lead, &plain, &f.repo, &next), "no worktrees");
    assert!(!add(&f, &f.lead, &id, &f.repo, &next), "a replaced grant");
    assert_eq!(granted_spaces(&f), std::slice::from_ref(&f.repo));
}
