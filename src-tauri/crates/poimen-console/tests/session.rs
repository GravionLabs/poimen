//! `poimen_client::agent::run_session` (ADR 0016, #744) against an in-process poimen-server on a
//! temporary socket and a fake harness (a shell script that records what it was given and
//! sleeps); the agent's hook reports are sent by the test. Unix-only; never a real agent.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::agent::{
    SessionContext, SessionError, SessionEvent, SessionRequest, run_session,
};
use poimen_client::protocol::{
    AgentStatus, ClientInfo, ClientKind, Event, EventsSubscribe, TerminalCreate, TerminalStatus,
};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

struct Fixture {
    dir: PathBuf,
    client: Client,
    own_space: String,
    other_space: String,
    caller: String,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `harness` is the JSON of the `harnesses` map of the config.
fn fixture(name: &str, harnesses: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-session-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
    let home = dir.join("home");
    let config_file = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    std::fs::write(config_file, format!(r#"{{"harnesses": {harnesses}}}"#)).unwrap();
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
    let space = |name: &str| {
        let path = dir.join(name).canonicalize().unwrap();
        client
            .create_space(name, &path.display().to_string())
            .unwrap()
            .id
    };
    let own_space = space("work");
    let other_space = space("other");
    let caller = client
        .create_terminal(&TerminalCreate {
            space_id: own_space.clone(),
            name: Some("lead".into()),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 60".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    Fixture {
        dir,
        client,
        own_space,
        other_space,
        caller,
        home,
    }
}

/// A harness (the id `claude`, so it counts as reporting hooks) that records its first argument
/// and `AGENT_EVENTS_RUN` under `dir`, then sleeps.
fn recording_harness(dir: &Path, prompt_args: &str) -> String {
    let script = format!(
        "printf '%s' \"$1\" > {d}/arg1; printf '%s' \"$AGENT_EVENTS_RUN:$EXTRA\" > {d}/env; sleep 30",
        d = dir.display()
    );
    json!({"claude": {"command": ["sh", "-c", script, "fake"],
        "promptArgs": serde_json::from_str::<serde_json::Value>(prompt_args).unwrap()}})
    .to_string()
}

fn report(client: &Client, terminal: &str, seq: u64, status: AgentStatus) {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": terminal, "seq": seq, "status": status, "agent": "claude"}),
        )
        .unwrap();
}

/// Plays the agent's hooks for the terminal named `name`: `idle` once it exists, then each of
/// `then` after `after` is true.
fn hooks(
    client: &Client,
    name: &str,
    after: impl Fn() -> bool + Send + 'static,
    then: &[AgentStatus],
) {
    let (client, name, then) = (client.clone(), name.to_owned(), then.to_vec());
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let id = loop {
            let found = client
                .list_terminals(None)
                .unwrap()
                .into_iter()
                .find(|t| t.name == name && t.status == TerminalStatus::Running);
            if let Some(t) = found {
                break t.id;
            }
            assert!(Instant::now() < deadline, "terminal {name} never appeared");
            thread::sleep(Duration::from_millis(50));
        };
        thread::sleep(Duration::from_millis(300));
        report(&client, &id, 1, AgentStatus::Idle);
        while !after() {
            assert!(Instant::now() < deadline, "the agent was never ready");
            thread::sleep(Duration::from_millis(50));
        }
        for (seq, status) in (2..).zip(then) {
            thread::sleep(Duration::from_millis(300));
            report(&client, &id, seq, status);
        }
    });
}

fn request(harness: &str) -> SessionRequest {
    SessionRequest {
        harness: harness.into(),
        prompt: "do it".into(),
        ..Default::default()
    }
}

fn ctx<'a>(f: &'a Fixture, caller: Option<&'a str>) -> SessionContext<'a> {
    SessionContext {
        caller,
        home: Some(&f.home),
        confirm_wait: Duration::from_secs(30),
    }
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

fn wait_for(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn names(f: &Fixture) -> Vec<String> {
    f.client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect()
}

#[test]
fn a_prompt_with_shell_syntax_reaches_the_agent_as_one_argument() {
    let f = fixture("args", "{}");
    let rec = f.dir.join("rec");
    std::fs::create_dir_all(&rec).unwrap();
    std::fs::write(
        poimen_core::settings::config_path(&f.home),
        format!(
            r#"{{"harnesses": {}}}"#,
            recording_harness(&rec, r#"["{prompt}"]"#)
        ),
    )
    .unwrap();
    let prompt = "it's a $(touch /tmp/x); \"q\" `id`\nsecond line | & > f";
    let mut req = request("claude");
    req.prompt = prompt.into();
    req.run = Some("M016".into());
    req.task = Some("744".into());
    req.title = Some("PBI 744".into());
    req.env.insert("EXTRA".into(), "yes".into());
    hooks(
        &f.client,
        "M016 PBI 744",
        {
            let arg = rec.join("arg1");
            move || arg.exists()
        },
        &[AgentStatus::Working, AgentStatus::Done],
    );
    let mut seen = Vec::new();
    let end = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |e| seen.push(e)).unwrap();

    assert_eq!(wait_for(&rec.join("arg1")), prompt);
    assert_eq!(wait_for(&rec.join("env")), "M016:yes");
    assert_eq!(end.status, AgentStatus::Done);
    assert_eq!(end.space, f.own_space);
    assert_eq!(
        seen.first(),
        Some(&SessionEvent::Started {
            terminal: end.terminal.clone(),
            space: end.space.clone()
        })
    );
    assert_eq!(seen.last(), Some(&SessionEvent::Ended(AgentStatus::Done)));
    for status in [AgentStatus::Working, AgentStatus::Done] {
        assert!(seen.contains(&SessionEvent::Status(status)), "{seen:?}");
    }
    let terminals = f.client.list_terminals(None).unwrap();
    let t = terminals.iter().find(|t| t.id == end.terminal).unwrap();
    assert_eq!(t.name, "M016 PBI 744");
    assert_eq!(t.run.as_deref(), Some("M016"));
    assert_eq!(t.task.as_deref(), Some("744"));
    assert_eq!(t.started_by.as_deref(), Some(f.caller.as_str()));
    assert_eq!(t.status, TerminalStatus::Running, "the terminal stays open");
}

#[test]
fn without_prompt_args_the_prompt_is_typed_after_the_start() {
    let f = fixture("typed", "{}");
    let rec = f.dir.join("rec");
    std::fs::create_dir_all(&rec).unwrap();
    let script = format!(
        "read line; printf '%s' \"$line\" > {}/typed; sleep 30",
        rec.display()
    );
    std::fs::write(
        poimen_core::settings::config_path(&f.home),
        json!({"harnesses": {"claude": {"command": ["sh", "-c", script, "fake"]}}}).to_string(),
    )
    .unwrap();
    let mut req = request("claude");
    req.prompt = "say 'hi' $HOME".into();
    req.name = Some("typed-session".into());
    hooks(
        &f.client,
        "typed-session",
        || true,
        &[AgentStatus::Working, AgentStatus::Done],
    );
    let end = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap();
    assert_eq!(end.status, AgentStatus::Done);
    assert_eq!(wait_for(&rec.join("typed")), "say 'hi' $HOME");
}

#[test]
fn an_agent_error_is_a_typed_error_and_the_default_name_uses_the_prompt() {
    let f = fixture(
        "error",
        &recording_harness(Path::new("/tmp"), r#"["{prompt}"]"#),
    );
    let mut req = request("claude");
    req.prompt = "fix the flaky test in the parser module please".into();
    req.run = Some("R1".into());
    hooks(
        &f.client,
        "R1 fix the flaky test in the",
        || true,
        &[AgentStatus::Error],
    );
    let err = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap_err();
    let SessionError::Ended { status, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(status, AgentStatus::Error);
    assert!(names(&f).contains(&"R1 fix the flaky test in the".to_owned()));
}

#[test]
fn a_closed_terminal_ends_the_session_as_gone() {
    let f = fixture(
        "gone",
        &recording_harness(Path::new("/tmp"), r#"["{prompt}"]"#),
    );
    let mut req = request("claude");
    req.name = Some("doomed".into());
    hooks(&f.client, "doomed", || true, &[AgentStatus::Working]);
    let (client, name) = (f.client.clone(), "doomed");
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(2));
        let id = client
            .list_terminals(None)
            .unwrap()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap()
            .id;
        client.close_terminal(&id).unwrap();
    });
    let err = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap_err();
    assert!(
        matches!(
            err,
            SessionError::Ended {
                status: AgentStatus::Gone,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_harness_without_hooks_or_unknown_starts_nothing() {
    let f = fixture("refused", r#"{"plain": {"command": ["sleep", "20"]}}"#);
    let before = names(&f);
    let err = run_session(
        &f.client,
        &ctx(&f, Some(&f.caller)),
        &request("plain"),
        |_| {},
    )
    .unwrap_err();
    let SessionError::StartFailed(reason) = err else {
        panic!("{err:?}");
    };
    assert!(reason.contains("hooks"), "{reason}");
    let err = run_session(
        &f.client,
        &ctx(&f, Some(&f.caller)),
        &request("nope"),
        |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(&err, SessionError::StartFailed(r) if r.contains("unknown harness")),
        "{err:?}"
    );
    let mut req = request("claude");
    req.space = Some("no-such-space".into());
    let err = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap_err();
    assert!(matches!(err, SessionError::StartFailed(_)), "{err:?}");
    assert_eq!(names(&f), before);
}

#[test]
fn another_space_needs_the_app_and_a_caller_is_required() {
    let f = fixture(
        "perm",
        &recording_harness(Path::new("/tmp"), r#"["{prompt}"]"#),
    );
    let before = names(&f);
    let mut req = request("claude");
    req.space = Some(f.other_space.clone());

    // No app is connected: nobody can confirm.
    let err = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap_err();
    assert!(matches!(err, SessionError::PermissionRefused(_)), "{err:?}");

    // No calling terminal: no space of its own, so no space by default and no free write.
    let err = run_session(&f.client, &ctx(&f, None), &request("claude"), |_| {}).unwrap_err();
    assert!(matches!(err, SessionError::PermissionRefused(_)), "{err:?}");
    let err = run_session(&f.client, &ctx(&f, None), &req, |_| {}).unwrap_err();
    assert!(matches!(err, SessionError::PermissionRefused(_)), "{err:?}");

    // A declined confirmation refuses.
    let rx = app(&f.client);
    let answer = answer(&f.client, rx, false);
    let err = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap_err();
    assert!(
        matches!(&err, SessionError::PermissionRefused(r) if r.contains("declined")),
        "{err:?}"
    );
    answer.join().unwrap();
    assert_eq!(names(&f), before);
}

#[test]
fn another_space_runs_after_the_user_approved() {
    let f = fixture(
        "approve",
        &recording_harness(Path::new("/tmp"), r#"["{prompt}"]"#),
    );
    let rx = app(&f.client);
    let answered = answer(&f.client, rx, true);
    let mut req = request("claude");
    req.space = Some(f.other_space.clone());
    req.name = Some("elsewhere".into());
    hooks(&f.client, "elsewhere", || true, &[AgentStatus::Done]);
    let end = run_session(&f.client, &ctx(&f, Some(&f.caller)), &req, |_| {}).unwrap();
    assert_eq!(end.space, f.other_space);
    answered.join().unwrap();
}

fn answer(client: &Client, rx: Receiver<Event>, approve: bool) -> thread::JoinHandle<()> {
    let client = client.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = rx.recv_timeout(left).expect("a confirmation in time");
            if let Event::AgentConfirmRequested { id, .. } = event {
                client.answer_agent_confirm(&id, approve).unwrap();
                return;
            }
        }
    })
}
