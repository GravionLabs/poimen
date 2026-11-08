//! Live state of deliver loops: reads `<repo>/.agents/deliver-loop/status.json` (schema
//! `deliver-loop-status.schema.json` of GravionLabs/skills, `schemaVersion: 1`) and watches it with
//! `notify`, no polling. The loop writes the file atomically (temp file, then rename), so a read
//! never sees a partial file. Read-only: poimen never writes it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;

/// The status file relative to a repository root.
pub const STATUS_PATH: &str = ".agents/deliver-loop/status.json";

/// The schema version this build understands; others are reported, not parsed.
pub const SUPPORTED_SCHEMA: u64 = 1;

/// A larger file is not a status file.
const MAX_BYTES: u64 = 1 << 20;

/// What a repository's status file holds right now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Read {
    /// No file: no loop was started here, or its data was removed.
    Missing,
    /// A JSON object of the supported schema version; the frontend validates its fields.
    Ok { status: Value },
    /// A JSON object with another `schemaVersion` (or none).
    Unsupported {
        #[serde(rename = "schemaVersion")]
        schema_version: Option<Value>,
    },
    /// Not readable, not JSON, not an object, not a regular file or too large.
    Invalid { error: String },
}

/// Reads the status file of the repository at `root`.
pub fn read(root: &Path) -> Read {
    let path = root.join(STATUS_PATH);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    if !meta.is_file() {
        return Read::Invalid {
            error: "status.json is not a regular file".into(),
        };
    }
    if meta.len() > MAX_BYTES {
        return Read::Invalid {
            error: "status.json is larger than 1 MiB".into(),
        };
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Removed between the check and the read.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    parse(&text)
}

/// Classifies the text of a status file.
pub fn parse(text: &str) -> Read {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    let Some(object) = value.as_object() else {
        return Read::Invalid {
            error: "status.json is not a JSON object".into(),
        };
    };
    match object.get("schemaVersion") {
        Some(v) if v.as_u64() == Some(SUPPORTED_SCHEMA) => Read::Ok { status: value },
        other => Read::Unsupported {
            schema_version: other.cloned(),
        },
    }
}

enum Msg {
    Changed,
    Stop,
}

/// A running watch of one repository; dropping it stops the thread and the file watcher.
pub struct Handle {
    stop: Sender<Msg>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop.send(Msg::Stop);
    }
}

/// Whether a changed path can concern the status file: anything below a `.agents` directory.
fn relevant(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".agents")
}

/// The deepest existing directory on the way to the status file; the one to watch, since a
/// directory that does not exist cannot be watched and its creation is seen one level up.
fn target(root: &Path) -> PathBuf {
    let loop_dir = root.join(".agents").join("deliver-loop");
    let agents = root.join(".agents");
    if loop_dir.is_dir() {
        loop_dir
    } else if agents.is_dir() {
        agents
    } else {
        root.to_path_buf()
    }
}

/// Watches the status file of the repository at `root`: calls `emit` with the current content at
/// the start and again after every change that alters it (bursts coalesced).
pub fn watch(root: PathBuf, emit: impl Fn(Read) + Send + 'static) -> Handle {
    let (tx, rx) = mpsc::channel();
    let handle = Handle { stop: tx.clone() };
    std::thread::spawn(move || {
        let notifier = tx;
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && event.paths.iter().any(|p| relevant(p))
                {
                    let _ = notifier.send(Msg::Changed);
                }
            })
        else {
            return;
        };
        let mut watched: Option<PathBuf> = None;
        let mut last: Option<Read> = None;
        loop {
            // Arm before reading, so a change between the two is not lost.
            let wanted = target(&root);
            if watched.as_ref() != Some(&wanted) {
                if let Some(old) = watched.take() {
                    let _ = watcher.unwatch(&old);
                }
                if watcher.watch(&wanted, RecursiveMode::NonRecursive).is_ok() {
                    watched = Some(wanted);
                }
            }
            let current = read(&root);
            if last.as_ref() != Some(&current) {
                emit(current.clone());
                last = Some(current);
            }
            match rx.recv() {
                Ok(Msg::Changed) => {
                    // A burst of writes becomes one read.
                    std::thread::sleep(Duration::from_millis(40));
                    let mut stopped = false;
                    while let Ok(msg) = rx.try_recv() {
                        stopped |= matches!(msg, Msg::Stop);
                    }
                    if stopped {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    handle
}

/// What to do to go from the `current` watched roots to the `wanted` ones: `(stop, start)`.
pub fn plan(current: &HashSet<String>, wanted: &[String]) -> (Vec<String>, Vec<String>) {
    let wanted_set: HashSet<&String> = wanted.iter().collect();
    let mut stop: Vec<String> = current
        .iter()
        .filter(|r| !wanted_set.contains(r))
        .cloned()
        .collect();
    stop.sort();
    let mut start = Vec::new();
    for root in wanted {
        if !current.contains(root) && !start.contains(root) {
            start.push(root.clone());
        }
    }
    (stop, start)
}

/// The running watches by repository root.
pub type Watches = HashMap<String, Handle>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "poimen-deliver-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Events carry resolved paths (`/private/var` for `/var` on macOS).
        dir.canonicalize().unwrap()
    }

    fn write_status(root: &Path, text: &str) {
        let dir = root.join(".agents/deliver-loop");
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("status.json.tmp");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, dir.join("status.json")).unwrap();
    }

    const RUNNING: &str = include_str!("../../src/app/deliver/fixtures/status-running.json");

    #[test]
    fn a_missing_file_is_missing() {
        let root = temp_dir("missing");
        assert_eq!(read(&root), Read::Missing);
    }

    #[test]
    fn a_status_of_the_supported_version_is_ok() {
        let root = temp_dir("ok");
        write_status(&root, RUNNING);
        let Read::Ok { status } = read(&root) else {
            panic!("not ok");
        };
        assert_eq!(status["milestone"]["key"], "M4");
    }

    #[test]
    fn another_schema_version_is_unsupported_not_an_error() {
        assert_eq!(
            parse(r#"{"schemaVersion":2,"run":"x"}"#),
            Read::Unsupported {
                schema_version: Some(Value::from(2))
            }
        );
        assert_eq!(
            parse(r#"{"run":"x"}"#),
            Read::Unsupported {
                schema_version: None
            }
        );
    }

    #[test]
    fn broken_content_is_invalid() {
        for text in ["", "{", "[]", "42", "null"] {
            assert!(matches!(parse(text), Read::Invalid { .. }), "{text:?}");
        }
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_invalid() {
        let root = temp_dir("dir");
        std::fs::create_dir_all(root.join(STATUS_PATH)).unwrap();
        assert!(matches!(read(&root), Read::Invalid { .. }));
    }

    #[test]
    fn a_huge_file_is_invalid() {
        let root = temp_dir("huge");
        write_status(&root, &" ".repeat(MAX_BYTES as usize + 1));
        assert!(matches!(read(&root), Read::Invalid { .. }));
    }

    #[test]
    fn serializes_with_the_wire_names() {
        let json = serde_json::to_value(Read::Unsupported {
            schema_version: Some(Value::from(9)),
        })
        .unwrap();
        assert_eq!(json["kind"], "unsupported");
        assert_eq!(json["schemaVersion"], 9);
        assert_eq!(
            serde_json::to_value(Read::Missing).unwrap()["kind"],
            "missing"
        );
    }

    #[test]
    fn plan_stops_gone_roots_and_starts_new_ones_once() {
        let current: HashSet<String> = ["/a", "/b"].iter().map(|s| s.to_string()).collect();
        let wanted: Vec<String> = ["/b", "/c", "/c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            plan(&current, &wanted),
            (vec!["/a".to_string()], vec!["/c".to_string()])
        );
        assert_eq!(plan(&HashSet::new(), &[]), (vec![], vec![]));
    }

    #[test]
    fn the_watch_follows_the_file_from_missing_to_written_to_rewritten() {
        let root = temp_dir("watch");
        let (tx, rx) = mpsc::channel();
        let _handle = watch(root.clone(), move |r| {
            let _ = tx.send(r);
        });
        let next = |want: &dyn Fn(&Read) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(r) if want(&r) => return r,
                    Ok(_) => {}
                    Err(_) => panic!("no matching update"),
                }
            }
        };
        next(&|r| *r == Read::Missing);
        // The directories do not exist yet: their creation is seen one level up.
        write_status(&root, RUNNING);
        next(&|r| matches!(r, Read::Ok { status } if status["result"] == "running"));
        write_status(&root, &RUNNING.replace("\"running\"", "\"done\""));
        next(&|r| matches!(r, Read::Ok { status } if status["result"] == "done"));
        std::fs::remove_file(root.join(STATUS_PATH)).unwrap();
        next(&|r| *r == Read::Missing);
    }
}
