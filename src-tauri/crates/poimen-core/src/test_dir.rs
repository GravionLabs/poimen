//! A test directory that removes itself when the guard drops, also when the test panics. A test that makes
//! a directory under `temp_dir()` by hand leaves it behind on every run; `pnpm ratchets` (`test-temp-dirs`)
//! rejects that. Keep the guard in a variable for as long as the test uses the path.
use std::ops::Deref;
use std::path::Path;

pub(crate) struct TestDir(tempfile::TempDir);

impl TestDir {
    /// A new directory named `<prefix><random>` in the system temp directory.
    pub(crate) fn new(prefix: &str) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(prefix)
                .tempdir()
                .expect("a test directory"),
        )
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}
