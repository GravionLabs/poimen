//! `--machine` (#1176): the arguments, and the commands over a pipe that is a local
//! `poimen-server stdio` bridge instead of ssh (an in-process server on a temp socket).
#![cfg(unix)]

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::pipe::{Connector, Pipe};
use poimen_console::cli::{self, Command, Request};
use poimen_console::{commands, remote};
use poimen_server::{Config, PtyHost, Server, stdio};

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn machine_comes_before_a_command_or_alone() {
    let inv = cli::parse_invocation(&args(&["--machine", "box", "ls", "--json"])).unwrap();
    assert_eq!(inv.machine.as_deref(), Some("box"));
    assert_eq!(inv.command, Command::Request(Request::Ls { json: true }));
    let inv = cli::parse_invocation(&args(&["--machine=box"])).unwrap();
    assert_eq!(inv.machine.as_deref(), Some("box"));
    assert_eq!(inv.command, Command::Tui);
    let inv = cli::parse_invocation(&args(&["ls"])).unwrap();
    assert_eq!(inv.machine, None);
    assert_eq!(
        cli::parse_invocation(&args(&["machines", "--json"]))
            .unwrap()
            .command,
        Command::Machines { json: true }
    );
}

#[test]
fn machine_needs_a_name_and_mcp_stays_local() {
    assert!(cli::parse_invocation(&args(&["--machine"])).is_err());
    assert!(cli::parse_invocation(&args(&["--machine", "--json"])).is_err());
    assert!(cli::parse_invocation(&args(&["--machine="])).is_err());
    let err = cli::parse_invocation(&args(&["--machine", "box", "mcp"])).unwrap_err();
    assert!(err.contains("local"), "{err}");
    assert!(cli::parse_invocation(&args(&["--machine", "box", "doctor"])).is_err());
    assert!(cli::parse_invocation(&args(&["--machine", "box", "machines"])).is_err());
    // Without a machine, mcp parses as before.
    assert_eq!(
        cli::parse_invocation(&args(&["mcp"])).unwrap().command,
        Command::Mcp
    );
}

struct Bridge {
    socket: PathBuf,
}

impl Connector for Bridge {
    fn connect(&self) -> io::Result<Pipe> {
        let conn = poimen_client::transport::Conn::connect(&self.socket)?;
        let (to_server_r, to_server_w) = io::pipe()?;
        let (from_server_r, from_server_w) = io::pipe()?;
        thread::spawn(move || {
            let _ = stdio::bridge(&conn, to_server_r, from_server_w, Duration::from_secs(2));
        });
        Ok(Pipe::new(from_server_r, to_server_w, || {}))
    }
}

#[test]
fn commands_run_over_a_stdio_bridge_like_over_ssh() {
    let dir = std::env::temp_dir().join(format!("poimen-machine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
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
    let work = dir.join("work").canonicalize().unwrap();
    Client::new(&config.socket)
        .create_space("remote-work", &work.display().to_string())
        .unwrap();

    let client = Client::with_connector(
        "ssh:box",
        Arc::new(Bridge {
            socket: config.socket.clone(),
        }),
    );
    assert!(matches!(
        remote::classify(client.ping()),
        remote::State::Connected { .. }
    ));
    let mut out = Vec::new();
    commands::run(
        &client,
        &Request::Ls { json: false },
        commands::PROMPT_WAIT,
        &mut out,
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("remote-work"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_connector_that_ssh_cannot_open_is_offline_with_the_advice_of_the_app() {
    struct Missing;
    impl Connector for Missing {
        fn connect(&self) -> io::Result<Pipe> {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "ssh: bash: poimen-server: command not found",
            ))
        }
    }
    let machine = poimen_core::machines::MachineConfig {
        name: "box".into(),
        ssh: "me@box".into(),
        repos_dir: poimen_core::machines::DEFAULT_REPOS_DIR.into(),
        server_path: None,
    };
    let state = remote::classify(Client::with_connector("ssh:box", Arc::new(Missing)).ping());
    let message = state.failure(&machine).unwrap();
    assert!(message.contains("offline: "), "{message}");
    assert!(
        message.contains("not installed on box or not on its PATH"),
        "{message}"
    );
}
