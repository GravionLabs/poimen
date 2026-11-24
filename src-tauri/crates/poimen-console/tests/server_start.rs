//! Starting `poimen-server` for the console (#1171) against a temp socket, with a fake server
//! program (a shell script) and an in-process server (never the user's). Unix only.
#![cfg(unix)]

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_console::server_start::{self, Error, ensure_with, find_server, spawn_detached};
use poimen_server::{Config, PtyHost, Server};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("poimen-start-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(dir: &Path) -> Config {
    Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    }
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn wait_for(path: &Path) {
    let end = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < end, "{} never appeared", path.display());
        thread::yield_now();
    }
}

#[test]
fn a_running_server_is_used_and_nothing_is_started() {
    let dir = Dir::new("running");
    let config = config(&dir.0);
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let spawned = Cell::new(false);
    ensure_with(
        &client,
        || panic!("the program is not looked for"),
        |_| {
            spawned.set(true);
            Ok(())
        },
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(!spawned.get());
}

#[test]
fn an_absent_server_is_started_and_waited_for() {
    let dir = Dir::new("absent");
    let config = config(&dir.0);
    let fake = dir.0.join("poimen-server");
    let marker = dir.0.join("started");
    script(&fake, &format!("echo started > '{}'", marker.display()));
    let client = Client::new(&config.socket);
    // The fake program records its start; the in-process server then answers in its place.
    let serve = {
        let (config, marker) = (config.clone(), marker.clone());
        thread::spawn(move || {
            wait_for(&marker);
            Server::bind(config).unwrap().serve();
        })
    };
    ensure_with(
        &client,
        || Some(fake.clone()),
        spawn_detached,
        Duration::from_secs(10),
    )
    .unwrap();
    assert!(marker.exists());
    assert!(client.ping().is_ok());
    drop(serve);
}

#[test]
fn a_started_server_has_its_own_process_group() {
    let dir = Dir::new("group");
    let fake = dir.0.join("poimen-server");
    let out = dir.0.join("pgid");
    script(
        &fake,
        &format!(
            "ps -o pgid= -p $$ > '{}.tmp'; mv '{0}.tmp' '{0}'",
            out.display()
        ),
    );
    spawn_detached(&fake).unwrap();
    wait_for(&out);
    let pgid: u32 = std::fs::read_to_string(&out)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let ours: u32 = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| s.rsplit(") ").next().map(str::to_owned))
        .and_then(|s| s.split(' ').nth(2).map(str::to_owned))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert_ne!(pgid, ours, "the server shares the console's process group");
}

#[test]
fn a_missing_program_names_what_to_install() {
    let dir = Dir::new("missing");
    let client = Client::new(dir.0.join("none.sock"));
    let e = ensure_with(&client, || None, |_| Ok(()), Duration::from_secs(1)).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)));
    let text = e.to_string();
    assert!(text.contains(&server_start::server_name()), "{text}");
    assert!(text.contains("install"), "{text}");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let dir = Dir::new("timeout");
    let client = Client::new(dir.0.join("none.sock"));
    let e = ensure_with(
        &client,
        || Some(PathBuf::from("/bin/true")),
        |_| Ok(()),
        Duration::from_millis(250),
    )
    .unwrap_err();
    assert!(matches!(e, Error::Timeout(_)), "{e}");
}

#[test]
fn the_program_next_to_the_console_wins_over_the_path() {
    let dir = Dir::new("find");
    let (beside, on_path) = (dir.0.join("beside"), dir.0.join("bin"));
    std::fs::create_dir_all(&beside).unwrap();
    std::fs::create_dir_all(&on_path).unwrap();
    let name = server_start::server_name();
    assert_eq!(
        find_server(Some(&beside), &on_path.display().to_string()),
        None
    );
    script(&on_path.join(&name), "");
    let path = on_path.display().to_string();
    assert_eq!(find_server(Some(&beside), &path), Some(on_path.join(&name)));
    script(&beside.join(&name), "");
    assert_eq!(find_server(Some(&beside), &path), Some(beside.join(&name)));
}
