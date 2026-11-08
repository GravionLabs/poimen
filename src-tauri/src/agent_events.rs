//! The reducer of the agent-events core (GravionLabs/agent-events, `SPEC.md` "Run state"): folds
//! the lines of one `<run>.jsonl` into the run state of `schema/v0/run-state.schema.json`. Displays
//! are core readers and depend on agent-events only: profiles (`deliver@1`, ...) are not read, their
//! types and fields are ignored. It passes every reader fixture `fixtures/state/<case>.jsonl` with
//! its `<case>.state.json` (`tests/fixtures/agent-events/`, the pinned tag is in `SOURCE.txt`).
//! Pure: the clock and the pid check are parameters. Unknown types, fields and profiles are
//! ignored, an incomplete last line is skipped (the `Tailer` of `deliver.rs` leaves it for the next
//! read), a gap in `seq` is recorded and folded anyway, events of another `v` are skipped and
//! reported.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::Value;

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
    pub subject: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
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
    /// One row per event of a known type, in file order.
    pub events: Vec<RunEvent>,
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
    rows: Vec<RunEvent>,
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
                subject: None,
                progress: None,
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
            rows: Vec::new(),
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
            }
            "run.finished" => {
                self.state.status = "finished".into();
                self.state.outcome = e.get("outcome").and_then(text);
                self.state.reason = e.get("reason").and_then(text);
                self.state.attentions.clear();
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
                let Some(t) = self.task_mut(e) else {
                    return;
                };
                t.step = Some(step);
                if finished {
                    add_links(&mut t.links, &links);
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
            events: self.rows.clone(),
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
mod tests {
    use super::*;

    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agent-events");

    fn fixture(name: &str, ext: &str) -> String {
        std::fs::read_to_string(format!("{DIR}/{name}.{ext}")).unwrap()
    }

    fn now() -> f64 {
        parse_ts("2026-11-08T12:00:00Z").unwrap()
    }

    /// The tag recorded in `SOURCE.txt`, the one place that pins it.
    fn pinned_tag() -> String {
        fixture("SOURCE", "txt")
            .lines()
            .find_map(|l| l.strip_prefix("tag: "))
            .unwrap()
            .trim()
            .to_owned()
    }

    #[test]
    fn the_pinned_tag_is_recorded_in_source_txt() {
        assert_eq!(pinned_tag(), "v0.1.0");
    }

    /// Every reader fixture of agent-events: events in, run state out. `<case>.now` fixes the
    /// clock (default 2026-11-08T10:30:00.000Z), no pid check.
    #[test]
    fn every_reader_fixture_of_agent_events_folds_to_its_state() {
        let mut names: Vec<String> = std::fs::read_dir(DIR)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".jsonl").map(str::to_owned)
            })
            .collect();
        names.sort();
        assert_eq!(names.len(), 10, "{names:?}");
        for name in names {
            let clock = std::fs::read_to_string(format!("{DIR}/{name}.now"))
                .unwrap_or_else(|_| "2026-11-08T10:30:00.000Z".into());
            let at = parse_ts(clock.trim()).unwrap();
            let run = reduce(&fixture(&name, "jsonl"), at, &|_| true)
                .unwrap_or_else(|| panic!("{name}: no run"));
            let got = serde_json::to_value(&run.state).unwrap();
            let want: Value = serde_json::from_str(&fixture(&name, "state.json")).unwrap();
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn tone_host_and_rows_come_from_the_events() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral","host":{"poimen.terminal":"t7","herdr.pane":"w1:p2","n":3},"agent":{"harness":"claude"}}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"task.started","tone":"info","task":"t","subject":{"kind":"x","id":"1","title":"T"}}
{"v":1,"seq":3,"ts":"2026-11-08T11:02:00Z","run":"r","type":"step.started","tone":"info","task":"t","step":{"name":"implement","text":"green"}}
{"v":1,"seq":4,"ts":"2026-11-08T11:03:00Z","run":"r","type":"deliver.new","tone":"danger"}
{"v":1,"seq":5,"ts":"2026-11-08T11:04:00Z","run":"r","type":"attention","tone":"warning","id":"a","reason":"CI","needsYou":true}
"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.tone, "warning");
        assert!(run.state.needs_you);
        assert_eq!(run.host["poimen.terminal"], "t7");
        assert_eq!(run.host.len(), 2);
        assert_eq!(run.agent.unwrap()["harness"], "claude");
        let kinds: Vec<&str> = run.events.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["run.started", "task.started", "step.started", "attention"]
        );
        assert_eq!(run.events[2].step.as_deref(), Some("implement: green"));
        assert_eq!(run.events[2].task.as_deref(), Some("t"));
    }

    #[test]
    fn the_run_tone_is_the_tone_of_run_finished() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"run.finished","tone":"danger","outcome":"failed","reason":"boom"}
{"v":1,"seq":3,"ts":"2026-11-08T11:02:00Z","run":"r","type":"progress","tone":"success","done":1,"total":2}
"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.tone, "danger");
        assert_eq!(run.state.status, "finished");
        assert_eq!(run.state.reason.as_deref(), Some("boom"));
        assert!(run.state.progress.is_none());
        assert_eq!(run.state.last_seq, 3);
    }

    #[test]
    fn a_dead_writer_pid_abandons_a_fresh_run_without_run_finished() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"info","host":{"jsonl-file.pid":"4242"}}
"#;
        let alive = reduce(text, now(), &|p| p == 4242).unwrap();
        assert_eq!(alive.state.status, "running");
        let dead = reduce(text, now(), &|_| false).unwrap();
        assert_eq!(dead.state.status, "abandoned");
        assert_eq!(dead.tone, "neutral");
    }

    #[test]
    fn events_of_another_version_are_skipped_and_reported() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":2,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"run.finished","tone":"success","outcome":"succeeded"}
"#;
        let mut reducer = Reducer::new();
        for line in text.lines() {
            reducer.push_line(line);
        }
        assert_eq!(reducer.unknown_v, Some(2));
        let run = reducer.snapshot(now(), &|_| true).unwrap();
        assert_eq!(run.state.status, "running");
        let mut only = Reducer::new();
        only.push_line(r#"{"v":9,"type":"run.started"}"#);
        assert!(only.is_empty());
        assert_eq!(only.unknown_v, Some(9));
    }

    #[test]
    fn unknown_fields_and_lines_that_are_not_json_are_ignored() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"neutral","future":{"a":1}}
not json
{"v":1,"seq":2,"ts":"2026-11-08T11:59:10Z","run":"r","type":"something.new","tone":"danger"}
"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.state.status, "running");
        assert_eq!(run.tone, "neutral");
        assert_eq!(run.state.last_seq, 2);
    }

    #[test]
    fn an_empty_or_incomplete_file_has_no_run() {
        assert!(reduce("", now(), &|_| true).is_none());
        assert!(reduce("{\"v\":1,\"ty", now(), &|_| true).is_none());
        let whole_but_unterminated = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"neutral"}"#;
        assert!(reduce(whole_but_unterminated, now(), &|_| true).is_none());
    }

    #[test]
    fn timestamps_parse_with_fraction_and_offset() {
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_ts("1970-01-01T01:00:00+01:00"), Some(0.0));
        assert_eq!(parse_ts("1970-01-01T00:00:01.5Z"), Some(1.5));
        assert_eq!(parse_ts("2026-11-07T12:00:00Z"), Some(1_794_052_800.0));
        for bad in [
            "",
            "2026-11-07",
            "2026-11-07T12:00:00",
            "2026-13-07T12:00:00Z",
        ] {
            assert_eq!(parse_ts(bad), None, "{bad}");
        }
    }
}
