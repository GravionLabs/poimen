//! The history log of a terminal (#973): output of a real PTY (the host in-process) lands in
//! `<history dir>/<terminal id>`. Everything in a temp dir, never the user's session.
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{Event, TerminalCreate};
use poimen_server::history::{Record, read};
use poimen_server::{Config, PtyHost, Server};

#[test]
fn the_output_of_a_terminal_lands_in_its_history_log() {
    let dir = std::env::temp_dir().join(format!("poimen-history-it-{}", std::process::id()));
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
    let client = Client::new(&config.socket);
    let work = dir.join("work").canonicalize().unwrap();
    let space = client
        .create_space("api", &work.display().to_string())
        .unwrap();
    let events = client.subscribe().unwrap();
    let id = client
        .create_terminal(&TerminalCreate {
            space_id: space.id.clone(),
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                "printf history-marker".into(),
            ]),
            ..Default::default()
        })
        .unwrap()
        .id;
    for event in events {
        if let Ok(Event::TerminalExited { terminal_id, .. }) = event
            && terminal_id == id
        {
            break;
        }
    }
    // The writer runs off the PTY path: wait on the condition, not on the clock.
    let path = config.history_dir.join(&id);
    let deadline = Instant::now() + Duration::from_secs(20);
    let text = loop {
        let text: Vec<u8> = read(&path)
            .into_iter()
            .filter_map(|r| match r {
                Record::Output(b) => Some(b),
                _ => None,
            })
            .flatten()
            .collect();
        let text = String::from_utf8_lossy(&text).into_owned();
        if text.contains("history-marker") || Instant::now() > deadline {
            break text;
        }
        thread::yield_now();
    };
    assert!(text.contains("history-marker"), "{text:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn setup(name: &str, home_config: Option<&str>) -> (Config, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("poimen-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let home = dir.join("home");
    if let Some(text) = home_config {
        let path = poimen_core::settings::config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: Some(home),
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    (config, dir)
}

/// Runs `printf marker` in a new terminal and waits until it exited.
fn run_marker(client: &Client, dir: &std::path::Path) -> String {
    let work = dir.join("work").canonicalize().unwrap();
    let space = client
        .create_space("api", &work.display().to_string())
        .unwrap();
    let events = client.subscribe().unwrap();
    let id = client
        .create_terminal(&TerminalCreate {
            space_id: space.id.clone(),
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                "printf history-marker".into(),
            ]),
            ..Default::default()
        })
        .unwrap()
        .id;
    for event in events {
        if let Ok(Event::TerminalExited { terminal_id, .. }) = event
            && terminal_id == id
        {
            break;
        }
    }
    id
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        thread::yield_now();
    }
}

#[test]
fn closing_a_terminal_removes_its_log() {
    let (config, dir) = setup("close", None);
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let id = run_marker(&client, &dir);
    let path = config.history_dir.join(&id);
    wait_for("the log exists", || path.exists());
    client.close_terminal(&id).unwrap();
    wait_for("the log is removed", || !path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_the_setting_off_nothing_is_written() {
    let (config, dir) = setup("off", Some(r#"{"terminalHistory":{"enabled":false}}"#));
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let id = run_marker(&client, &dir);
    // Let the writer see everything queued: a second terminal's exit comes after the first's.
    let _ = run_marker(&client, &dir);
    assert!(!config.history_dir.join(&id).exists());
    assert!(
        !config.history_dir.exists()
            || std::fs::read_dir(&config.history_dir)
                .unwrap()
                .next()
                .is_none()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_log_without_a_terminal_is_removed_at_start() {
    let (config, dir) = setup("orphan", None);
    std::fs::create_dir_all(&config.history_dir).unwrap();
    let orphan = config.history_dir.join("t999");
    std::fs::write(&orphan, b"PMNHIST1").unwrap();
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    assert!(!orphan.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
