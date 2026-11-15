//! A terminal of a server without a locale gets a UTF-8 `LC_CTYPE`, so `locale charmap` (what
//! `pbcopy` and other tools read) answers UTF-8. Unix-only: `locale` and the temp dir modes.
//!
//! The in-process server reads the environment of this process, and the crate forbids `unsafe`
//! (`remove_var`), so the test runs itself again as a child with the locale variables removed.

#![cfg(unix)]
#![allow(clippy::disallowed_methods)] // Unix-only test: no console window to hide.

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::TerminalCreate;
use poimen_server::{Config, PtyHost, Server};

const CHILD: &str = "POIMEN_LOCALE_TEST_CHILD";
const TEST: &str = "a_terminal_without_a_server_locale_reports_utf8";

#[test]
fn a_terminal_without_a_server_locale_reports_utf8() {
    if std::env::var_os(CHILD).is_none() {
        // Parent: run this test again with no locale in the environment.
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--test-threads=1"])
            .env(CHILD, "1")
            .env_remove("LC_ALL")
            .env_remove("LC_CTYPE")
            .env_remove("LANG")
            .status()
            .unwrap();
        assert!(status.success(), "the child run failed: {status}");
        return;
    }
    assert!(
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .all(|name| std::env::var_os(name).is_none()),
        "the child must start without a locale"
    );

    let dir = std::env::temp_dir().join(format!("poimen-locale-{}", std::process::id()));
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
        .create_space("locale", &work.display().to_string())
        .unwrap();
    let out = dir.join("charmap.txt");
    let command = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("locale charmap > '{}'", out.display()),
    ];
    client
        .create_terminal(&TerminalCreate {
            space_id: space.id,
            command: Some(command),
            ..Default::default()
        })
        .unwrap();

    let charmap = wait_for_file(&out);
    assert_eq!(charmap.trim(), "UTF-8");
    let _ = std::fs::remove_dir_all(&dir);
}

fn wait_for_file(path: &PathBuf) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text;
        }
        assert!(Instant::now() < deadline, "no output in {}", path.display());
        thread::sleep(Duration::from_millis(50));
    }
}
