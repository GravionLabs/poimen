//! The `poimen` binary and the server start (#1171): `doctor` and a poimen terminal
//! (`POIMEN_SOCKET` set) never start a server. A fake `poimen-server` on `PATH` records a start.
#![cfg(unix)]

use std::path::Path;

fn fake_server(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join(poimen_protocol::CHANNEL.server_program(false));
    let marker = dir.join("started");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\n[ $# -eq 0 ] && echo started > '{}'\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    marker
}

fn poimen(dir: &Path, args: &[&str], socket: Option<&Path>) -> std::process::Output {
    let mut cmd = poimen_core::gui_path::command(env!("CARGO_BIN_EXE_poimen"));
    cmd.args(args)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .env("HOME", dir)
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("USER", "poimen-test");
    if let Some(socket) = socket {
        cmd.env("POIMEN_SOCKET", socket);
    }
    cmd.output().unwrap()
}

fn dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("poimen-startbin-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

#[test]
fn doctor_never_starts_a_server() {
    let dir = dir("doctor");
    let marker = fake_server(&dir);
    poimen(&dir, &["doctor", "--json"], None);
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inside_a_poimen_terminal_nothing_is_started() {
    let dir = dir("terminal");
    let marker = fake_server(&dir);
    let out = poimen(&dir, &["ls"], Some(&dir.join("run").join("gone.sock")));
    assert!(!out.status.success());
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_and_version_start_nothing() {
    let dir = dir("help");
    let marker = fake_server(&dir);
    assert!(poimen(&dir, &["--version"], None).status.success());
    assert!(poimen(&dir, &["--help"], None).status.success());
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
