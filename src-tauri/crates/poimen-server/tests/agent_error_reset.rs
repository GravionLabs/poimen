//! A failed turn (`error`) is reset by input with Enter, owned by the server (#1110).

use std::path::PathBuf;
use std::thread;

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

struct Env {
    dir: PathBuf,
    client: Client,
    terminal: String,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("poimen-errreset-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
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

    fn report(&self, seq: u64, status: &str) {
        self.client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": status}),
            )
            .unwrap();
    }

    fn input(&self, data: &str) {
        self.client
            .request(
                "terminal.input",
                json!({"terminal_id": self.terminal, "data": data}),
            )
            .unwrap();
    }

    fn status(&self) -> AgentStatus {
        self.client
            .list_terminals(None)
            .unwrap()
            .remove(0)
            .agent
            .expect("agent")
            .status
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn enter_resets_error_to_idle() {
    let env = Env::new("reset");
    env.report(1, "error");
    assert_eq!(env.status(), AgentStatus::Error);
    env.input("typing");
    assert_eq!(env.status(), AgentStatus::Error);
    env.input("clear\r");
    assert_eq!(env.status(), AgentStatus::Idle);
}

#[test]
fn enter_leaves_other_statuses_alone() {
    let env = Env::new("other");
    env.report(1, "blocked");
    env.input("\r");
    assert_eq!(env.status(), AgentStatus::Blocked);
}
