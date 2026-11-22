//! `poimen doctor` against an in-process poimen-server on a temporary socket, a temporary state
//! directory and a temporary `PATH` of fake programs (never the user's server or home). Unix only.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::thread;

use poimen_client::Client;
use poimen_console::doctor::bundle;
use poimen_console::doctor::collect::{Env, Runner, collect};
use poimen_console::doctor::{self, Inputs, Status};
use poimen_server::{Config, PtyHost, Server};

struct Fixture {
    dir: PathBuf,
    state_file: PathBuf,
    client: Client,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str, serve: bool) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-doctor-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("work-secret")).unwrap();
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
    let client = Client::new(&config.socket);
    let state_file = config.state_file.clone();
    if serve {
        let server = Server::bind(config).unwrap();
        thread::spawn(move || server.serve());
        let cwd = dir.join("work-secret").canonicalize().unwrap();
        client
            .create_space("project-secret", &cwd.display().to_string())
            .unwrap();
    }
    Fixture {
        dir,
        state_file,
        client,
    }
}

/// Fake programs on the `PATH`: `gh`, `git`, and the agent CLIs `claude` and `copilot`.
fn programs(f: &Fixture, names: &[&str]) {
    for n in names {
        std::fs::write(f.dir.join("bin").join(n), "#!/bin/sh\n").unwrap();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut std::fs::metadata(f.dir.join("bin").join(n))
                .unwrap()
                .permissions(),
            0o755,
        );
        let p = f.dir.join("bin").join(n);
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&p, perm).unwrap();
    }
}

/// Answers `integration status` and `--version` like the real programs.
struct Fake {
    hooks: &'static str,
}

impl Runner for Fake {
    fn run(&self, _program: &Path, args: &[&str]) -> Result<String, String> {
        match args {
            ["--version"] => Ok("poimen-pty-host 0.0.0 (host protocol 1)\n".into()),
            ["integration", "status", _] => Ok(format!("{}\nmcp: installed\n", self.hooks)),
            _ => Err("unexpected".into()),
        }
    }
}

fn inputs(f: &Fixture, hooks: &'static str) -> Inputs {
    let bin = f.dir.join("bin");
    let server = poimen_protocol::CHANNEL.server_program(false);
    let host = poimen_protocol::CHANNEL.host_program(false);
    programs(
        f,
        &[&server, &host, "gh", "git", "claude", "copilot", "gemini"],
    );
    let env = Env {
        client: &f.client,
        state_file: f.state_file.clone(),
        bin_dir: Some(bin.clone()),
        path: bin.display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    collect(&env, &Fake { hooks })
}

fn status(r: &doctor::Report, id: &str) -> Status {
    r.checks.iter().find(|c| c.id == id).unwrap().status
}

#[test]
fn everything_ok_exits_zero() {
    let f = fixture("ok", true);
    let r = doctor::report(&inputs(&f, "installed"), true);
    for c in &r.checks {
        assert_eq!(c.status, Status::Ok, "{c:?}");
    }
    assert_eq!(r.exit_code(), 0);
    let state = r.checks.iter().find(|c| c.id == "state").unwrap();
    assert!(state.detail.contains("1 spaces"), "{}", state.detail);
}

#[test]
fn without_a_server_it_still_reports_everything_it_can_read() {
    let f = fixture("noserver", false);
    let r = doctor::report(&inputs(&f, "installed"), true);
    assert_eq!(status(&r, "server"), Status::Warning);
    assert_eq!(status(&r, "pty-host"), Status::Ok);
    assert_eq!(status(&r, "tool-git"), Status::Ok);
    assert_eq!(r.exit_code(), 0);
}

#[test]
fn missing_hooks_are_warnings() {
    let f = fixture("hooks", true);
    let r = doctor::report(&inputs(&f, "not installed"), true);
    assert_eq!(status(&r, "agent-claude"), Status::Warning);
    assert_eq!(r.exit_code(), 0);
}

#[test]
fn a_damaged_state_file_is_an_error_and_a_backup_a_warning() {
    let f = fixture("damaged", false);
    std::fs::create_dir_all(f.state_file.parent().unwrap()).unwrap();
    std::fs::write(&f.state_file, "{ not json").unwrap();
    let r = doctor::report(&inputs(&f, "installed"), true);
    assert_eq!(status(&r, "state"), Status::Error);
    assert_eq!(r.exit_code(), 1);

    // The file is gone and the previous copy is left: a save was interrupted.
    std::fs::remove_file(&f.state_file).unwrap();
    std::fs::write(f.state_file.with_extension("json.bak"), r#"{"spaces":[]}"#).unwrap();
    let r = doctor::report(&inputs(&f, "installed"), true);
    assert_eq!(status(&r, "state"), Status::Warning);
    assert_eq!(r.exit_code(), 0);
}

#[test]
fn the_bundle_holds_no_directory_or_name_and_is_private_and_never_overwritten() {
    let f = fixture("bundle", true);
    // A log with a path in it, as the real ones can have.
    let log = f.state_file.with_file_name("server.log");
    std::fs::write(&log, format!("t server saved {}\n", f.state_file.display())).unwrap();
    let i = inputs(&f, "installed");
    let hosts = f.dir.join("run").join("hosts");
    let value = bundle::build(&i, &f.state_file, &hosts, Some(&f.dir));
    let text = value.to_string();
    for leak in [
        f.dir.to_str().unwrap(),
        "project-secret",
        "work-secret",
        "poimen-doctor-",
    ] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    assert!(
        text.contains("<state>"),
        "log kept, directory hidden: {text}"
    );
    assert_eq!(value["state"]["spaces"].as_array().unwrap().len(), 1);

    let out = f.dir.join("bundle.json");
    bundle::write(&out, &value).unwrap();
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&out).unwrap().permissions());
    assert_eq!(mode & 0o777, 0o600);
    assert!(bundle::write(&out, &value).is_err());
}
