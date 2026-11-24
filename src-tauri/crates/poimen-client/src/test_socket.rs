//! A socket path in a fresh private directory, for the tests of the client. Keep the guard: it removes the
//! directory when it drops, also when the test panics (`docs/agents/tests.md`).
use std::path::PathBuf;

pub(crate) fn temp_socket(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("poimen-client-test-{name}-"))
        .tempdir()
        .unwrap();
    // A server refuses a socket in a directory others can reach.
    #[cfg(unix)]
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let socket = dir.path().join("s.sock");
    (dir, socket)
}
