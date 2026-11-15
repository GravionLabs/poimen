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

fn wait_for(env: &Env, want: Option<AgentStatus>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while env.status() != want {
        assert!(Instant::now() < deadline, "status stays {:?}", env.status());
        thread::sleep(Duration::from_millis(20));
    }
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
fn a_child_record_is_ignored() {
    let env = Env::new("child");
    env.print(r"\033]7501;state=working:id=sub\007\033]7501;state=error\007");
    // `error` maps to done for now.
    wait_for(&env, Some(AgentStatus::Done));
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
