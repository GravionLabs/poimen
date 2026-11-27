//! The reducer of the agent-events core (GravionLabs/agent-events, `SPEC.md` "Run state"): folds
//! the lines of one `<run>.jsonl` into the run state of `schema/v0/run-state.schema.json`. Displays
//! are core readers and depend on agent-events only: profiles (`deliver@1`, ...) are not read, their
//! types and fields are ignored. It passes every reader fixture `fixtures/state/<case>.jsonl` with
//! its `<case>.state.json` (`fixtures/agent-events/`, the pinned tag is in `SOURCE.txt`).
//! Pure: the clock and the pid check are parameters. Unknown types, fields and profiles are
//! ignored, an incomplete last line is skipped ([`LineTail`] leaves it for the next read), a gap in
//! `seq` is recorded and folded anyway, events of another `v` are skipped and reported.
//!
//! Shared by the app (`deliver.rs` folds the local files, `remote_runs.rs` the events a machine
//! forwards) and `poimen-server` (`runs.rs` forwards the files of its spaces, #692): the run-file
//! names of the jsonl-file binding ([`is_run_file`]) and [`LineTail`], which reads the complete
//! lines a file gained since the last read.

use std::collections::{BTreeMap, HashSet};
use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

mod lead;
mod location;
pub use lead::{LeadDependency, LeadGrant, LeadMember, LeadState};
pub use location::{
    DIR_ENV, Head, Heads, Origin, RESOLVE_TTL, RepoCache, RepoRef, attribute, first_head,
    legacy_dir, repo_name, shared_dir, shared_dir_here,
};

/// The event version this build understands.
pub const SUPPORTED_V: i64 = 1;

/// Without `run.finished`, a run whose last event is older than this is abandoned.
const ABANDONED_AFTER_SECS: f64 = 3600.0;

const KNOWN: [&str; 9] = [
    "run.started",
    "run.finished",
    "task.started",
    "task.finished",
    "step.started",
    "step.finished",
    "progress",
    "attention",
    "attention.resolved",
];
const TONES: [&str; 5] = ["neutral", "info", "success", "warning", "danger"];
const OUTCOMES: [&str; 4] = ["succeeded", "failed", "cancelled", "skipped"];

/// The `host` key of the jsonl-file binding with the writer's pid.
const PID_KEY: &str = "jsonl-file.pid";

/// The legacy event directory relative to the git common dir (jsonl-file binding up to 0.2,
/// [`legacy_dir`]); run files live in the shared directory ([`shared_dir`]) since 0.3.
pub const EVENTS_PATH: &str = ".agents/events";

/// A larger event file is not read.
pub const MAX_EVENTS_BYTES: u64 = 16 << 20;

/// Whether `path` is a run file of the jsonl-file binding: `<run>.jsonl` with a run id of
/// `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`; files with other names are ignored.
pub fn is_run_file(path: &Path) -> bool {
    run_of(path).is_some()
}

/// The run id of a run file ([`is_run_file`]), `None` for any other name.
pub fn run_of(path: &Path) -> Option<&str> {
    let (Some(stem), Some("jsonl")) = (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) else {
        return None;
    };
    let mut chars = stem.chars();
    (stem.len() <= 128
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    .then_some(stem)
}

/// Runs that ended or were abandoned longer ago than this are not shown (unless their subject is,
/// see [`SUBJECT_KEEP_SECS`]).
pub const RECENT_SECS: f64 = 24.0 * 3600.0;

/// Once one run of a subject is shown, every run of that subject whose last event is at most this
/// old (14 days) is kept too, so the views can list all runs of a milestone. A file whose last
/// change is older than this is not opened.
pub const SUBJECT_KEEP_SECS: f64 = 14.0 * 24.0 * 3600.0;

/// At most this many runs of one subject are kept, newest first.
pub const MAX_SUBJECT_RUNS: usize = 50;

/// The identity of a run's subject: `(kind, id)` of `subject` in `run.started`.
pub type SubjectKey = (String, String);

/// `(kind, id)` of a `subject` object; `None` when either is missing or not a string.
pub fn subject_key(subject: &Value) -> Option<SubjectKey> {
    Some((
        subject.get("kind")?.as_str()?.to_owned(),
        subject.get("id")?.as_str()?.to_owned(),
    ))
}

/// Where the reading of one event file stands: the offset after its last complete line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineTail {
    offset: u64,
}

/// What [`LineTail::read`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tailed {
    /// The file is shorter than what was read before (truncated or replaced): `lines` start from
    /// its beginning, and what was folded before is to be dropped.
    pub reset: bool,
    /// The complete, non-blank lines added since the last read, in file order.
    pub lines: Vec<String>,
}

impl LineTail {
    /// Reads the complete lines `path` gained since the last read; `len` is its current length.
    /// An incomplete last line is still being written and is left for the next read.
    pub fn read(&mut self, path: &Path, len: u64) -> std::io::Result<Tailed> {
        let mut out = Tailed::default();
        if len < self.offset {
            out.reset = true;
            self.offset = 0;
        }
        if len == self.offset {
            return Ok(out);
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        file.take(len - self.offset).read_to_end(&mut buf)?;
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
            return Ok(out);
        };
        out.lines = String::from_utf8_lossy(&buf[..=end])
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect();
        self.offset += end as u64 + 1;
        Ok(out)
    }
}

/// An open task: `subject`, `step`, `progress` and `links` are passed on as the writer sent them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Task {
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub subject: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Value>,
}

/// A finished task (no step and progress any more).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoneTask {
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub subject: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Finished {
    pub succeeded: Vec<DoneTask>,
    pub failed: Vec<DoneTask>,
    pub cancelled: Vec<DoneTask>,
    pub skipped: Vec<DoneTask>,
}

impl Finished {
    fn list(&mut self, outcome: &str) -> Option<&mut Vec<DoneTask>> {
        Some(match outcome {
            "succeeded" => &mut self.succeeded,
            "failed" => &mut self.failed,
            "cancelled" => &mut self.cancelled,
            "skipped" => &mut self.skipped,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub needs_you: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    pub from: i64,
    pub to: i64,
}

/// The core run state of `run-state.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub v: i64,
    pub run: String,
    /// `running`, `finished` or `abandoned`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
    /// The latest step without a task: the step of the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<Value>,
    pub tasks: Vec<Task>,
    pub finished: Finished,
    pub attentions: Vec<Attention>,
    pub needs_you: bool,
    pub links: Vec<Value>,
    pub gaps: Vec<Gap>,
    pub last_seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

/// A run for the panel: the core state plus the `tone` and `host` the writer set.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    #[serde(flatten)]
    pub state: RunState,
    /// `tone` of the run: of `run.finished` once it ended, else of the last event.
    pub tone: String,
    /// `host` of `run.started`: namespaced keys to strings (`poimen.terminal`, `herdr.pane`).
    pub host: BTreeMap<String, String>,
    /// `agent` of `run.started` (`harness`, `model`), as sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Value>,
    /// `origin` of `run.started`: the repository, branch, worktree and machine of the writer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// The tasks `run.finished` found open, with their step and progress: the core state lists
    /// them as cancelled, the Runs view shows them as left open.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub left_open: Vec<Task>,
    /// One row per event of a known type, in file order.
    pub events: Vec<RunEvent>,
    /// The members, dependencies and grant of a lead run (`orchestrate@1`, #1213); absent for
    /// any other run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead: Option<LeadState>,
}

/// One event as the Events tab shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunEvent {
    pub seq: i64,
    pub ts: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub tone: String,
    pub task: Option<String>,
    pub step: Option<String>,
    pub outcome: Option<String>,
    /// URL of the first link of the event.
    pub link: Option<String>,
    /// `reason` of the event (core field), when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// `text` of the event (core field), when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Title (else id) of the `subject` of the event, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

/// Seconds since the epoch of an ISO 8601 timestamp of the schema (`Z` or an offset, optional
/// fraction); `None` when it does not match.
pub fn parse_ts(ts: &str) -> Option<f64> {
    let b = ts.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |a, d| a * 10 + i64::from(d - b'0')))
    };
    let sep = |at: usize, c: u8| (b.get(at) == Some(&c)).then_some(());
    let (year, month, day) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    sep(4, b'-')?;
    sep(7, b'-')?;
    sep(10, b'T')?;
    let (hour, min, sec) = (digits(11, 2)?, digits(14, 2)?, digits(17, 2)?);
    sep(13, b':')?;
    sep(16, b':')?;
    let mut at = 19;
    let mut fraction = 0.0;
    if b.get(at) == Some(&b'.') {
        let start = at + 1;
        let mut end = start;
        while b.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return None;
        }
        fraction = format!("0.{}", &ts[start..end]).parse().ok()?;
        at = end;
    }
    let offset = match b.get(at)? {
        b'Z' if at + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if at + 6 == b.len() => {
            sep(at + 3, b':')?;
            let secs = digits(at + 1, 2)? * 3600 + digits(at + 4, 2)? * 60;
            if *sign == b'+' { secs } else { -secs }
        }
        _ => return None,
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hour * 3600 + min * 60 + sec - offset) as f64 + fraction)
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn add_links(into: &mut Vec<Value>, more: &[Value]) {
    let key = |v: &Value| (v.get("rel").cloned(), v.get("url").cloned());
    for l in more {
        if !into.iter().any(|x| key(x) == key(l)) {
            into.push(l.clone());
        }
    }
}

fn links_of(e: &Value) -> Vec<Value> {
    e.get("links")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter(|v| v.is_object()).cloned().collect())
        .unwrap_or_default()
}

/// Folds events one by one; `snapshot` gives the state at any point, so a follower feeds new
/// lines only.
#[derive(Debug, Clone)]
pub struct Reducer {
    events: usize,
    /// The first `v` this build does not understand; its events are skipped.
    pub unknown_v: Option<i64>,
    state: RunState,
    seen: HashSet<String>,
    last_ts: Option<f64>,
    pid: Option<u32>,
    tone: String,
    host: BTreeMap<String, String>,
    agent: Option<Value>,
    origin: Option<Origin>,
    rows: Vec<RunEvent>,
    left_open: Vec<Task>,
    lead: Option<LeadState>,
}

impl Default for Reducer {
    fn default() -> Self {
        Self::new()
    }
}

impl Reducer {
    pub fn new() -> Self {
        Reducer {
            events: 0,
            unknown_v: None,
            state: RunState {
                v: SUPPORTED_V,
                run: String::new(),
                status: "running".into(),
                outcome: None,
                reason: None,
                text: None,
                subject: None,
                progress: None,
                step: None,
                tasks: Vec::new(),
                finished: Finished::default(),
                attentions: Vec::new(),
                needs_you: false,
                links: Vec::new(),
                gaps: Vec::new(),
                last_seq: 0,
                updated: None,
            },
            seen: HashSet::new(),
            last_ts: None,
            pid: None,
            tone: "neutral".into(),
            host: BTreeMap::new(),
            agent: None,
            origin: None,
            rows: Vec::new(),
            left_open: Vec::new(),
            lead: None,
        }
    }

    /// Whether any event of a known type was folded.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.events == 0
    }

    /// Folds one line; a line that is not a JSON object is skipped (a half-written last line).
    /// Returns whether it was an event.
    pub fn push_line(&mut self, line: &str) -> bool {
        match serde_json::from_str::<Value>(line) {
            Ok(event) if event.is_object() => {
                self.push(&event);
                true
            }
            _ => false,
        }
    }

    fn task_mut(&mut self, e: &Value) -> Option<&mut Task> {
        let id = e.get("task").and_then(Value::as_str)?;
        self.state.tasks.iter_mut().find(|t| t.task == id)
    }

    pub fn push(&mut self, e: &Value) {
        let v = e.get("v").and_then(Value::as_i64);
        if v != Some(SUPPORTED_V) {
            if self.unknown_v.is_none() && e.get("type").is_some() {
                self.unknown_v = Some(v.unwrap_or(0));
            }
            return;
        }
        let Some(seq) = e.get("seq").and_then(Value::as_i64) else {
            return;
        };
        if seq <= self.state.last_seq {
            return;
        }
        if seq > self.state.last_seq + 1 {
            self.state.gaps.push(Gap {
                from: self.state.last_seq + 1,
                to: seq - 1,
            });
        }
        self.state.last_seq = seq;
        let ts = e.get("ts").and_then(Value::as_str);
        if let Some(ts) = ts {
            self.state.updated = Some(ts.to_owned());
            if let Some(secs) = parse_ts(ts) {
                self.last_ts = Some(secs);
            }
        }
        if self.state.run.is_empty() {
            self.state.run = e.get("run").and_then(text).unwrap_or_default();
        }
        // Nothing folds after run.finished.
        if self.state.status == "finished" {
            return;
        }
        let mut lead = self.lead.take().unwrap_or_default();
        let is_lead = lead.push(e);
        if is_lead || lead != LeadState::default() {
            self.lead = Some(lead);
        }
        if is_lead {
            return;
        }
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
        if !KNOWN.contains(&ty) {
            return;
        }
        self.events += 1;
        let tone = e
            .get("tone")
            .and_then(Value::as_str)
            .filter(|t| TONES.contains(t))
            .unwrap_or("neutral");
        self.tone = tone.to_owned();
        let step = e.get("step").filter(|s| s.is_object()).and_then(|s| {
            let name = s.get("name").and_then(text)?;
            Some(match s.get("text").and_then(text) {
                Some(t) => format!("{name}: {t}"),
                None => name,
            })
        });
        self.rows.push(RunEvent {
            seq,
            ts: ts.map(str::to_owned),
            kind: ty.to_owned(),
            tone: tone.to_owned(),
            task: e.get("task").and_then(text),
            step,
            outcome: e.get("outcome").and_then(text),
            link: links_of(e)
                .first()
                .and_then(|l| l.get("url"))
                .and_then(text),
            reason: e.get("reason").and_then(text),
            text: e.get("text").and_then(text),
            subject: e.get("subject").filter(|s| s.is_object()).and_then(|s| {
                s.get("title")
                    .and_then(text)
                    .or_else(|| s.get("id").and_then(text))
            }),
        });

        match ty {
            "run.started" => {
                if let Some(s) = e.get("subject").filter(|s| s.is_object()) {
                    self.state.subject = Some(s.clone());
                }
                if let Some(h) = e.get("host").and_then(Value::as_object) {
                    self.host = h
                        .iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                        .collect();
                    self.pid = self.host.get(PID_KEY).and_then(|p| p.parse().ok());
                }
                self.agent = e.get("agent").filter(|a| a.is_object()).cloned();
                self.origin = Origin::of(e);
            }
            "run.finished" => {
                self.state.status = "finished".into();
                self.state.outcome = e.get("outcome").and_then(text);
                self.state.reason = e.get("reason").and_then(text);
                self.state.text = e.get("text").and_then(text);
                self.state.attentions.clear();
                self.left_open = std::mem::take(&mut self.state.tasks);
                let cancelled = self.left_open.iter().map(|t| DoneTask {
                    task: t.task.clone(),
                    parent: t.parent.clone(),
                    subject: t.subject.clone(),
                    links: t.links.clone(),
                    reason: Some("run-finished".into()),
                    text: Some("run finished".into()),
                });
                self.state.finished.cancelled.extend(cancelled);
            }
            "task.started" => {
                let Some(id) = e.get("task").and_then(text) else {
                    return;
                };
                let Some(subject) = e.get("subject").filter(|s| s.is_object()) else {
                    return;
                };
                if self.seen.insert(id.clone()) {
                    self.state.tasks.push(Task {
                        task: id,
                        parent: e.get("parent").and_then(text),
                        subject: subject.clone(),
                        step: None,
                        progress: None,
                        links: Vec::new(),
                    });
                }
            }
            "task.finished" => {
                let outcome = e.get("outcome").and_then(Value::as_str).unwrap_or("");
                let at = e
                    .get("task")
                    .and_then(Value::as_str)
                    .and_then(|id| self.state.tasks.iter().position(|t| t.task == id));
                let (Some(at), true) = (at, OUTCOMES.contains(&outcome)) else {
                    return;
                };
                let t = self.state.tasks.remove(at);
                self.state
                    .attentions
                    .retain(|a| a.task.as_deref() != Some(t.task.as_str()));
                let done = DoneTask {
                    task: t.task,
                    parent: t.parent,
                    subject: t.subject,
                    links: t.links,
                    reason: e.get("reason").and_then(text),
                    text: e.get("text").and_then(text),
                };
                if let Some(list) = self.state.finished.list(outcome) {
                    list.push(done);
                }
            }
            "step.started" | "step.finished" => {
                let links = links_of(e);
                let Some(step) = e.get("step").filter(|s| s.is_object()).cloned() else {
                    return;
                };
                let finished = ty == "step.finished" && !links.is_empty();
                if e.get("task").is_none() {
                    self.state.step = Some(step);
                } else if let Some(t) = self.task_mut(e) {
                    t.step = Some(step);
                    if finished {
                        add_links(&mut t.links, &links);
                    }
                } else {
                    return;
                }
                if finished {
                    add_links(&mut self.state.links, &links);
                }
            }
            "progress" => {
                let mut p = serde_json::Map::new();
                p.insert("done".into(), e.get("done").cloned().unwrap_or(Value::Null));
                p.insert(
                    "total".into(),
                    e.get("total").cloned().unwrap_or(Value::Null),
                );
                if let Some(label) = e.get("label") {
                    p.insert("label".into(), label.clone());
                }
                let p = Value::Object(p);
                if e.get("task").is_none() {
                    self.state.progress = Some(p);
                } else if let Some(t) = self.task_mut(e) {
                    t.progress = Some(p);
                }
            }
            "attention" => {
                let Some(id) = e.get("id").and_then(text) else {
                    return;
                };
                if self.state.attentions.iter().any(|a| a.id == id) {
                    return;
                }
                let links = e.get("links").and_then(Value::as_array).cloned();
                if let Some(l) = &links {
                    add_links(&mut self.state.links, l);
                }
                self.state.attentions.push(Attention {
                    id,
                    task: e.get("task").and_then(text),
                    reason: e.get("reason").and_then(text).unwrap_or_default(),
                    text: e.get("text").and_then(text),
                    needs_you: e.get("needsYou").and_then(Value::as_bool).unwrap_or(false),
                    links,
                });
            }
            "attention.resolved" => {
                let id = e.get("id").and_then(Value::as_str);
                self.state.attentions.retain(|a| Some(a.id.as_str()) != id);
            }
            _ => {}
        }
        self.state.needs_you = self.state.attentions.iter().any(|a| a.needs_you);
    }

    /// The run as of `now` (epoch seconds): `None` before the first event. `pid_alive` is asked
    /// for the writer pid of a run without `run.finished`.
    pub fn snapshot(&self, now: f64, pid_alive: &dyn Fn(u32) -> bool) -> Option<Run> {
        if self.events == 0 {
            return None;
        }
        let mut state = self.state.clone();
        if state.status == "running" {
            let old = self.last_ts.is_some_and(|t| now - t > ABANDONED_AFTER_SECS);
            let dead = self.pid.is_some_and(|p| !pid_alive(p));
            if old || dead {
                state.status = "abandoned".into();
            }
        }
        let tone = if state.status == "abandoned" {
            "neutral".to_owned()
        } else {
            self.tone.clone()
        };
        Some(Run {
            state,
            tone,
            host: self.host.clone(),
            agent: self.agent.clone(),
            left_open: self.left_open.clone(),
            origin: self.origin.clone(),
            events: self.rows.clone(),
            lead: self.lead.clone(),
        })
    }
}

/// Folds the whole text of an event file; an incomplete last line (no newline) is skipped.
#[cfg(test)]
pub fn reduce(text: &str, now: f64, pid_alive: &dyn Fn(u32) -> bool) -> Option<Run> {
    let mut reducer = Reducer::new();
    let complete = text.rfind('\n').map_or("", |i| &text[..=i]);
    for line in complete.lines().filter(|l| !l.trim().is_empty()) {
        reducer.push_line(line);
    }
    reducer.snapshot(now, pid_alive)
}

/// Whether a process with this pid lives. Where this cannot be asked cheaply (Windows) the run
/// counts as alive and only the age rule applies.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        crate::gui_path::command("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(true)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests;
