//! `wait_any` (#1210, ADR 0018): `wait_agent` over several terminals and runs. It subscribes to
//! the server's events and watches the event files of the runs before it reads any state, so no
//! change is missed, and it never polls the server. Run files are read with the core `Tailer`.

use super::runs::{events_dir, only_keys};
use super::{Fail, WAIT_DEFAULT_S, WAIT_MAX_S, report, server_error};
use notify::{RecursiveMode, Watcher};
use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, Event};
use poimen_core::agent_events::{self, Reducer};
use poimen_core::transcripts::Tailer;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) fn tool() -> Value {
    json!({
        "name": "wait_any",
        "description": "Waits on several targets at once and returns at the first that needs you: an agent of `terminals` that is done, blocked, in error or gone (closed or exited terminal), or a run of `runs` that is finished or needs you. Returns the target that fired with its status, the notification text when blocked, the `report_done` report when there is one, and the state of the other targets. A target already in such a state returns at once. Prefer it over `wait_agent` when you wait on more than one terminal or run (for example one deliver loop per repository); use `wait_agent` for a single terminal. A read tool: any space, no confirmation. Uses events, never polls the server.",
        "inputSchema": {"type": "object", "properties": {
            "terminals": {"type": "array", "items": {"type": "string"}, "description": "Terminal ids."},
            "runs": {"type": "array", "items": {"type": "object", "properties": {
                "repository": {"type": "string", "description": "A directory inside the repository."},
                "run": {"type": "string", "description": "Run id."}
            }, "required": ["repository", "run"], "additionalProperties": false}, "description": "Runs of the Runs view."},
            "timeout_s": {"type": "number", "description": "Seconds to wait (default 300, at most 3600). On timeout nothing fired: `fired` is null and `timed_out` true."}
        }, "additionalProperties": false},
    })
}

/// One thing that wakes the wait.
enum Msg {
    Event(Box<Result<Event, poimen_client::ClientError>>),
    /// A run file or its directory changed.
    Files,
}

struct TerminalTarget {
    id: String,
    status: AgentStatus,
    message: Option<String>,
}

struct RunTarget {
    repository: String,
    run: String,
    tailer: Tailer,
    reducer: Reducer,
}

impl RunTarget {
    fn refresh(&mut self) {
        for line in self.tailer.read_new() {
            self.reducer.push_line(&line);
        }
    }

    /// `finished` or `needs-you` when the run is in a state that ends the wait; else `running`
    /// (or `abandoned`).
    fn status(&self) -> &'static str {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        match self.reducer.snapshot(now, &agent_events::pid_alive) {
            Some(r) if r.state.status == "finished" => "finished",
            Some(r) if r.state.needs_you => "needs-you",
            Some(r) if r.state.status == "abandoned" => "abandoned",
            _ => "running",
        }
    }

    fn json(&self) -> Value {
        let status = self.status();
        let mut v = json!({"type": "run", "repository": self.repository, "run": self.run, "status": status});
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        if let (Some(r), Some(o)) = (
            self.reducer.snapshot(now, &agent_events::pid_alive),
            v.as_object_mut(),
        ) {
            if let Some(outcome) = r.state.outcome {
                o.insert("outcome".into(), json!(outcome));
            }
            if let Some(reason) = r.state.reason {
                o.insert("reason".into(), json!(reason));
            }
            let texts: Vec<_> = r
                .state
                .attentions
                .iter()
                .filter(|a| a.needs_you)
                .map(|a| a.text.clone().unwrap_or_else(|| a.reason.clone()))
                .collect();
            if status == "needs-you" && !texts.is_empty() {
                o.insert("attentions".into(), json!(texts));
            }
        }
        v
    }
}

fn is_final(status: AgentStatus) -> bool {
    super::is_final(status)
}

impl TerminalTarget {
    fn json(&self) -> Value {
        let status = serde_json::to_value(self.status).unwrap_or_default();
        let mut v = json!({"type": "terminal", "terminal": self.id, "status": status});
        if let (Some(m), Some(o)) = (
            self.message
                .as_ref()
                .filter(|_| self.status == AgentStatus::Blocked),
            v.as_object_mut(),
        ) {
            o.insert("message".into(), json!(m));
        }
        v
    }
}

fn string_list(args: &Value, key: &str) -> Result<Vec<String>, Fail> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| {
                i.as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| Fail::Tool(format!("`{key}` holds terminal ids (strings)")))
            })
            .collect(),
        Some(_) => Err(Fail::Tool(format!("`{key}` must be an array"))),
    }
}

fn run_specs(args: &Value) -> Result<Vec<(String, String)>, Fail> {
    let bad = || Fail::Tool("`runs` holds objects `{repository, run}`".into());
    match args.get("runs") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| {
                let field = |k: &str| {
                    i.get(k)
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                only_keys(i, &["repository", "run"])?;
                Ok((
                    field("repository").ok_or_else(bad)?,
                    field("run").ok_or_else(bad)?,
                ))
            })
            .collect(),
        Some(_) => Err(Fail::Tool("`runs` must be an array".into())),
    }
}

/// The event file of a run; an unknown run is an error.
fn run_file(repository: &str, run: &str) -> Result<PathBuf, Fail> {
    let path = events_dir(repository)
        .map_err(Fail::Tool)?
        .join(format!("{run}.jsonl"));
    let known = !run.contains(['/', '\\'])
        && agent_events::run_of(&path) == Some(run)
        && std::fs::metadata(&path).is_ok_and(|m| m.is_file());
    if known {
        Ok(path)
    } else {
        Err(Fail::Tool(format!("unknown run `{run}` in `{repository}`")))
    }
}

/// `wait_any {terminals?, runs?, timeout_s?}`.
pub(super) fn wait_any(client: &Client, args: &Value) -> Result<Value, Fail> {
    only_keys(args, &["terminals", "runs", "timeout_s"])?;
    let mut ids = string_list(args, "terminals")?;
    ids.dedup();
    let specs = run_specs(args)?;
    if ids.is_empty() && specs.is_empty() {
        return Err(Fail::Tool(
            "give at least one of `terminals` and `runs`".into(),
        ));
    }
    let seconds = args
        .get("timeout_s")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or(WAIT_DEFAULT_S)
        .min(WAIT_MAX_S);
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);

    // Subscribe and watch first, read the state after.
    let (tx, rx) = mpsc::channel();
    let mut runs = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for (repository, run) in &specs {
        let path = run_file(repository, run)?;
        if let Some(dir) = path.parent().map(Path::to_path_buf)
            && !dirs.contains(&dir)
        {
            dirs.push(dir);
        }
        runs.push((repository.clone(), run.clone(), path));
    }
    let watcher = if dirs.is_empty() {
        None
    } else {
        let notifier = tx.clone();
        let mut w = notify::recommended_watcher(move |e: notify::Result<notify::Event>| {
            if e.is_ok() {
                let _ = notifier.send(Msg::Files);
            }
        })
        .map_err(|e| Fail::Tool(format!("cannot watch the runs: {e}")))?;
        for dir in &dirs {
            w.watch(dir, RecursiveMode::NonRecursive)
                .map_err(|e| Fail::Tool(format!("cannot watch the runs: {e}")))?;
        }
        Some(w)
    };
    let mut runs: Vec<RunTarget> = runs
        .into_iter()
        .map(|(repository, run, path)| RunTarget {
            repository,
            run,
            tailer: Tailer::new(path),
            reducer: Reducer::new(),
        })
        .collect();
    let mut terminals: Vec<TerminalTarget> = Vec::new();
    if !ids.is_empty() {
        let stream = client.subscribe().map_err(server_error)?;
        let events = tx.clone();
        std::thread::spawn(move || {
            for event in stream {
                if events.send(Msg::Event(Box::new(event))).is_err() {
                    break;
                }
            }
        });
        let listed = client.list_terminals(None).map_err(server_error)?;
        for id in &ids {
            let t = listed
                .iter()
                .find(|t| &t.id == id)
                .ok_or_else(|| Fail::Tool(format!("terminal {id} not found")))?;
            let status = t.agent.as_ref().map_or(AgentStatus::Gone, |a| a.status);
            terminals.push(TerminalTarget {
                id: id.clone(),
                status,
                message: t.agent.as_ref().and_then(|a| a.message.clone()),
            });
        }
    }
    drop(tx);

    let fired = loop {
        for r in &mut runs {
            r.refresh();
        }
        let hit = terminals
            .iter()
            .position(|t| is_final(t.status))
            .map(|i| (true, i))
            .or_else(|| {
                runs.iter()
                    .position(|r| matches!(r.status(), "finished" | "needs-you"))
                    .map(|i| (false, i))
            });
        if let Some(hit) = hit {
            break Some(hit);
        }
        let msg = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        match msg {
            Ok(Msg::Files) => {}
            Ok(Msg::Event(result)) => match *result {
                Err(e) => return Err(server_error(e)),
                Ok(event) => match event {
                    Event::AgentStatusChanged {
                        terminal_id,
                        status,
                        message,
                        ..
                    } => {
                        if let Some(t) = terminals.iter_mut().find(|t| t.id == terminal_id) {
                            t.status = status;
                            t.message = message;
                        }
                    }
                    Event::TerminalClosed { terminal_id, .. }
                    | Event::TerminalExited { terminal_id, .. } => {
                        if let Some(t) = terminals.iter_mut().find(|t| t.id == terminal_id) {
                            t.status = AgentStatus::Gone;
                            t.message = None;
                        }
                    }
                    _ => {}
                },
            },
            Err(mpsc::RecvTimeoutError::Timeout) => break None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Fail::Tool("the event stream ended".into()));
            }
        }
    };
    drop(watcher);

    let mut others: Vec<Value> = Vec::new();
    let mut fired_json = Value::Null;
    for (i, t) in terminals.iter().enumerate() {
        let mut v = t.json();
        if fired == Some((true, i)) {
            fired_json = report::with_report(client, v);
        } else {
            if let Some(o) = v.as_object_mut() {
                o.remove("message");
            }
            others.push(v);
        }
    }
    for (i, r) in runs.iter().enumerate() {
        if fired == Some((false, i)) {
            fired_json = r.json();
        } else {
            others.push(r.json());
        }
    }
    Ok(json!({"fired": fired_json, "others": others, "timed_out": fired.is_none()}))
}
