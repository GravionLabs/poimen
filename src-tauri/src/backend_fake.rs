//! A fake `Backend` for tests of the command layer: it records every call as a line of text and
//! answers from what the test set up. It never starts a process or touches a socket.

use std::sync::Mutex;

use serde_json::json;

use crate::backend::{
    Backend, BackendEvent, BackendInfo, LaidOut, LayoutRequest, NewSpace, OnRecord, OpenedStream,
    Outcome, Snapshot, SplitDirection, StartError, StreamRecord, StreamRequest,
};
use crate::herdr::{AgentSession, PromptOutcome};

pub struct FakeBackend {
    calls: Mutex<Vec<String>>,
    fail_on: Mutex<Option<String>>,
    /// What `snapshot` returns.
    pub snapshot: Mutex<Snapshot>,
    /// Records `open_stream` delivers before it returns.
    pub records: Mutex<Vec<StreamRecord>>,
    /// Whether opened streams are emulated.
    pub emulated: bool,
    /// What `create_space_with_layout` answers; `None`: the fake does not build layouts (herdr).
    pub laid_out: Mutex<Option<LaidOut>>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self {
            calls: Mutex::default(),
            fail_on: Mutex::default(),
            snapshot: Mutex::new(Snapshot {
                workspaces: json!([]),
                tabs: json!([]),
                panes: json!([]),
            }),
            records: Mutex::default(),
            emulated: false,
            laid_out: Mutex::default(),
        }
    }
}

impl FakeBackend {
    /// The recorded calls, oldest first.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// Makes every call whose line starts with `prefix` fail with `boom` (a failed call is not
    /// recorded).
    pub fn fail_on(&self, prefix: &str) {
        *self.fail_on.lock().unwrap() = Some(prefix.to_owned());
    }

    fn record(&self, line: String) -> Result<usize, String> {
        let mut calls = self.calls.lock().unwrap();
        let failing = self
            .fail_on
            .lock()
            .unwrap()
            .as_deref()
            .is_some_and(|prefix| line.starts_with(prefix));
        if failing {
            return Err("boom".into());
        }
        calls.push(line);
        Ok(calls.len() - 1)
    }
}

fn dir(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Right => "right",
        SplitDirection::Down => "down",
    }
}

impl Backend for FakeBackend {
    fn ensure_running(&self, auto_start: bool) -> Result<Outcome, StartError> {
        self.record(format!("ensure_running {auto_start}"))
            .map_err(StartError::Spawn)?;
        Ok(Outcome::AlreadyRunning)
    }

    fn info(&self) -> Option<BackendInfo> {
        None
    }

    fn snapshot(&self) -> Result<Snapshot, String> {
        self.record("snapshot".into())?;
        Ok(self.snapshot.lock().unwrap().clone())
    }

    fn create_space(&self, cwd: &str, label: &str) -> Result<NewSpace, String> {
        self.record(format!("create_space {cwd} {label}"))?;
        Ok(NewSpace {
            space_id: "w9".into(),
            root_terminal: Some("w9:root".into()),
        })
    }

    fn create_space_with_layout(
        &self,
        request: LayoutRequest<'_>,
    ) -> Option<Result<LaidOut, String>> {
        let answer = self.laid_out.lock().unwrap().clone()?;
        let decision = match request.decision {
            None => "-".to_owned(),
            Some(crate::trust::Decision::Trust { hash }) => format!("trust:{hash}"),
            Some(crate::trust::Decision::Without) => "without".to_owned(),
        };
        let line = format!(
            "create_space_with_layout {} {} {decision} {}",
            request.cwd,
            request.label,
            request.command.unwrap_or("-")
        );
        Some(self.record(line).map(|_| answer))
    }

    fn focus_space(&self, space_id: &str) -> Result<(), String> {
        self.record(format!("focus {space_id}")).map(drop)
    }

    fn close_space(&self, space_id: &str) -> Result<(), String> {
        self.record(format!("close_space {space_id}")).map(drop)
    }

    fn rename_space(&self, space_id: &str, label: &str) -> Result<(), String> {
        self.record(format!("rename_space {space_id} {label}"))
            .map(drop)
    }

    fn create_terminal(&self, space_id: &str, cwd: Option<&str>) -> Result<Option<String>, String> {
        let n = self.record(format!("create_terminal {space_id} {}", cwd.unwrap_or("-")))?;
        Ok(Some(format!("{space_id}:t{n}")))
    }

    fn split(
        &self,
        target: &str,
        direction: SplitDirection,
        cwd: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Option<String>, String> {
        let ratio = ratio.map_or("-".to_owned(), |r| r.to_string());
        let n = self.record(format!(
            "split {target} {} {} {ratio}",
            dir(direction),
            cwd.unwrap_or("-")
        ))?;
        // The id depends on the number of calls so far, like a server that numbers its panes.
        Ok(Some(format!("w1:p{}", n + 10)))
    }

    fn close_terminal(&self, terminal_id: &str) -> Result<(), String> {
        self.record(format!("close_terminal {terminal_id}"))
            .map(drop)
    }

    fn rename_terminal(&self, terminal_id: &str, label: Option<&str>) -> Result<(), String> {
        self.record(format!("rename {terminal_id} {}", label.unwrap_or("-")))
            .map(drop)
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), String> {
        self.record(format!("close_tab {tab_id}")).map(drop)
    }

    fn rename_tab(&self, tab_id: &str, name: &str) -> Result<(), String> {
        self.record(format!("rename_tab {tab_id} {name}")).map(drop)
    }

    fn resize_split(&self, tab_id: &str, split_id: &str, ratio: f64) -> Result<(), String> {
        self.record(format!("resize_split {tab_id} {split_id} {ratio}"))
            .map(drop)
    }

    fn focus_tab(&self, tab_id: &str, terminal_id: &str) -> Result<(), String> {
        self.record(format!("focus_tab {tab_id} {terminal_id}"))
            .map(drop)
    }

    fn send_input(&self, terminal_id: &str, text: &str) -> Result<(), String> {
        self.record(format!("input {terminal_id} {text}")).map(drop)
    }

    fn agent_start(
        &self,
        terminal_id: &str,
        kind: &str,
        name: Option<&str>,
        args: &[String],
    ) -> Result<(), String> {
        self.record(format!(
            "agent_start {terminal_id} {kind} {} {}",
            name.unwrap_or("-"),
            args.join(",")
        ))
        .map(drop)
    }

    fn agent_rename(&self, target: &str, name: Option<&str>) -> Result<(), String> {
        self.record(format!("agent_rename {target} {}", name.unwrap_or("-")))
            .map(drop)
    }

    fn agent_prompt(&self, target: &str, text: &str) -> PromptOutcome {
        match self.record(format!("agent_prompt {target} {text}")) {
            Ok(_) => PromptOutcome::Started {
                status: "working".into(),
            },
            Err(message) => PromptOutcome::Failed { message },
        }
    }

    fn agent_session(&self, terminal_id: &str) -> Result<Option<AgentSession>, String> {
        self.record(format!("agent_session {terminal_id}"))?;
        Ok(None)
    }

    fn open_stream(
        &self,
        request: StreamRequest<'_>,
        on_record: OnRecord,
    ) -> Result<OpenedStream, String> {
        self.record(format!(
            "open {:?} {} {}x{} {}",
            request.mode, request.target, request.cols, request.rows, request.takeover
        ))?;
        for record in self.records.lock().unwrap().drain(..) {
            on_record(1, record);
        }
        Ok(OpenedStream {
            id: 1,
            emulated: self.emulated,
        })
    }

    fn emulated(&self) -> bool {
        self.emulated
    }

    fn stream_input(&self, id: u32, text: &str) -> Result<(), String> {
        self.record(format!("stream_input {id} {text}")).map(drop)
    }

    fn stream_resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        self.record(format!("stream_resize {id} {cols}x{rows}"))
            .map(drop)
    }

    fn stream_scroll(&self, id: u32, up: bool, lines: u16) -> Result<(), String> {
        self.record(format!("stream_scroll {id} {up} {lines}"))
            .map(drop)
    }

    fn stream_mouse(
        &self,
        id: u32,
        action: &str,
        button: &str,
        column: u16,
        row: u16,
        modifiers: u8,
    ) -> Result<(), String> {
        self.record(format!(
            "stream_mouse {id} {action} {button} {column},{row} {modifiers}"
        ))
        .map(drop)
    }

    fn close_stream(&self, id: u32) {
        let _ = self.record(format!("close_stream {id}"));
    }

    fn close_streams(&self) {
        let _ = self.record("close_streams".into());
    }

    fn watch(&self, emit: &dyn Fn(&BackendEvent)) {
        emit(&BackendEvent {
            kind: "poimen.connected".into(),
            data: json!(null),
        });
    }
}
