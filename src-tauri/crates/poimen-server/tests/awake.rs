//! Keeping the machine awake (#987) against a fake agent that reports through the real
//! `poimen-server hook claude`, and a fake inhibitor (a shell that writes its pid and waits on its
//! standard input). The clock is the `now_ms` the test gives the sweeper.
//!
//! Unix-only like `hibernate.rs`: the fake agent and inhibitor are `/bin/sh`.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, Event, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};

const LONG: Duration = Duration::from_secs(10);
const SECOND_MS: u64 = 1000;

struct Env {
    dir: PathBuf,
    sweep: Box<dyn Fn(u64) + Send + Sync>,
    client: Client,
    config_path: PathBuf,
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

impl Env {
    fn new(name: &str, config: Option<&str>) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-awake-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let home = dir.join("home");
        let config_path = poimen_core::settings::config_path(&home);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        if let Some(text) = config {
            std::fs::write(&config_path, text).unwrap();
        }
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            scrollback: 1000,
            home: Some(home),
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let server = Server::bind(config.clone()).unwrap();
        server.set_awake_command(vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo $$ > '{}'; cat >/dev/null",
                dir.join("holder.pid").display()
            ),
        ]);
        let sweep = Box::new(server.awake_sweeper());
        thread::spawn(move || server.serve());
        let client = Client::new(&config.socket);
        Self {
            dir,
            sweep,
            client,
            config_path,
        }
    }

    /// The fake agent: reports `UserPromptSubmit` for a prompt line and `Stop` once the file
    /// `stop` exists.
    fn agent(&self) -> String {
        let hook = env!("CARGO_BIN_EXE_poimen-server");
        let report = |event: &str| {
            format!(
                "printf '{{\"hook_event_name\":\"{event}\",\"session_id\":\"fake-1\"}}' | '{hook}' hook claude"
            )
        };
        let script = format!(
            "#!/bin/sh\n{}\nwhile IFS= read -r line; do\n  {}\n  while [ ! -e '{}' ]; do sleep 0.1; done\n  {}\ndone\n",
            report("SessionStart"),
            report("UserPromptSubmit"),
            self.dir.join("stop").display(),
            report("Stop"),
        );
        let path = self.dir.join("fake-agent");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        path.display().to_string()
    }

    /// An agent that is `working`; returns its terminal id.
    fn working_agent(&self) -> String {
        let work = self.dir.join("work").canonicalize().unwrap();
        let space = self
            .client
            .create_space("api", &work.display().to_string())
            .unwrap();
        let id = self
            .client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id;
        self.client
            .start_agent(&id, &self.agent(), &[], LONG)
            .unwrap();
        self.client.prompt_agent(&id, "work", LONG).unwrap();
        self.wait_status(&id, AgentStatus::Working);
        id
    }

    fn wait_status(&self, id: &str, status: AgentStatus) {
        wait_until("the agent status", || {
            self.client
                .list_terminals(None)
                .unwrap()
                .into_iter()
                .find(|t| t.id == id)
                .and_then(|t| t.agent)
                .is_some_and(|a| a.status == status)
        });
    }

    /// Whether the fake inhibitor runs.
    fn held(&self) -> bool {
        let Ok(pid) = std::fs::read_to_string(self.dir.join("holder.pid")) else {
            return false;
        };
        Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + LONG;
    while !ok() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(20));
    }
}

const ON: &str = r#"{"agents":{"keepAwake":true}}"#;

#[test]
fn a_working_agent_holds_the_machine_and_the_hold_ends_after_the_delay() {
    let env = Env::new("hold", Some(ON));
    let id = env.working_agent();
    let t0 = now_ms();

    (env.sweep)(t0);
    wait_until("the hold", || env.held());

    std::fs::write(env.dir.join("stop"), "").unwrap();
    env.wait_status(&id, AgentStatus::Done);
    // Idle from here: the hold stays for the delay, then ends.
    (env.sweep)(t0 + SECOND_MS);
    (env.sweep)(t0 + 30 * SECOND_MS);
    assert!(env.held(), "30 seconds idle is too early");
    (env.sweep)(t0 + 62 * SECOND_MS);
    wait_until("the release", || !env.held());
}

#[test]
fn work_again_inside_the_delay_keeps_the_hold() {
    let env = Env::new("again", Some(ON));
    let id = env.working_agent();
    let t0 = now_ms();
    (env.sweep)(t0);
    wait_until("the hold", || env.held());
    let pid = std::fs::read_to_string(env.dir.join("holder.pid")).unwrap();

    std::fs::write(env.dir.join("stop"), "").unwrap();
    env.wait_status(&id, AgentStatus::Done);
    (env.sweep)(t0 + SECOND_MS);
    // A new prompt: working again before the delay ran out.
    std::fs::remove_file(env.dir.join("stop")).unwrap();
    env.client.prompt_agent(&id, "more", LONG).unwrap();
    env.wait_status(&id, AgentStatus::Working);
    (env.sweep)(t0 + 40 * SECOND_MS);
    (env.sweep)(t0 + 100 * SECOND_MS);
    assert!(env.held());
    assert_eq!(
        std::fs::read_to_string(env.dir.join("holder.pid")).unwrap(),
        pid,
        "the same hold, not a new one"
    );
}

#[test]
fn nothing_is_held_while_the_setting_is_off_or_absent() {
    for (name, config) in [
        ("absent", None),
        ("false", Some(r#"{"agents":{"keepAwake":false}}"#)),
        ("invalid", Some(r#"{"agents":{"keepAwake":"yes"}}"#)),
    ] {
        let env = Env::new(name, config);
        env.working_agent();
        (env.sweep)(now_ms());
        thread::sleep(Duration::from_millis(300));
        assert!(!env.dir.join("holder.pid").exists(), "{name}");
    }
}

#[test]
fn switching_the_setting_off_releases_at_once() {
    let env = Env::new("off", Some(ON));
    env.working_agent();
    let t0 = now_ms();
    (env.sweep)(t0);
    wait_until("the hold", || env.held());
    std::fs::write(&env.config_path, r#"{"agents":{"keepAwake":false}}"#).unwrap();
    (env.sweep)(t0 + SECOND_MS);
    wait_until("the release", || !env.held());
}

#[test]
fn a_new_subscription_learns_that_the_machine_is_held() {
    let env = Env::new("event", Some(ON));
    env.working_agent();
    (env.sweep)(now_ms());
    wait_until("the hold", || env.held());
    let stream = env.client.subscribe().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + LONG;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx
            .recv_timeout(left)
            .expect("server.awake_changed on subscribe");
        if matches!(event, Event::ServerAwakeChanged { awake: true }) {
            break;
        }
    }
}
