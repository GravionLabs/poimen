//! `terminal.list` carries the resident memory of a terminal's process tree (#971).
#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::TerminalCreate;
use poimen_server::{Config, PtyHost, Server};

#[test]
fn a_running_terminal_lists_its_memory_and_an_exited_one_does_not() {
    let dir = std::env::temp_dir().join(format!("poimen-memory-{}", std::process::id()));
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
    let spawn = |command: &[&str]| {
        client
            .create_terminal(&TerminalCreate {
                space_id: space.id.clone(),
                command: Some(command.iter().map(|s| s.to_string()).collect()),
                ..Default::default()
            })
            .unwrap()
            .id
    };
    let running = spawn(&["sh", "-c", "sleep 30 & wait"]);
    let exited = spawn(&["true"]);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let list = client.list_terminals(None).unwrap();
        let find = |id: &str| list.iter().find(|t| t.id == id).unwrap().clone();
        let (r, e) = (find(&running), find(&exited));
        if e.status == poimen_client::protocol::TerminalStatus::Exited {
            assert!(r.memory_bytes.is_some_and(|b| b > 0), "{r:?}");
            assert_eq!(e.memory_bytes, None);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the second terminal never exited"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
