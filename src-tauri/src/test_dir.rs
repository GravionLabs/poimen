//! A test directory that removes itself when the guard drops, also when the test panics. A test that makes
//! a directory under `temp_dir()` by hand leaves it behind on every run; `pnpm ratchets` (`test-temp-dirs`)
//! rejects that. Keep the guard in a variable for as long as the test uses the path.
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The path is the canonical one (`/private/var` for `/var` on macOS), as events carry resolved paths.
pub(crate) struct TestDir {
    _guard: tempfile::TempDir,
    path: PathBuf,
}

impl TestDir {
    /// A new directory named `<prefix><random>` in the system temp directory.
    pub(crate) fn new(prefix: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("a test directory");
        // Only the owner may reach it: the client refuses a socket in a directory others can reach.
        #[cfg(unix)]
        std::fs::set_permissions(
            dir.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("a private test directory");
        let path = dir.path().canonicalize().expect("a test directory path");
        Self { _guard: dir, path }
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}
