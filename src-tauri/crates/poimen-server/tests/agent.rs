//! Starting an agent and prompting it against a fake agent: a shell script that sends hook
//! reports through the real `poimen-server hook claude`, like Claude Code does.
//!
//! Unix-only on purpose: the fake agent is a `#!/bin/sh` script and the typed command line is
//! POSIX-quoted. On Windows the hook paths are covered by the unit tests of `claude.rs` and
//! `copilot.rs` (and the manual check of #417).
#![cfg(unix)]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::agent::{AgentError, PromptResult};
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-agent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn client(&self) -> Client {
        let config = Config {
            socket: self.dir.join("run").join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let server = Server::bind(config.clone()).unwrap();
        thread::spawn(move || server.serve());
        Client::new(&config.socket)
    }

    /// The fake agent: on start it reports `SessionStart` (after `delay` seconds, none: never);
    /// for each line read it reports `UserPromptSubmit`, records the line, and then reports
    /// `Stop`, or a permission `Notification` when the line is `block`.
    fn agent(&self, delay: Option<&str>) -> String {
        let hook = env!("CARGO_BIN_EXE_poimen-server");
        let log = self.dir.join("prompts.log");
        let report = |event: &str, extra: &str| {
            format!(
                "printf '{{\"hook_event_name\":\"{event}\",\"session_id\":\"fake-1\"{extra}}}' | '{hook}' hook claude"
            )
        };
        let start = match delay {
            Some(d) => format!("sleep {d}\n{}", report("SessionStart", "")),
            None => "sleep 60".into(),
        };
        let script = format!(
            "#!/bin/sh\n{start}\nwhile IFS= read -r line; do\n  {}\n  echo \"$line\" >> '{}'\n  sleep 0.3\n  if [ \"$line\" = block ]; then {}; else {}; fi\ndone\n",
            report("UserPromptSubmit", ""),
            log.display(),
            report(
                "Notification",
                ",\"notification_type\":\"permission_prompt\""
            ),
            report("Stop", ""),
        );
        let path = self.dir.join("fake-agent");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        path.display().to_string()
    }

    fn prompts(&self) -> String {
        std::fs::read_to_string(self.dir.join("prompts.log")).unwrap_or_default()
    }

    fn terminal(&self, client: &Client) -> String {
        let work = self.dir.join("work").canonicalize().unwrap();
        let space = client
            .create_space("api", &work.display().to_string())
            .unwrap();
        client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(20));
    }
}

fn status(client: &Client, id: &str) -> Option<AgentStatus> {
    let list = client.list_terminals(None).unwrap();
    list.iter()
        .find(|t| t.id == id)
        .and_then(|t| t.agent.as_ref().map(|a| a.status))
}

const LONG: Duration = Duration::from_secs(10);

#[test]
fn start_resolves_with_the_session_start_report_then_a_prompt_reaches_the_agent() {
    let env = Env::new("flow");
    let client = env.client();
    let id = env.terminal(&client);
    let agent = env.agent(Some("0.3"));

    let started = Instant::now();
    let info = client.start_agent(&id, &agent, &[], LONG).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(info.status, AgentStatus::Idle);
    assert_eq!(info.session_id.as_deref(), Some("fake-1"));
    assert_eq!(info.agent.as_deref(), Some("claude"));

    let result = client.prompt_agent(&id, "fix the bug", LONG).unwrap();
    assert_eq!(result, PromptResult::Started(AgentStatus::Working));
    wait_until("the prompt", || env.prompts() == "fix the bug\n");
    wait_until("done after the turn", || {
        status(&client, &id) == Some(AgentStatus::Done)
    });
}

#[test]
fn a_prompt_is_refused_while_the_agent_works_or_is_blocked() {
    let env = Env::new("refused");
    let client = env.client();
    let id = env.terminal(&client);
    client
        .start_agent(&id, &env.agent(Some("0")), &[], LONG)
        .unwrap();

    // Blocked at a permission dialog: the next prompt is refused and not typed.
    assert_eq!(
        client.prompt_agent(&id, "block", LONG).unwrap(),
        PromptResult::Started(AgentStatus::Working)
    );
    wait_until("blocked", || {
        status(&client, &id) == Some(AgentStatus::Blocked)
    });
    assert_eq!(
        client.prompt_agent(&id, "second", LONG).unwrap(),
        PromptResult::Blocked
    );
    assert_eq!(env.prompts(), "block\n");
}

#[test]
fn start_times_out_when_the_agent_never_reports() {
    let env = Env::new("timeout");
    let client = env.client();
    let id = env.terminal(&client);
    let silent = env.agent(None);
    let err = client
        .start_agent(&id, &silent, &[], Duration::from_millis(400))
        .unwrap_err();
    assert!(matches!(err, AgentError::Timeout(_)), "{err}");
    assert!(matches!(
        client.prompt_agent(&id, "hi", LONG).unwrap(),
        PromptResult::NoAgent
    ));
}

#[test]
fn start_in_a_terminal_with_an_agent_types_nothing() {
    let env = Env::new("twice");
    let client = env.client();
    let id = env.terminal(&client);
    client
        .start_agent(&id, &env.agent(Some("0")), &[], LONG)
        .unwrap();
    let err = client
        .start_agent(&id, &env.agent(Some("0")), &[], LONG)
        .unwrap_err();
    assert!(matches!(err, AgentError::AlreadyRunning));
}

#[test]
fn start_in_an_unknown_terminal_is_not_found() {
    let env = Env::new("missing");
    let client = env.client();
    let err = client.start_agent("t99", "true", &[], LONG).unwrap_err();
    assert!(matches!(err, AgentError::NotFound(_)));
}

#[test]
fn an_agent_without_hooks_is_started_without_waiting_and_shows_no_status() {
    let env = Env::new("unreported");
    let client = env.client();
    let id = env.terminal(&client);
    let marker = env.dir.join("started");

    let started = Instant::now();
    client
        .start_unreported(&id, "touch", &[marker.display().to_string()])
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    wait_until("the typed command", || marker.exists());
    assert_eq!(status(&client, &id), None);
    // Hook kinds and plain shells stay refused ...
    assert!(matches!(
        client.prompt_agent(&id, "hi", LONG).unwrap(),
        PromptResult::NoAgent
    ));
    // ... a hook-less agent is prompted by the caller that started it.
    let typed = env.dir.join("prompted");
    client
        .prompt_unreported(&id, &format!("touch {}", typed.display()))
        .unwrap();
    wait_until("the prompt typed", || typed.exists());
}

#[test]
fn prompt_unreported_in_an_unknown_terminal_is_not_found() {
    let env = Env::new("prompt-unreported-missing");
    let client = env.client();
    let err = client.prompt_unreported("t99", "hi").unwrap_err();
    assert!(matches!(err, AgentError::NotFound(_)));
}

#[test]
fn start_unreported_in_an_unknown_terminal_is_not_found() {
    let env = Env::new("unreported-missing");
    let client = env.client();
    let err = client.start_unreported("t99", "true", &[]).unwrap_err();
    assert!(matches!(err, AgentError::NotFound(_)));
}
