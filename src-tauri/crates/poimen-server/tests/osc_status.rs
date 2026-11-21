//! The agent status a program reports with OSC 7501 (#681), end to end through a real terminal.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, Event, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

struct Env {
    dir: PathBuf,
    client: Client,
    terminal: String,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-osc-{}-{name}", std::process::id()));
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
            .create_space("api", &work.display().to_string())
            .unwrap();
        let terminal = client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id;
        Self {
            dir,
            client,
            terminal,
        }
    }

    fn status(&self) -> Option<AgentStatus> {
        let t = self.client.list_terminals(None).unwrap().remove(0);
        t.agent.map(|a| a.status)
    }

    fn hook(&self, seq: u64, status: &str) {
        self.client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": status}),
            )
            .unwrap();
    }

    /// Runs a shell command that prints a report.
    fn print(&self, escape: &str) {
        self.client
            .input(&self.terminal, &format!("printf '{escape}'\n"))
            .unwrap();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn events(client: &Client) -> Receiver<Event> {
    let stream = client.subscribe().unwrap();
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

fn next_status(rx: &Receiver<Event>) -> (AgentStatus, Option<String>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("status event did not arrive") {
            Event::AgentStatusChanged { status, agent, .. } => return (status, agent),
            _ => continue,
        }
    }
}

/// Polls `ok` for up to 10 s; `what` names the state that did not arrive.
fn poll(what: impl Fn() -> String, ok: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "{}", what());
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for(env: &Env, want: Option<AgentStatus>) {
    poll(
        || format!("status stays {:?}", env.status()),
        || env.status() == want,
    );
}

#[test]
fn a_printed_report_sets_the_status_and_publishes_one_event() {
    let env = Env::new("print");
    let rx = events(&env.client);
    env.print(r"\033]7501;state=working:app=codex\033\\");
    assert_eq!(
        next_status(&rx),
        (AgentStatus::Working, Some("codex".into()))
    );
    let t = env.client.list_terminals(None).unwrap().remove(0);
    assert_eq!(t.agent.unwrap().agent.as_deref(), Some("codex"));
    // The same state again is no change.
    env.print(r"\033]7501;state=working:app=codex\007");
    env.print(r"\033]7501;state=blocked:app=codex\007");
    assert_eq!(next_status(&rx).0, AgentStatus::Blocked);
}

#[test]
fn the_report_wins_over_hooks_until_it_is_cleared() {
    let env = Env::new("precedence");
    env.print(r"\033]7501;state=working:app=codex\007");
    wait_for(&env, Some(AgentStatus::Working));
    env.hook(1, "done");
    assert_eq!(env.status(), Some(AgentStatus::Working));
    env.print(r"\033]7501;state=clear\007");
    // The hook status that arrived meanwhile applies again.
    wait_for(&env, Some(AgentStatus::Done));
}

#[test]
fn a_new_prompt_drops_working_but_not_done() {
    let env = Env::new("prompt");
    env.print(r"\033]7501;state=working\007");
    wait_for(&env, Some(AgentStatus::Working));
    env.print(r"\033]133;A\007");
    wait_for(&env, None);
    env.print(r"\033]7501;state=done\007\033]133;A\007");
    wait_for(&env, Some(AgentStatus::Done));
    thread::sleep(Duration::from_millis(200));
    assert_eq!(env.status(), Some(AgentStatus::Done));
}

#[test]
fn a_child_record_leaves_the_root_status_alone() {
    let env = Env::new("child");
    env.print(r"\033]7501;state=working:id=sub\007\033]7501;state=error\007");
    wait_for(&env, Some(AgentStatus::Error));
}

fn records(env: &Env) -> Vec<(String, AgentStatus, Option<String>)> {
    let t = env.client.list_terminals(None).unwrap().remove(0);
    t.records
        .into_iter()
        .map(|r| (r.id, r.status, r.app))
        .collect()
}

fn wait_records(env: &Env, want: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while records(env).len() != want {
        assert!(Instant::now() < deadline, "records stay {:?}", records(env));
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn child_records_are_listed_published_and_cleared() {
    let env = Env::new("records");
    let rx = events(&env.client);
    env.print(r"\033]7501;state=working:app=codex\007");
    env.print(r"\033]7501;state=working:id=run\007\033]7501;state=blocked:id=run/a\007");
    let deadline = Instant::now() + Duration::from_secs(10);
    let got = loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("records event did not arrive")
        {
            Event::TerminalRecordsChanged { records, .. } if records.len() == 2 => break records,
            _ => {}
        }
    };
    assert_eq!(got[1].id, "run/a");
    // `app` comes from the root record, the root status stays.
    assert_eq!(got[1].app.as_deref(), Some("codex"));
    assert_eq!(env.status(), Some(AgentStatus::Working));
    // An invalid id drops the report.
    env.print(r"\033]7501;state=working:id=a//b\007");
    env.print(r"\033]7501;state=clear:id=run\007");
    wait_records(&env, 0);
    env.print(r"\033]7501;state=done:id=x\007\033]7501;state=working:id=y\007");
    wait_records(&env, 2);
    // A new prompt drops the working one.
    env.print(r"\033]133;A\007");
    wait_records(&env, 1);
    assert_eq!(records(&env)[0].0, "x");
    env.print(r"\033]7501;state=clear\007");
    wait_records(&env, 0);
    assert_eq!(env.status(), None);
}

#[test]
fn error_progress_kind_and_title_reach_the_terminal_and_the_event() {
    let env = Env::new("rich");
    let rx = events(&env.client);
    // title "build <RLO>x" (a bidi override), as base64 of UTF-8.
    env.print(r"\033]7501;state=working:progress=40:title=YnVpbGQg4oCueA==\007");
    let deadline = Instant::now() + Duration::from_secs(10);
    let event = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("status event did not arrive") {
            e @ Event::AgentStatusChanged { .. } => break e,
            _ => continue,
        }
    };
    let Event::AgentStatusChanged {
        status,
        progress,
        title,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!((status, progress), (AgentStatus::Working, Some(40)));
    assert_eq!(title.as_deref(), Some("build x"));
    env.print(r"\033]7501;state=blocked:kind=auth\007");
    let deadline = Instant::now() + Duration::from_secs(10);
    let agent = loop {
        let a = env.client.list_terminals(None).unwrap().remove(0).agent;
        if a.as_ref().is_some_and(|a| a.status == AgentStatus::Blocked) {
            break a.unwrap();
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    };
    // The kind belongs to the block; the progress of the earlier report is gone.
    assert_eq!(agent.kind.as_deref(), Some("auth"));
    assert_eq!(agent.progress, None);
    env.print(r"\033]7501;state=error:kind=auth\007");
    wait_for(&env, Some(AgentStatus::Error));
    let agent = env
        .client
        .list_terminals(None)
        .unwrap()
        .remove(0)
        .agent
        .unwrap();
    assert_eq!(agent.kind, None);
}

#[test]
fn working_ends_with_the_process() {
    let env = Env::new("exit");
    env.print(r"\033]7501;state=working\007");
    wait_for(&env, Some(AgentStatus::Working));
    let rx = events(&env.client);
    env.client.input(&env.terminal, "exit\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        rx.recv_timeout(deadline.saturating_duration_since(Instant::now())),
        Ok(Event::TerminalExited { .. })
    ) {}
}

fn tone(env: &Env) -> Option<String> {
    env.client
        .list_terminals(None)
        .unwrap()
        .remove(0)
        .agent
        .and_then(|a| a.tone)
}

fn wait_tone(env: &Env, want: Option<&str>) {
    poll(
        || format!("tone stays {:?}", tone(env)),
        || tone(env).as_deref() == want,
    );
}

#[test]
fn a_warning_tone_belongs_to_done_and_lives_like_done() {
    let env = Env::new("tone");
    let rx = events(&env.client);
    // Another state ignores the tone; the state still counts.
    env.print(r"\033]7501;state=working:tone=warning\007");
    wait_for(&env, Some(AgentStatus::Working));
    assert_eq!(tone(&env), None);
    env.print(r"\033]7501;state=done:tone=warning\007");
    wait_tone(&env, Some("warning"));
    let tone_of_event = loop {
        let left = Duration::from_secs(10);
        match rx.recv_timeout(left).expect("status event did not arrive") {
            Event::AgentStatusChanged {
                status: AgentStatus::Done,
                tone,
                ..
            } => break tone,
            _ => continue,
        }
    };
    assert_eq!(tone_of_event.as_deref(), Some("warning"));
    // A new prompt (OSC 133 A) keeps a done status and its tone.
    // A child record reported after the prompt shows the prompt was handled first.
    env.print(r"\033]133;A\007\033]7501;state=working:id=run/a\007");
    wait_records(&env, 1);
    assert_eq!(env.status(), Some(AgentStatus::Done));
    assert_eq!(tone(&env).as_deref(), Some("warning"));
    env.print(r"\033]7501;state=clear:id=run/a\007");
    // A later report without the tone clears it.
    env.print(r"\033]7501;state=done\007");
    wait_tone(&env, None);
    assert_eq!(env.status(), Some(AgentStatus::Done));
    // RIS ends it.
    env.print(r"\033]7501;state=done:tone=warning\007");
    wait_tone(&env, Some("warning"));
    env.print(r"\033c");
    wait_for(&env, None);
    // state=clear ends it too.
    env.print(r"\033]7501;state=done:tone=warning\007");
    wait_tone(&env, Some("warning"));
    env.print(r"\033]7501;state=clear\007");
    wait_for(&env, None);
}

#[test]
fn the_status_history_keeps_changes_with_their_source_and_at_most_twenty() {
    let env = Env::new("history");
    let history = || {
        let t = env.client.list_terminals(None).unwrap().remove(0);
        (t.agent_source, t.status_history)
    };
    assert_eq!(history(), (None, Vec::new()));
    env.hook(1, "working");
    // The same status again is no change.
    env.hook(2, "working");
    env.print(r"\033]7501;state=done:app=deliver\033\\");
    wait_for(&env, Some(AgentStatus::Done));
    let (source, changes) = history();
    assert_eq!(source.as_deref(), Some("osc7501"));
    let seen: Vec<_> = changes
        .iter()
        .map(|c| (c.status, c.source.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        seen,
        [
            (AgentStatus::Working, String::new()),
            (AgentStatus::Done, "osc7501".to_string())
        ]
    );
    assert!(changes[0].at <= changes[1].at);

    // Clearing the program's status brings the hook's back, as a change.
    env.print(r"\033]7501;state=clear\033\\");
    wait_for(&env, Some(AgentStatus::Working));
    assert_eq!(history().1.len(), 3);

    // Many changes: only the last twenty stay, newest last.
    for i in 0..25 {
        let state = if i % 2 == 0 { "blocked" } else { "working" };
        env.print(&format!(r"\033]7501;state={state}\033\\"));
        let want = if i % 2 == 0 {
            AgentStatus::Blocked
        } else {
            AgentStatus::Working
        };
        wait_for(&env, Some(want));
    }
    let changes = history().1;
    assert_eq!(changes.len(), poimen_client::protocol::MAX_STATUS_HISTORY);
    assert_eq!(changes.last().unwrap().status, AgentStatus::Blocked);
}
