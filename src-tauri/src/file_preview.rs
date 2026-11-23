//! The read-only file preview tab (#1004): one file as text, or only a marker, and a watch that
//! says when the file changed. Nothing here writes.

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// Largest file shown; above it only the size is sent.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// How many bytes are searched for a NUL byte to tell a binary file.
const SNIFF_BYTES: usize = 8192;

/// What the preview tab may show of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Preview {
    Text {
        text: String,
        size: u64,
    },
    /// A NUL byte near the start, or content that is not UTF-8.
    Binary {
        size: u64,
    },
    /// Over [`MAX_BYTES`]; the content is not read.
    TooLarge {
        size: u64,
    },
}

/// Reads `path` (an absolute path of a regular file) for the preview.
pub fn read(path: &str) -> Result<Preview, String> {
    let path = Path::new(path);
    if !path.is_absolute() || path.to_string_lossy().contains('\0') {
        return Err("The path is not absolute".into());
    }
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "The file does not exist".to_owned(),
        _ => e.to_string(),
    })?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    let size = meta.len();
    if size > MAX_BYTES {
        return Ok(Preview::TooLarge { size });
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    // The file grew between the check and the read.
    if bytes.len() as u64 > MAX_BYTES {
        return Ok(Preview::TooLarge {
            size: bytes.len() as u64,
        });
    }
    if bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) {
        return Ok(Preview::Binary { size });
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Preview::Text { text, size }),
        Err(_) => Ok(Preview::Binary { size }),
    }
}

/// The preview of one file for the webview. Read only.
#[tauri::command]
pub async fn preview_read(path: String) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || read(&path))
        .await
        .map_err(|e| e.to_string())?
}

enum Msg {
    Changed,
    Stop,
}

/// A running watch of one file; dropping it stops the thread and the file watcher.
pub struct Handle {
    stop: Sender<Msg>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop.send(Msg::Stop);
    }
}

/// Calls `changed` after every change of the file `path` (bursts coalesced). The directory is
/// watched, not the file, so a replace through rename (most editors, agents) is seen too.
pub fn watch(path: PathBuf, changed: impl Fn() + Send + 'static) -> Option<Handle> {
    let dir = path.parent()?.to_path_buf();
    let (tx, rx) = mpsc::channel();
    let notifier = tx.clone();
    let target = path.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event.paths.iter().any(|p| p == &target)
        {
            let _ = notifier.send(Msg::Changed);
        }
    })
    .ok()?;
    watcher.watch(&dir, RecursiveMode::NonRecursive).ok()?;
    std::thread::spawn(move || {
        // The watcher lives as long as this thread.
        let _watcher = watcher;
        while let Ok(Msg::Changed) = rx.recv() {
            // A burst of writes becomes one call: wait until the channel is quiet for a moment.
            let mut stopped = false;
            loop {
                match rx.recv_timeout(Duration::from_millis(60)) {
                    Ok(Msg::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        stopped = true;
                        break;
                    }
                    Ok(Msg::Changed) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            if stopped {
                break;
            }
            changed();
        }
    });
    Some(Handle { stop: tx })
}

/// The running watches by path.
#[derive(Default)]
pub struct Watches(Mutex<HashMap<String, Handle>>);

/// Starts the watch of `path`: a `file-preview` event `{ path }` follows each change. Watching the
/// same path again replaces the watch. Read only.
#[tauri::command]
pub fn preview_watch(app: AppHandle, watches: State<'_, Watches>, path: String) {
    let name = path.clone();
    let handle = watch(PathBuf::from(&path), move || {
        let _ = app.emit("file-preview", serde_json::json!({ "path": name }));
    });
    if let Ok(mut map) = watches.0.lock() {
        match handle {
            Some(h) => map.insert(path, h),
            None => map.remove(&path),
        };
    }
}

/// Stops the watch of `path`.
#[tauri::command]
pub fn preview_unwatch(watches: State<'_, Watches>, path: String) {
    if let Ok(mut map) = watches.0.lock() {
        map.remove(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-preview-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn path_of(file: &Path) -> String {
        file.to_string_lossy().into_owned()
    }

    #[test]
    fn text_is_returned_with_its_size() {
        let dir = temp("text");
        let file = dir.join("a.md");
        std::fs::write(&file, "# Plan\n").unwrap();
        assert_eq!(
            read(&path_of(&file)),
            Ok(Preview::Text {
                text: "# Plan\n".into(),
                size: 7
            })
        );
    }

    #[test]
    fn a_nul_byte_or_invalid_utf8_is_binary() {
        let dir = temp("binary");
        let nul = dir.join("a.bin");
        std::fs::write(&nul, b"ab\0cd").unwrap();
        assert_eq!(read(&path_of(&nul)), Ok(Preview::Binary { size: 5 }));
        let bad = dir.join("b.txt");
        std::fs::write(&bad, [0xff, 0xfe, 0x41]).unwrap();
        assert_eq!(read(&path_of(&bad)), Ok(Preview::Binary { size: 3 }));
    }

    #[test]
    fn a_file_over_one_mebibyte_is_not_read() {
        let dir = temp("large");
        let file = dir.join("big.txt");
        std::fs::write(&file, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(
            read(&path_of(&file)),
            Ok(Preview::TooLarge {
                size: MAX_BYTES + 1
            })
        );
        let exact = dir.join("exact.txt");
        std::fs::write(&exact, vec![b'a'; MAX_BYTES as usize]).unwrap();
        assert!(matches!(read(&path_of(&exact)), Ok(Preview::Text { .. })));
    }

    #[test]
    fn a_missing_file_a_directory_and_a_relative_path_are_errors() {
        let dir = temp("errors");
        assert_eq!(
            read(&path_of(&dir.join("none.txt"))),
            Err("The file does not exist".into())
        );
        assert_eq!(read(&path_of(&dir)), Err("Not a file".into()));
        assert_eq!(read("a.txt"), Err("The path is not absolute".into()));
    }

    #[test]
    fn the_wire_shape_is_tagged_by_state() {
        let json = serde_json::to_value(Preview::TooLarge { size: 3 }).unwrap();
        assert_eq!(json, serde_json::json!({ "state": "too_large", "size": 3 }));
        let json = serde_json::to_value(Preview::Text {
            text: "x".into(),
            size: 1,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "state": "text", "text": "x", "size": 1 })
        );
    }

    #[test]
    fn a_change_of_the_file_is_reported() {
        let dir = temp("watch");
        let file = dir.join("plan.md");
        std::fs::write(&file, "one").unwrap();
        let (tx, rx) = mpsc::channel();
        let _handle = watch(file.clone(), move || {
            let _ = tx.send(());
        })
        .expect("the directory can be watched");
        std::fs::write(&file, "two").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok());
    }
}
