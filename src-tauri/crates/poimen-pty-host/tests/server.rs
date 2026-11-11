//! `poimen-server` with this crate's binary as its PTY host (`PtyHost::Binary`), end to end: the
//! server starts one host process per terminal, reads it as a client and kills it on close.
//! Scripted `sh -c` children only (never a user session), so Unix only.

#![cfg(unix)]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AttachMode, Event, TerminalCreate, decode_bytes};
use poimen_server::{Config, PtyHost, Server};

const WAIT: Duration = Duration::from_secs(20);

struct Env(PathBuf);

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn server(name: &str) -> (Env, Client, String) {
    let dir = std::env::temp_dir().join(format!("ph-s-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config {
        socket: dir.join("run").join("s.sock"),
        state_file: dir.join("server.json"),
        scrollback: 100,
        home: None,
        pty_host: PtyHost::Binary(PathBuf::from(env!("CARGO_BIN_EXE_poimen-pty-host"))),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let space = client
        .create_space("work", &dir.display().to_string())
        .unwrap()
        .id;
    (Env(dir), client, space)
}

fn sh(space: &str, script: &str) -> TerminalCreate {
    TerminalCreate {
        space_id: space.to_string(),
        command: Some(vec!["sh".into(), "-c".into(), script.into()]),
        ..Default::default()
    }
}

#[test]
fn a_terminal_runs_in_a_host_process_and_ends_with_its_child() {
    let (env, client, space) = server("exit");
    let id = client
        .create_terminal(&sh(
            &space,
            "printf \"id=$POIMEN_TERMINAL_ID\"; sleep 0.3; exit 4",
        ))
        .unwrap()
        .id;
    // The host's endpoint is next to the server's socket.
    assert!(env.0.join("run").join("hosts").join(&id).exists());
    let mut a = client.attach(&id, AttachMode::Observe, false).unwrap();
    let mut out = String::from_utf8_lossy(&decode_bytes(&a.info.snapshot).unwrap()).into_owned();
    loop {
        match a.next_record().unwrap() {
            Event::TerminalOutput { data, .. } => {
                out.push_str(&String::from_utf8_lossy(&decode_bytes(&data).unwrap()));
            }
            Event::TerminalExited { exit_code, .. } => {
                assert_eq!(exit_code, Some(4));
                break;
            }
            _ => {}
        }
    }
    assert!(out.contains(&format!("id={id}")), "{out:?}");
}

#[test]
fn closing_a_terminal_kills_its_host() {
    let (env, client, space) = server("close");
    let id = client.create_terminal(&sh(&space, "sleep 60")).unwrap().id;
    let endpoint = env.0.join("run").join("hosts").join(&id);
    assert!(endpoint.exists());
    client.close_terminal(&id).unwrap();
    // The host ends after the kill and removes its endpoint.
    let deadline = Instant::now() + WAIT;
    while endpoint.exists() {
        assert!(Instant::now() < deadline, "the host did not end");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn closing_a_terminal_whose_child_reads_no_input_still_kills_its_host() {
    let (env, client, space) = server("stuck");
    let id = client
        .create_terminal(&sh(&space, "stty raw -echo; sleep 60"))
        .unwrap()
        .id;
    let endpoint = env.0.join("run").join("hosts").join(&id);
    // Far more than the PTY and the host connection hold: the host blocks on the PTY and stops
    // reading its connection.
    let chunk = "x".repeat(32 * 1024);
    for _ in 0..16 {
        client.input(&id, &chunk).unwrap();
    }
    let started = Instant::now();
    client.resize(&id, 100, 30).unwrap();
    client.close_terminal(&id).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "close waited {:?} behind the input",
        started.elapsed()
    );
    let deadline = Instant::now() + WAIT;
    while endpoint.exists() {
        assert!(Instant::now() < deadline, "the host did not end");
        thread::sleep(Duration::from_millis(50));
    }
}
