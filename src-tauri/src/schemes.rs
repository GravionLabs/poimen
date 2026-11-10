//! User color schemes: the parser lives in `poimen_core::schemes`; this adds the polling watch
//! that emits `color-schemes` to the frontend.

use std::path::Path;
use std::time::{Duration, SystemTime};

pub use poimen_core::schemes::{Schemes, load, load_dir, themes_dir};

/// Names and modification times of the files in the directory.
fn stamp(dir: &Path) -> Vec<(String, Option<SystemTime>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.metadata().and_then(|m| m.modified()).ok(),
            )
        })
        .collect();
    v.sort();
    v
}

/// Runs forever on the calling thread: polls the themes directory and calls `emit` on every change.
pub fn watch(home: &Path, interval: Duration, emit: impl Fn(&Schemes)) {
    let dir = themes_dir(home);
    let mut last = stamp(&dir);
    loop {
        std::thread::sleep(interval);
        let now = stamp(&dir);
        if now != last {
            last = now;
            emit(&load_dir(&dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-schemes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stamp_changes_when_a_file_appears() {
        let dir = temp_dir("stamp");
        let before = stamp(&dir);
        std::fs::write(dir.join("x.toml"), "[colors]").unwrap();
        assert_ne!(before, stamp(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
