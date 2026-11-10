//! Client kinds on `events.subscribe`: a real server on a temp socket, the real client.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::{ClientInfo, ClientKind, Event};
use poimen_client::{Client, EventStream};
use poimen_server::{Config, Server};

fn start(name: &str) -> (Client, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("poimen-clients-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        scrollback: 1000,
        home: None,
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), dir)
}

fn listen(stream: EventStream) -> Receiver<Event> {
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

fn next_clients(rx: &Receiver<Event>) -> Vec<ClientKind> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("clients_changed in time");
        if let Event::ServerClientsChanged { clients } = event {
            return clients;
        }
    }
}

fn info(kind: ClientKind) -> Option<ClientInfo> {
    Some(ClientInfo {
        kind,
        pid: std::process::id(),
    })
}

#[test]
fn ping_lists_connected_kinds_and_events_follow_connect_and_disconnect() {
    let (client, dir) = start("kinds");
    assert!(client.ping().unwrap().clients.is_empty());

    let watcher = listen(client.subscribe_as(info(ClientKind::Tui)).unwrap());
    assert_eq!(client.ping().unwrap().clients, vec![ClientKind::Tui]);

    let app = client.subscribe_as(info(ClientKind::App)).unwrap();
    assert_eq!(
        next_clients(&watcher),
        vec![ClientKind::App, ClientKind::Tui]
    );
    assert_eq!(
        client.ping().unwrap().clients,
        vec![ClientKind::App, ClientKind::Tui]
    );

    // A subscription without `client` counts as unknown.
    let old = client.subscribe().unwrap();
    assert_eq!(
        next_clients(&watcher),
        vec![ClientKind::App, ClientKind::Tui, ClientKind::Unknown]
    );

    drop(app);
    assert_eq!(
        next_clients(&watcher),
        vec![ClientKind::Tui, ClientKind::Unknown]
    );
    drop(old);
    assert_eq!(next_clients(&watcher), vec![ClientKind::Tui]);
    assert_eq!(client.ping().unwrap().clients, vec![ClientKind::Tui]);
    let _ = std::fs::remove_dir_all(dir);
}
