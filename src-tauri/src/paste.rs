//! Pasted images (#906): the bytes of an image on the clipboard go to a file in the app cache dir
//! and the terminal view types its quoted path. Only png, jpeg, gif and webp, at most 20 MiB;
//! files older than 24 h are removed on each call.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The largest image accepted.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;
/// Age after which a pasted file is removed.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The file extension of an accepted image type.
pub fn extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Removes regular files in `dir` last modified more than [`MAX_AGE`] before `now`.
pub fn cleanup(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > MAX_AGE);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Writes `bytes` to `<dir>/<unix ms>.<ext>` (`create_new`, a taken name moves to the next
/// millisecond) and returns the path. Old files are removed first.
pub fn write(dir: &Path, bytes: &[u8], mime: &str, now: SystemTime) -> Result<PathBuf, String> {
    let ext = extension(mime).ok_or_else(|| format!("the image type {mime:?} is not supported"))?;
    if bytes.is_empty() {
        return Err("the image is empty".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err("the image is larger than 20 MiB".into());
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    cleanup(dir, now);
    let mut ms = now
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    loop {
        let path = dir.join(format!("{ms}.{ext}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ms += 1,
            Err(e) => return Err(format!("cannot write {}: {e}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-paste-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn at(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn writes_the_bytes_under_the_millisecond() {
        let dir = temp("write");
        let path = write(&dir, b"png", "image/png", at(1_700_000_000_123)).unwrap();
        assert_eq!(path, dir.join("1700000000123.png"));
        assert_eq!(fs::read(&path).unwrap(), b"png");
        // The same millisecond again never overwrites.
        let second = write(&dir, b"two", "image/png", at(1_700_000_000_123)).unwrap();
        assert_eq!(second, dir.join("1700000000124.png"));
        assert_eq!(fs::read(&path).unwrap(), b"png");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn maps_each_type_to_its_extension() {
        assert_eq!(extension("image/png"), Some("png"));
        assert_eq!(extension("image/jpeg"), Some("jpg"));
        assert_eq!(extension("image/gif"), Some("gif"));
        assert_eq!(extension("image/webp"), Some("webp"));
        assert_eq!(extension("image/svg+xml"), None);
        assert_eq!(extension("text/plain"), None);
    }

    #[test]
    fn refuses_other_types_and_large_images() {
        let dir = temp("refuse");
        assert!(write(&dir, b"x", "image/svg+xml", at(1)).is_err());
        assert!(write(&dir, &vec![0; MAX_BYTES + 1], "image/png", at(1)).is_err());
        assert!(write(&dir, b"", "image/png", at(1)).is_err());
        assert!(write(&dir, &vec![0; MAX_BYTES], "image/png", at(1)).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_files_older_than_a_day_on_each_call() {
        let dir = temp("cleanup");
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.png");
        fs::write(&old, b"o").unwrap();
        // "now" is two days after the file was written.
        let now = SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60);
        let fresh = write(&dir, b"n", "image/png", now).unwrap();
        assert!(!old.exists());
        assert!(fresh.exists());
        // A file younger than a day stays.
        let young = dir.join("young.png");
        fs::write(&young, b"y").unwrap();
        write(&dir, b"n", "image/png", SystemTime::now()).unwrap();
        assert!(young.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
