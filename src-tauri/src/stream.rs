//! Live terminal streams through `herdr terminal session observe|control`.
//!
//! The CLI speaks a stable newline-delimited JSON contract on stdin/stdout,
//! so poimen does not implement herdr's internal binary client protocol.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;
use serde_json::{Value, json};

use crate::backend::{CloseCode, StreamRecord};

pub use crate::backend::Mode;

/// One stdout record of a terminal session stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Record {
    /// `bytes` stays base64 (as herdr sends it); the frontend decodes it for xterm.js.
    Frame {
        seq: u64,
        full: bool,
        width: u16,
        height: u16,
        bytes: String,
    },
    Closed {
        reason: String,
        exit_code: Option<i32>,
    },
}

pub fn parse_record(line: &str) -> Option<Record> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str()? {
        "terminal.frame" => Some(Record::Frame {
            seq: value["seq"].as_u64().unwrap_or(0),
            full: value["full"].as_bool().unwrap_or(false),
            width: value["width"].as_u64().unwrap_or(0) as u16,
            height: value["height"].as_u64().unwrap_or(0) as u16,
            bytes: value["bytes"].as_str()?.to_owned(),
        }),
        "terminal.closed" => Some(Record::Closed {
            reason: value["reason"].as_str().unwrap_or("closed").to_owned(),
            exit_code: value["exit_code"]
                .as_i64()
                .and_then(|code| i32::try_from(code).ok()),
        }),
        _ => None,
    }
}

pub fn stream_args(mode: Mode, target: &str, cols: u16, rows: u16, takeover: bool) -> Vec<String> {
    let verb = match mode {
        Mode::Observe => "observe",
        Mode::Control => "control",
    };
    let mut args: Vec<String> = ["terminal", "session", verb, target]
        .map(String::from)
        .into();
    if mode == Mode::Control && takeover {
        args.push("--takeover".into());
    }
    args.extend([
        "--cols".into(),
        cols.to_string(),
        "--rows".into(),
        rows.to_string(),
    ]);
    args
}

fn command_line(value: Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

pub fn input_command(text: &str) -> String {
    command_line(json!({ "type": "terminal.input", "text": text }))
}

pub fn resize_command(cols: u16, rows: u16) -> String {
    command_line(json!({ "type": "terminal.resize", "cols": cols, "rows": rows }))
}

pub fn scroll_command(up: bool, lines: u16) -> String {
    let direction = if up { "up" } else { "down" };
    command_line(json!({ "type": "terminal.scroll", "direction": direction, "lines": lines }))
}

/// Rejects what herdr's `terminal.mouse` does not define, before it reaches the CLI's stdin.
pub fn validate_mouse(action: &str, button: &str) -> Result<(), String> {
    if !matches!(action, "down" | "up" | "drag" | "move")
        || !matches!(button, "left" | "right" | "middle")
    {
        return Err("unknown mouse action or button".into());
    }
    Ok(())
}

/// One mouse event at a zero-based cell of the controller's viewport. herdr encodes it for the
/// application's mouse mode and drops it when the application has not enabled mouse reporting.
pub fn mouse_command(action: &str, button: &str, column: u16, row: u16, modifiers: u8) -> String {
    command_line(json!({
        "type": "terminal.mouse",
        "action": action,
        "button": button,
        "column": column,
        "row": row,
        "modifiers": modifiers,
    }))
}

pub fn release_command() -> String {
    command_line(json!({ "type": "terminal.release" }))
}

/// Finds the herdr binary: `HERDR_BIN_PATH`, else `herdr` on `path` (the GUI app's login-shell
/// `PATH`, `gui_path::child_path`, which includes Homebrew, `~/.local/bin` and `~/.cargo/bin`).
/// The auto start (`backend.rs`) uses the same function, so both pick one binary.
pub fn resolve_bin(
    override_path: Option<String>,
    path: &str,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let pathext = std::env::var("PATHEXT").ok();
    resolve_bin_named(
        override_path,
        path,
        &bin_names(cfg!(windows), pathext.as_deref()),
        exists,
    )
}

/// File names to try for `herdr` (`gui_path::executable_names`): on Windows the `PATHEXT` names
/// (`herdr.exe`) before the bare `herdr`; elsewhere just `herdr`.
pub fn bin_names(windows: bool, pathext: Option<&str>) -> Vec<String> {
    crate::gui_path::executable_names("herdr", windows, pathext)
}

pub fn resolve_bin_named(
    override_path: Option<String>,
    path: &str,
    names: &[String],
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    if let Some(p) = override_path.filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|c| exists(c))
        .unwrap_or_else(|| PathBuf::from("herdr"))
}

/// `resolve_bin` against the real environment.
pub fn resolve_bin_from_env() -> PathBuf {
    resolve_bin(
        std::env::var("HERDR_BIN_PATH").ok(),
        &crate::gui_path::child_path(),
        |p| p.is_file(),
    )
}

/// Whether a close reason says another client holds the controller. herdr's wording for a held
/// terminal is "... already has an attached client; retry with --takeover" (found in the herdr
/// binary; the control path itself is not exercised). Anything else (pane gone, crash, spawn
/// failure) is a plain error. This text match lives here, next to the wire format, and nowhere
/// else: everything above it sees `CloseCode::ControllerHeld`.
pub fn is_controller_held(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    [
        "already has an attached client",
        "retry with --takeover",
        "controlled by another client",
        "controller taken over",
    ]
    .iter()
    .any(|text| reason.contains(text))
}

/// Maps the frames of one herdr stream onto the backend-neutral records: a full frame is a
/// `Reset`, other frames are `Output` (after a `Resized` when the size changed), the first frame
/// of a control stream also says the controller is attached, and `Closed` carries the structured
/// code. herdr streams finished screens, so these streams are not `emulated`.
pub struct FrameMapper {
    mode: Mode,
    size: Option<(u16, u16)>,
    attached: bool,
}

impl FrameMapper {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            size: None,
            attached: false,
        }
    }

    pub fn map(&mut self, record: Record) -> Vec<StreamRecord> {
        match record {
            Record::Frame {
                full,
                width,
                height,
                bytes,
                ..
            } => {
                let mut out = Vec::new();
                if full {
                    out.push(StreamRecord::Reset {
                        cols: width,
                        rows: height,
                        bytes,
                    });
                } else {
                    if self.size.is_some_and(|size| size != (width, height)) {
                        out.push(StreamRecord::Resized {
                            cols: width,
                            rows: height,
                        });
                    }
                    out.push(StreamRecord::Output { bytes });
                }
                self.size = Some((width, height));
                if self.mode == Mode::Control && !self.attached {
                    self.attached = true;
                    out.push(StreamRecord::Controller { attached: true });
                }
                out
            }
            Record::Closed { reason, exit_code } => {
                let mut out = Vec::new();
                if std::mem::take(&mut self.attached) {
                    out.push(StreamRecord::Controller { attached: false });
                }
                let code = is_controller_held(&reason).then_some(CloseCode::ControllerHeld);
                out.push(StreamRecord::Closed {
                    reason,
                    exit_code,
                    code,
                });
                out
            }
        }
    }
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
}

#[derive(Default)]
pub struct Sessions {
    next_id: AtomicU32,
    open: Mutex<HashMap<u32, Session>>,
}

pub struct OpenRequest<'a> {
    pub bin: &'a Path,
    pub mode: Mode,
    pub target: &'a str,
    pub cols: u16,
    pub rows: u16,
    pub takeover: bool,
}

impl Sessions {
    /// Spawns a stream and calls `on_record` from a reader thread for every record.
    /// The last record is always `Closed` (with stderr as reason when the CLI failed).
    pub fn open(
        &self,
        req: OpenRequest<'_>,
        on_record: impl Fn(u32, Record) + Send + 'static,
    ) -> Result<u32, String> {
        let mut child = crate::gui_path::command(req.bin)
            .args(stream_args(
                req.mode,
                req.target,
                req.cols,
                req.rows,
                req.takeover,
            ))
            .stdin(if req.mode == Mode::Control {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("failed to start {}: {err}", req.bin.display()))?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let stdin = child.stdin.take();

        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        std::thread::spawn(move || {
            let mut closed = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(record) = parse_record(&line) {
                    closed |= matches!(record, Record::Closed { .. });
                    on_record(id, record);
                }
            }
            if !closed {
                let stderr = stderr_reader.join().unwrap_or_default();
                let reason = match stderr.trim() {
                    "" => "stream ended".to_owned(),
                    text => text.to_owned(),
                };
                on_record(
                    id,
                    Record::Closed {
                        reason,
                        exit_code: None,
                    },
                );
            }
        });

        self.open
            .lock()
            .unwrap()
            .insert(id, Session { child, stdin });
        Ok(id)
    }

    pub fn send(&self, id: u32, line: &str) -> Result<(), String> {
        let mut open = self.open.lock().unwrap();
        let session = open.get_mut(&id).ok_or("unknown terminal session")?;
        let stdin = session.stdin.as_mut().ok_or("session is read-only")?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|err| err.to_string())
    }

    pub fn close(&self, id: u32) {
        let Some(mut session) = self.open.lock().unwrap().remove(&id) else {
            return;
        };
        if let Some(stdin) = session.stdin.as_mut() {
            // Release gives up control cleanly; the herdr pane keeps running.
            let _ = stdin.write_all(release_command().as_bytes());
        }
        drop(session.stdin.take());
        let _ = session.child.kill();
        let _ = session.child.wait();
    }

    pub fn close_all(&self) {
        let ids: Vec<u32> = self.open.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_args() {
        assert_eq!(
            stream_args(Mode::Observe, "w1:p1", 120, 40, true),
            [
                "terminal", "session", "observe", "w1:p1", "--cols", "120", "--rows", "40"
            ]
        );
    }

    #[test]
    fn control_args_with_takeover() {
        assert_eq!(
            stream_args(Mode::Control, "w1:p1", 80, 24, true),
            [
                "terminal",
                "session",
                "control",
                "w1:p1",
                "--takeover",
                "--cols",
                "80",
                "--rows",
                "24"
            ]
        );
    }

    #[test]
    fn parses_frame_and_keeps_base64() {
        let record = parse_record(
            r#"{"type":"terminal.frame","seq":3,"encoding":"ansi","width":80,"height":24,"full":true,"bytes":"G1sySg=="}"#,
        );
        assert_eq!(
            record,
            Some(Record::Frame {
                seq: 3,
                full: true,
                width: 80,
                height: 24,
                bytes: "G1sySg==".into()
            })
        );
    }

    #[test]
    fn parses_closed() {
        assert_eq!(
            parse_record(r#"{"type":"terminal.closed","reason":"detached"}"#),
            Some(Record::Closed {
                reason: "detached".into(),
                exit_code: None
            })
        );
    }

    #[test]
    fn ignores_unknown_and_invalid_lines() {
        assert_eq!(parse_record(r#"{"type":"terminal.graphics"}"#), None);
        assert_eq!(parse_record("not json"), None);
    }

    #[test]
    fn commands_are_json_lines() {
        let parse = |line: String| -> Value {
            assert!(line.ends_with('\n'));
            serde_json::from_str(line.trim()).unwrap()
        };
        assert_eq!(
            parse(input_command("ls\r")),
            json!({ "type": "terminal.input", "text": "ls\r" })
        );
        assert_eq!(
            parse(resize_command(100, 30)),
            json!({ "type": "terminal.resize", "cols": 100, "rows": 30 })
        );
        assert_eq!(
            parse(scroll_command(true, 3)),
            json!({ "type": "terminal.scroll", "direction": "up", "lines": 3 })
        );
        assert_eq!(
            parse(mouse_command("down", "left", 12, 5, 0)),
            json!({
                "type": "terminal.mouse", "action": "down", "button": "left",
                "column": 12, "row": 5, "modifiers": 0
            })
        );
        assert!(validate_mouse("down", "left").is_ok());
        assert!(validate_mouse("move", "middle").is_ok());
        assert!(validate_mouse("click", "left").is_err());
        assert!(validate_mouse("down", "side").is_err());
        assert_eq!(
            parse(release_command()),
            json!({ "type": "terminal.release" })
        );
    }

    fn frame(full: bool, width: u16, height: u16) -> Record {
        Record::Frame {
            seq: 1,
            full,
            width,
            height,
            bytes: "AA==".into(),
        }
    }

    #[test]
    fn a_full_frame_resets_and_later_frames_are_output() {
        let mut mapper = FrameMapper::new(Mode::Observe);
        assert_eq!(
            mapper.map(frame(true, 80, 24)),
            [StreamRecord::Reset {
                cols: 80,
                rows: 24,
                bytes: "AA==".into()
            }]
        );
        assert_eq!(
            mapper.map(frame(false, 80, 24)),
            [StreamRecord::Output {
                bytes: "AA==".into()
            }]
        );
    }

    #[test]
    fn a_changed_size_is_announced_before_the_output() {
        let mut mapper = FrameMapper::new(Mode::Observe);
        mapper.map(frame(true, 80, 24));
        assert_eq!(
            mapper.map(frame(false, 100, 30)),
            [
                StreamRecord::Resized {
                    cols: 100,
                    rows: 30
                },
                StreamRecord::Output {
                    bytes: "AA==".into()
                }
            ]
        );
    }

    #[test]
    fn only_a_control_stream_reports_the_controller() {
        let mut observer = FrameMapper::new(Mode::Observe);
        assert_eq!(observer.map(frame(true, 80, 24)).len(), 1);
        let mut controller = FrameMapper::new(Mode::Control);
        let first = controller.map(frame(true, 80, 24));
        assert_eq!(
            first.last(),
            Some(&StreamRecord::Controller { attached: true })
        );
        // Reported once.
        assert_eq!(controller.map(frame(false, 80, 24)).len(), 1);
        let closed = controller.map(Record::Closed {
            reason: "detached".into(),
            exit_code: None,
        });
        assert_eq!(
            closed,
            [
                StreamRecord::Controller { attached: false },
                StreamRecord::Closed {
                    reason: "detached".into(),
                    exit_code: None,
                    code: None
                }
            ]
        );
    }

    #[test]
    fn a_held_controller_is_a_code_not_text() {
        let held = FrameMapper::new(Mode::Control).map(Record::Closed {
            reason: "terminal already has an attached client; retry with --takeover".into(),
            exit_code: None,
        });
        assert_eq!(
            held,
            [StreamRecord::Closed {
                reason: "terminal already has an attached client; retry with --takeover".into(),
                exit_code: None,
                code: Some(CloseCode::ControllerHeld)
            }]
        );
        let crashed = FrameMapper::new(Mode::Control).map(Record::Closed {
            reason: "pane not found".into(),
            exit_code: Some(1),
        });
        assert_eq!(
            crashed,
            [StreamRecord::Closed {
                reason: "pane not found".into(),
                exit_code: Some(1),
                code: None
            }]
        );
        assert!(is_controller_held("Controlled by another client"));
        assert!(!is_controller_held("spawn failed"));
    }

    #[test]
    fn neutral_records_serialize_with_snake_case_tags() {
        let json = serde_json::to_value(StreamRecord::Closed {
            reason: "x".into(),
            exit_code: None,
            code: Some(CloseCode::ControllerHeld),
        })
        .unwrap();
        assert_eq!(
            json,
            json!({ "type": "closed", "reason": "x", "code": "controller_held" })
        );
        let json = serde_json::to_value(StreamRecord::Reset {
            cols: 2,
            rows: 1,
            bytes: "AA==".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            json!({ "type": "reset", "cols": 2, "rows": 1, "bytes": "AA==" })
        );
    }

    #[test]
    fn parses_the_exit_code_of_a_closed_record() {
        assert_eq!(
            parse_record(r#"{"type":"terminal.closed","reason":"exited","exit_code":3}"#),
            Some(Record::Closed {
                reason: "exited".into(),
                exit_code: Some(3)
            })
        );
    }

    /// A `PATH` of `parts` with the separator of this system.
    fn join(parts: &[&str]) -> String {
        std::env::join_paths(parts)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn resolve_bin_prefers_override_then_path() {
        let nothing = |_: &Path| false;
        let a_then_b = join(&["/a", "", "/b"]);
        assert_eq!(
            resolve_bin(Some("/x/herdr".into()), &join(&["/a"]), nothing),
            PathBuf::from("/x/herdr")
        );
        let found = resolve_bin(None, &a_then_b, |p: &Path| {
            p == Path::new("/b").join("herdr")
        });
        assert_eq!(found, Path::new("/b").join("herdr"));
        let found = resolve_bin(Some(String::new()), &join(&["/a"]), |p: &Path| {
            p == Path::new("/a").join("herdr")
        });
        assert_eq!(found, Path::new("/a").join("herdr"));
        assert_eq!(
            resolve_bin(None, &join(&["/a"]), nothing),
            PathBuf::from("herdr")
        );
    }

    #[test]
    fn bin_names_put_pathext_extensions_first_on_windows_only() {
        assert_eq!(bin_names(false, Some(".EXE")), ["herdr"]);
        assert_eq!(
            bin_names(true, Some(".COM;.EXE;;exe")),
            ["herdr.com", "herdr.exe", "herdr"]
        );
        assert_eq!(bin_names(true, None)[..2], ["herdr.com", "herdr.exe"]);
    }

    #[test]
    fn resolve_bin_finds_herdr_exe_with_the_windows_names() {
        let path = join(&["/nope", "/win/bin"]);
        let names = bin_names(true, None);
        let found = resolve_bin_named(None, &path, &names, |p: &Path| {
            p == Path::new("/win/bin").join("herdr.exe")
        });
        assert_eq!(found, Path::new("/win/bin").join("herdr.exe"));
    }

    #[test]
    fn resolve_bin_finds_cargo_bin_in_the_gui_path() {
        let path = crate::gui_path::augmented_path(Some(&join(&["/usr/bin"])), "/h", "");
        let cargo = Path::new("/h").join(".cargo/bin").join("herdr");
        assert_eq!(resolve_bin(None, &path, |p: &Path| p == cargo), cargo);
    }
}
