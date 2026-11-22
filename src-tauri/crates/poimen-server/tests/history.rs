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
