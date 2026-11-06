//! The server end to end: a real socket, a real shell in a temp dir, the real client.
//!
//! The shell is `sh` on Unix and `cmd.exe` on Windows. Unix-only on purpose: the directory mode
//! and symlink checks (Windows has no modes; the pipe's DACL protects the server there).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::{AgentStatus, ErrorCode, Event, TerminalCreate, TerminalStatus};
use poimen_client::{Client, ClientError};
use poimen_server::{Config, Server, StartError};

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-it-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn config(&self) -> Config {
        Config {
            socket: self.dir.join("run").join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
        }
    }

    fn work(&self) -> String {
        let work = self.dir.join("work");
        // /tmp may be a symlink (macOS); the shell reports the resolved path. Windows: no
        // canonicalize, its `\\?\` form is not a working directory cmd.exe accepts.
        if cfg!(windows) {
            work.display().to_string()
        } else {
            work.canonicalize().unwrap().display().to_string()
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(config: &Config) -> Client {
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    Client::new(&config.socket)
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

fn wait_for_event(rx: &Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("event did not arrive");
        if pred(&event) {
            return event;
        }
    }
}

fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The platform's shell: `sh`, on Windows `cmd.exe`.
fn sh(space_id: &str) -> TerminalCreate {
    let shell = if cfg!(windows) { "cmd.exe" } else { "sh" };
    TerminalCreate {
        space_id: space_id.into(),
        command: Some(vec![shell.into()]),
        ..Default::default()
    }
}

/// Enter in the terminal: a carriage return through ConPTY, a newline for `sh`.
const ENTER: &str = if cfg!(windows) { "\r" } else { "\n" };

#[test]
fn ping_reports_the_protocol_version() {
    let env = Env::new("ping");
    let client = start(&env.config());
    let ping = client.ping().unwrap();
    assert_eq!(ping.protocol, 2);
    assert_eq!(ping.pid, std::process::id());
}

#[test]
fn a_second_server_on_the_same_socket_is_refused() {
    let env = Env::new("single");
    let config = env.config();
    start(&config);
    assert!(matches!(
        Server::bind(config),
        Err(StartError::AlreadyRunning(_))
    ));
}

#[cfg(unix)]
#[test]
fn an_existing_open_socket_directory_is_tightened_to_0700() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new("loosedir");
    let config = env.config();
    let run = config.socket.parent().unwrap();
    std::fs::create_dir_all(run).unwrap();
    std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o777)).unwrap();
    start(&config).ping().unwrap();
    let mode = std::fs::metadata(run).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[cfg(unix)]
#[test]
fn a_symlinked_socket_directory_is_refused() {
    let env = Env::new("linkdir");
    let config = env.config();
    std::fs::create_dir_all(env.dir.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(env.dir.join("elsewhere"), env.dir.join("run")).unwrap();
    let err = Server::bind(config).err().expect("must be refused");
    assert!(matches!(err, StartError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied));
}

#[test]
fn a_regular_file_at_the_socket_path_is_not_removed() {
    let env = Env::new("notsock");
    let config = env.config();
    std::fs::create_dir_all(config.socket.parent().unwrap()).unwrap();
    std::fs::write(&config.socket, "precious").unwrap();
    assert!(matches!(
        Server::bind(config.clone()),
        Err(StartError::Io(_))
    ));
    assert_eq!(std::fs::read_to_string(&config.socket).unwrap(), "precious");
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let env = Env::new("stale");
    let config = env.config();
    std::fs::create_dir_all(config.socket.parent().unwrap()).unwrap();
    // A socket entry nobody listens on, as left by a killed server (Unix: the socket file;
    // Windows: the marker file).
    drop(poimen_client::transport::Listener::bind(&config.socket).unwrap());
    start(&config).ping().unwrap();
}

#[test]
fn spaces_persist_across_restarts_but_terminals_do_not() {
    let env = Env::new("persist");
    let first = env.config();
    let client = start(&first);
    let space = client.create_space("api", &env.work()).unwrap();
    client.create_space("docs", &env.work()).unwrap();
    client.rename_space(&space.id, "api v2").unwrap();
    client.create_terminal(&sh(&space.id)).unwrap();

    // A new server (another socket, same state file), as after a restart.
    let second = Config {
        socket: env.dir.join("run2").join("server.sock"),
        ..first
    };
    let client = start(&second);
    let spaces = client.list_spaces().unwrap();
    let names: Vec<_> = spaces.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["api v2", "docs"]);
    assert_eq!(spaces[0].cwd, env.work());
    assert!(client.list_terminals(None).unwrap().is_empty());
    // New ids do not collide with persisted ones.
    let third = client.create_space("new", &env.work()).unwrap();
    assert!(spaces.iter().all(|s| s.id != third.id));
}

#[cfg(unix)]
#[test]
fn a_real_shell_gets_its_environment_input_and_resize() {
    let env = Env::new("shell");
    let client = start(&env.config());
    let rx = events(&client);
    let space = client.create_space("work", &env.work()).unwrap();
    let terminal = client.create_terminal(&sh(&space.id)).unwrap();
    assert_eq!(terminal.name, "sh");
    assert_eq!(terminal.status, TerminalStatus::Running);
    assert_eq!(terminal.cwd, env.work());
    wait_for_event(&rx, |e| matches!(e, Event::TerminalCreated { .. }));

    let out = env.dir.join("work").join("env.txt");
    client
        .input(
            &terminal.id,
            "echo \"$POIMEN_TERMINAL_ID|$POIMEN_SOCKET|$PWD\" > env.txt\n",
        )
        .unwrap();
    assert_eq!(
        wait_for_file(&out).trim_end(),
        format!(
            "{}|{}|{}",
            terminal.id,
            env.config().socket.display(),
            env.work()
        )
    );

    client.resize(&terminal.id, 100, 30).unwrap();
    client
        .input(&terminal.id, "stty size > size.txt\n")
        .unwrap();
    assert_eq!(
        wait_for_file(&env.dir.join("work").join("size.txt")).trim(),
        "30 100"
    );
    let listed = client.list_terminals(Some(&space.id)).unwrap();
    assert_eq!((listed[0].cols, listed[0].rows), (100, 30));
}

#[cfg(windows)]
#[test]
fn a_real_cmd_gets_its_environment_input_and_resize() {
    let env = Env::new("cmd");
    let client = start(&env.config());
    let space = client.create_space("work", &env.work()).unwrap();
    let terminal = client.create_terminal(&sh(&space.id)).unwrap();
    assert_eq!(terminal.name, "cmd");
    assert_eq!(terminal.status, TerminalStatus::Running);
    assert_eq!(terminal.cwd, env.work());

    let out = env.dir.join("work").join("env.txt");
    client
        .input(
            &terminal.id,
            "echo %POIMEN_TERMINAL_ID%^|%POIMEN_SOCKET%^|%CD%> env.txt\r",
        )
        .unwrap();
    assert_eq!(
        wait_for_file(&out).trim_end(),
        format!(
            "{}|{}|{}",
            terminal.id,
            env.config().socket.display(),
            env.work()
        )
    );

    client.resize(&terminal.id, 100, 30).unwrap();
    let listed = client.list_terminals(Some(&space.id)).unwrap();
    assert_eq!((listed[0].cols, listed[0].rows), (100, 30));
}

#[test]
fn exit_close_and_rename_are_reported() {
    let env = Env::new("exit");
    let client = start(&env.config());
    let rx = events(&client);
    let space = client.create_space("work", &env.work()).unwrap();
    let terminal = client.create_terminal(&sh(&space.id)).unwrap();

    let renamed = client.rename_terminal(&terminal.id, "build").unwrap();
    assert_eq!(renamed.name, "build");
    wait_for_event(&rx, |e| matches!(e, Event::TerminalRenamed { .. }));

    client
        .input(&terminal.id, &format!("exit 3{ENTER}"))
        .unwrap();
    let exited = wait_for_event(&rx, |e| matches!(e, Event::TerminalExited { .. }));
    assert_eq!(
        exited,
        Event::TerminalExited {
            terminal_id: terminal.id.clone(),
            exit_code: Some(3)
        }
    );
    let listed = client.list_terminals(None).unwrap();
    assert_eq!(listed[0].status, TerminalStatus::Exited);
    assert_eq!(listed[0].exit_code, Some(3));
    match client.input(&terminal.id, "x") {
        Err(ClientError::Server { code, .. }) => assert_eq!(code, ErrorCode::Exited),
        other => panic!("{other:?}"),
    }

    client.close_terminal(&terminal.id).unwrap();
    wait_for_event(&rx, |e| matches!(e, Event::TerminalClosed { .. }));
    assert!(client.list_terminals(None).unwrap().is_empty());
}

#[test]
fn closing_a_terminal_kills_its_shell_and_closing_a_space_closes_its_terminals() {
    let env = Env::new("close");
    let client = start(&env.config());
    let rx = events(&client);
    let space = client.create_space("work", &env.work()).unwrap();
    let other = client.create_space("other", &env.work()).unwrap();
    client.create_terminal(&sh(&space.id)).unwrap();
    client.create_terminal(&sh(&space.id)).unwrap();
    let kept = client.create_terminal(&sh(&other.id)).unwrap();

    client.close_space(&space.id).unwrap();
    wait_for_event(&rx, |e| matches!(e, Event::SpaceClosed { .. }));
    let left = client.list_terminals(None).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, kept.id);
    assert_eq!(client.list_spaces().unwrap().len(), 1);
}

fn code<T>(r: Result<T, ClientError>) -> ErrorCode {
    match r {
        Err(ClientError::Server { code, .. }) => code,
        Ok(_) => panic!("expected an error"),
        Err(other) => panic!("{other:?}"),
    }
}

#[test]
fn errors_are_typed() {
    let env = Env::new("errors");
    let client = start(&env.config());
    let space = client.create_space("work", &env.work()).unwrap();

    assert_eq!(code(client.close_space("nope")), ErrorCode::NotFound);
    assert_eq!(code(client.close_terminal("nope")), ErrorCode::NotFound);
    assert_eq!(
        code(client.create_terminal(&sh("nope"))),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(client.create_space("  ", "/x")),
        ErrorCode::InvalidParams
    );
    assert_eq!(code(client.resize("t1", 0, 5)), ErrorCode::InvalidParams);
    assert_eq!(
        code(client.request("nope.nope", serde_json::Value::Null)),
        ErrorCode::UnknownMethod
    );
    assert_eq!(
        code(client.request("space.rename", serde_json::json!({"space_id": 5}))),
        ErrorCode::InvalidParams
    );
    let bad = TerminalCreate {
        command: Some(vec!["/definitely/not/a/program".into()]),
        ..sh(&space.id)
    };
    assert_eq!(code(client.create_terminal(&bad)), ErrorCode::SpawnFailed);
    let empty = TerminalCreate {
        command: Some(vec![]),
        ..sh(&space.id)
    };
    assert_eq!(
        code(client.create_terminal(&empty)),
        ErrorCode::InvalidParams
    );
}

#[test]
fn a_bad_line_gets_an_error_and_the_connection_survives() {
    use std::io::{BufRead, BufReader, Write};
    let env = Env::new("raw");
    let config = env.config();
    start(&config);
    let mut stream = poimen_client::transport::Conn::connect(&config.socket).unwrap();
    stream
        .write_all(b"garbage\n{\"id\":\"9\",\"method\":\"ping\"}\n")
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert!(first.contains("parse_error"), "{first}");
    let mut second = String::new();
    reader.read_line(&mut second).unwrap();
    assert!(
        second.contains(r#""id":"9""#) && second.contains("protocol"),
        "{second}"
    );
}

fn report(id: &str, seq: u64, status: AgentStatus, session: Option<&str>) -> serde_json::Value {
    serde_json::json!({"terminal_id": id, "seq": seq, "status": status, "agent": "claude", "session_id": session})
}

#[test]
fn agent_reports_update_the_terminal_emit_an_event_and_stale_ones_are_dropped() {
    let env = Env::new("agent");
    let client = start(&env.config());
    let space = client.create_space("api", &env.work()).unwrap();
    let term = client.create_terminal(&sh(&space.id)).unwrap();
    let rx = events(&client);
    let send = |seq, status, session| {
        client
            .request(
                "terminal.report_agent",
                report(&term.id, seq, status, session),
            )
            .unwrap()
    };
    let agent = || client.list_terminals(None).unwrap()[0].agent.clone();

    assert_eq!(send(10, AgentStatus::Working, Some("s1"))["accepted"], true);
    let event = wait_for_event(&rx, |e| matches!(e, Event::AgentStatusChanged { .. }));
    assert_eq!(
        event,
        Event::AgentStatusChanged {
            terminal_id: term.id.clone(),
            status: AgentStatus::Working,
            agent: Some("claude".into()),
            session_id: Some("s1".into()),
            message: None,
            message_at: None
        }
    );
    assert_eq!(agent().unwrap().status, AgentStatus::Working);

    // Stale: lower and equal seq are dropped and change nothing.
    for seq in [9, 10] {
        assert_eq!(
            send(seq, AgentStatus::Blocked, Some("s1"))["accepted"],
            false
        );
    }
    assert_eq!(agent().unwrap().status, AgentStatus::Working);

    send(11, AgentStatus::Gone, None);
    let gone = wait_for_event(&rx, |e| matches!(e, Event::AgentStatusChanged { .. }));
    assert!(matches!(
        gone,
        Event::AgentStatusChanged {
            status: AgentStatus::Gone,
            ..
        }
    ));
    assert!(agent().is_none());
}

#[test]
fn the_notification_text_is_kept_until_the_agent_works_again() {
    let env = Env::new("agent-message");
    let client = start(&env.config());
    let space = client.create_space("api", &env.work()).unwrap();
    let term = client.create_terminal(&sh(&space.id)).unwrap();
    let rx = events(&client);
    let send = |seq: u64, status: AgentStatus, message: Option<&str>| {
        let mut r = report(&term.id, seq, status, None);
        r["message"] = serde_json::json!(message);
        client.request("terminal.report_agent", r).unwrap();
    };
    let agent = || {
        client.list_terminals(None).unwrap()[0]
            .agent
            .clone()
            .unwrap()
    };

    send(1, AgentStatus::Blocked, Some("Allow Bash?\n\u{1b}[0m"));
    let event = wait_for_event(&rx, |e| matches!(e, Event::AgentStatusChanged { .. }));
    let Event::AgentStatusChanged {
        message,
        message_at,
        ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(message.as_deref(), Some("Allow Bash? [0m"));
    assert!(message_at.is_some());
    assert_eq!(agent().message.as_deref(), Some("Allow Bash? [0m"));

    // A report without text keeps it; `working` clears it.
    send(2, AgentStatus::Done, None);
    assert_eq!(agent().message.as_deref(), Some("Allow Bash? [0m"));
    send(3, AgentStatus::Working, None);
    let cleared = agent();
    assert_eq!((cleared.message, cleared.message_at), (None, None));
}

#[test]
fn reporting_an_unknown_terminal_is_not_found() {
    let env = Env::new("agent-missing");
    let client = start(&env.config());
    let err = client
        .request(
            "terminal.report_agent",
            report("t99", 1, AgentStatus::Idle, None),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        ClientError::Server {
            code: ErrorCode::NotFound,
            ..
        }
    ));
}
