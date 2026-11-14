//! `agent.transcript` (#698): the agent transcript of a terminal, read on the server's machine,
//! against a real server on a temp socket and a temp home with fake transcripts. Never the user's
//! home or server.
#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::thread;

use poimen_client::protocol::{AgentTranscriptResult, ErrorCode, TerminalCreate, TranscriptState};
use poimen_client::{Client, ClientError};
use poimen_core::transcripts::{project_dir_name, projects_root};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

const SESSION: &str = "abc-123";
const BASH: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-02T04:07:54.000Z","message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}}"#;
const BASH_2: &str = r#"{"type":"assistant","uuid":"a2","timestamp":"2026-11-02T04:07:58.000Z","message":{"id":"m2","role":"assistant","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"pwd"}}]}}"#;

struct Env {
    home: PathBuf,
    work: PathBuf,
    client: Client,
    terminal: String,
}

impl Env {
    fn new(name: &str, with_home: bool) -> Self {
        let dir =
            std::env::temp_dir().join(format!("poimen-transcript-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            scrollback: 1000,
            home: with_home.then(|| dir.join("home")),
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
            home: dir.join("home"),
            work,
            client,
            terminal,
        }
    }

    fn report(&self, seq: u64, agent: &str, session: &str) {
        self.client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": "working",
                       "agent": agent, "session_id": session}),
            )
            .unwrap();
    }

    fn transcript_file(&self) -> PathBuf {
        projects_root(&self.home)
            .join(project_dir_name(&self.work.display().to_string()))
            .join(format!("{SESSION}.jsonl"))
    }

    fn append(&self, line: &str) {
        let path = self.transcript_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }

    fn read(&self, follower: &str, restart: bool) -> AgentTranscriptResult {
        self.client
            .agent_transcript(&self.terminal, follower, restart)
            .unwrap()
    }
}

/// The count of plain tool calls of each `main` update.
fn tools(result: &AgentTranscriptResult) -> Vec<u64> {
    result
        .updates
        .iter()
        .filter(|u| u["kind"] == "main")
        .map(|u| u["tools"].as_u64().unwrap())
        .collect()
}

#[test]
fn an_existing_transcript_gives_its_updates_once_per_follower() {
    let env = Env::new("existing", true);
    env.append(BASH);
    env.report(1, "claude", SESSION);

    let first = env.read("panel", false);
    assert_eq!(first.state, TranscriptState::Ok);
    assert_eq!(first.session.as_deref(), Some(SESSION));
    assert!(first.fresh);
    assert_eq!(tools(&first), vec![1]);

    let quiet = env.read("panel", false);
    assert!(!quiet.fresh && quiet.updates.is_empty());

    env.append(BASH_2);
    assert_eq!(tools(&env.read("panel", false)), vec![2]);

    // Another view of the same terminal starts at the beginning; the first is not disturbed.
    let other = env.read("flow", false);
    assert!(other.fresh);
    assert_eq!(tools(&other), vec![2]);
    assert!(env.read("panel", false).updates.is_empty());

    let again = env.read("panel", true);
    assert!(again.fresh);
    assert_eq!(tools(&again), vec![2]);
}

#[test]
fn a_new_session_starts_the_reader_over() {
    let env = Env::new("session", true);
    env.append(BASH);
    env.report(1, "claude", SESSION);
    assert!(env.read("panel", false).fresh);
    env.report(2, "claude", "other-1");
    // The new session has no file yet.
    assert_eq!(env.read("panel", false).state, TranscriptState::Missing);
}

#[test]
fn a_session_without_a_transcript_file_is_missing_until_it_appears() {
    let env = Env::new("missing", true);
    env.report(1, "claude", SESSION);
    let r = env.read("panel", false);
    assert_eq!(r.state, TranscriptState::Missing);
    assert!(r.updates.is_empty());
    env.append(BASH);
    let r = env.read("panel", false);
    assert_eq!((r.state, tools(&r)), (TranscriptState::Ok, vec![1]));
}

#[test]
fn a_transcript_that_cannot_be_opened_is_unreadable_with_the_reason() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new("unreadable", true);
    env.append(BASH);
    env.report(1, "claude", SESSION);
    let path = env.transcript_file();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let r = env.read("panel", false);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    if r.state == TranscriptState::Ok {
        return; // running as root: permissions do not apply
    }
    assert_eq!(r.state, TranscriptState::Unreadable);
    assert!(r.reason.unwrap().contains(SESSION));
    assert!(r.updates.is_empty());
}

#[test]
fn a_terminal_without_a_followable_agent_has_no_agent() {
    let env = Env::new("noagent", true);
    assert_eq!(env.read("panel", false).state, TranscriptState::NoAgent);
    env.report(1, "codex", SESSION);
    assert_eq!(env.read("panel", false).state, TranscriptState::NoAgent);
}

#[test]
fn a_server_without_a_home_finds_no_transcript() {
    let env = Env::new("nohome", false);
    env.report(1, "claude", SESSION);
    assert_eq!(env.read("panel", false).state, TranscriptState::Missing);
}

#[test]
fn an_unknown_terminal_is_not_found() {
    let env = Env::new("unknown", true);
    match env.client.agent_transcript("nope", "panel", false) {
        Err(ClientError::Server { code, .. }) => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("expected not_found, got {other:?}"),
    }
}
