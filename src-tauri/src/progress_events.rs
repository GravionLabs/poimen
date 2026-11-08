//! The reducer of the deliver progress events: folds the lines of one `<run>.jsonl` (schema
//! `progress-event.schema.json` of GravionLabs/skills, ADR 0001 "Agent progress events replace
//! status.json") into the run state of `deliver-run-state.schema.json`. It follows the reference
//! reducer `ConvertFrom-ProgressEvents.ps1` and passes its paired fixtures
//! (`tests/fixtures/progress-events/`, commit in `SOURCE.txt`). Pure: the clock and the pid check
//! are parameters. Unknown types and fields are ignored, an incomplete last line is skipped, a
//! gap in `seq` is counted and folded anyway, events of another `v` are skipped and reported.

use serde::Serialize;
use serde_json::Value;

/// The event version this build understands.
pub const SUPPORTED_V: i64 = 1;

/// Without `run.finished`, a run whose last event is older than this is abandoned.
const ABANDONED_AFTER_SECS: f64 = 3600.0;

const KNOWN: [&str; 8] = [
    "run.started",
    "milestone.progress",
    "issue.started",
    "step.finished",
    "attention",
    "issue.finished",
    "release.updated",
    "run.finished",
];
const RESULTS: [&str; 4] = ["done", "max reached", "blocked", "no progress"];
const TONES: [&str; 5] = ["neutral", "info", "success", "warning", "danger"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Milestone {
    pub key: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub n: i64,
    pub total: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Current {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub step: Step,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivered {
    pub pbi: u64,
    pub url: String,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub number: u64,
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestIssue {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub review_round: Option<i64>,
    pub pr_url: Option<String>,
    pub rc_url: Option<String>,
    pub rc_version: Option<String>,
    pub test_issue: Option<TestIssue>,
}

/// The run state of `deliver-run-state.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub run: String,
    pub harness: Option<String>,
    pub model: String,
    pub repository: Option<String>,
    pub milestone: Option<Milestone>,
    pub closed: i64,
    pub total: i64,
    pub current: Option<Current>,
    pub delivered: Vec<Delivered>,
    pub blocked: Vec<Item>,
    pub paused: Vec<Item>,
    pub needs_you: Vec<Item>,
    pub release: Release,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub result: String,
}

/// A run for the panel: the state plus what the emitter said about it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    #[serde(flatten)]
    pub state: RunState,
    /// `loop` or `session` (`run.started`); `None` when the event did not say.
    pub source: Option<String>,
    /// `tone` of the run: of `run.finished` once it ended, else of the last event.
    pub tone: String,
    /// Where the run lives (`run.started` `host`): the poimen terminal and the herdr pane.
    pub host: Host,
    /// One row per event of a known type, in file order.
    pub events: Vec<RunEvent>,
}

/// The `host` of `run.started`; the ids that tie a run to a terminal.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub poimen_terminal: Option<String>,
    pub herdr_pane: Option<String>,
}

/// One progress event as the Events tab shows it (harness-neutral).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub seq: Option<i64>,
    pub ts: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub tone: String,
    pub issue: Option<u64>,
    pub step: Option<String>,
    pub result: Option<String>,
    pub pr_url: Option<String>,
}

/// Insertion-ordered map by issue number; an existing key keeps its place, like an ordered
/// hashtable.
#[derive(Debug, Clone)]
struct Ordered<T>(Vec<(u64, T)>);

impl<T> Ordered<T> {
    fn set(&mut self, key: u64, value: T) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }
    fn get(&self, key: u64) -> Option<&T> {
        self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
    fn remove(&mut self, key: u64) {
        self.0.retain(|(k, _)| *k != key);
    }
    fn values(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.0.iter().map(|(_, v)| v.clone()).collect()
    }
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

fn int(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn step_number(name: &str) -> Option<i64> {
    Some(match name {
        "select" => 1,
        "resume" => 2,
        "implement" => 3,
        "review" => 4,
        "fix" => 5,
        "pr" => 6,
        "ci" | "merge" | "merged" | "cleanup" => 7,
        _ => return None,
    })
}

/// Folds events one by one; `snapshot` gives the state at any point, so a follower feeds new
/// lines only.
#[derive(Debug, Clone)]
pub struct Reducer {
    events: usize,
    prev_seq: Option<i64>,
    /// Gaps seen in `seq` (folded anyway).
    pub seq_gaps: u32,
    /// The first `v` this build does not understand; its events are skipped.
    pub unknown_v: Option<i64>,
    run: Option<String>,
    harness: Option<String>,
    model: Option<String>,
    repository: Option<String>,
    milestone: Option<Milestone>,
    source: Option<String>,
    closed: i64,
    total: i64,
    current: Option<Current>,
    started_at: Option<String>,
    updated_at: Option<String>,
    result: Option<String>,
    pid: Option<u32>,
    last_ts: Option<f64>,
    delivered: Ordered<Delivered>,
    blocked: Ordered<Item>,
    paused: Ordered<Item>,
    needs: Ordered<Item>,
    release: Release,
    titles: Ordered<String>,
    urls: Ordered<String>,
    prs: Ordered<String>,
    tone: String,
    host: Host,
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
            prev_seq: None,
            seq_gaps: 0,
            unknown_v: None,
            run: None,
            harness: None,
            model: None,
            repository: None,
            milestone: None,
            source: None,
            closed: 0,
            total: 0,
            current: None,
            started_at: None,
            updated_at: None,
            result: None,
            pid: None,
            last_ts: None,
            delivered: Ordered(Vec::new()),
            blocked: Ordered(Vec::new()),
            paused: Ordered(Vec::new()),
            needs: Ordered(Vec::new()),
            release: Release::default(),
            titles: Ordered(Vec::new()),
            urls: Ordered(Vec::new()),
            prs: Ordered(Vec::new()),
            tone: "neutral".into(),
            host: Host::default(),
            rows: Vec::new(),
        }
    }

    /// Whether any event of a known type was folded.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.events == 0
    }

    fn issue_url(&self, n: u64) -> String {
        if let Some(url) = self.urls.get(n) {
            return url.clone();
        }
        match &self.repository {
            Some(repo) => format!("https://github.com/{repo}/issues/{n}"),
            None => format!("#{n}"),
        }
    }

    fn new_current(&self, n: u64) -> Current {
        Current {
            number: n,
            title: self.titles.get(n).cloned().unwrap_or(format!("#{n}")),
            url: self.issue_url(n),
            step: Step {
                n: 1,
                total: 7,
                text: "select".into(),
            },
            pr_url: self.prs.get(n).cloned(),
        }
    }

    fn item(&self, n: u64, reason: &str) -> Item {
        Item {
            number: n,
            url: self.issue_url(n),
            reason: reason.to_owned(),
        }
    }

    fn set_step(cur: &mut Current, step: &Value) {
        let Some(name) = step.get("name").and_then(text) else {
            return;
        };
        let n = step
            .get("n")
            .and_then(int)
            .or_else(|| step_number(&name))
            .unwrap_or(cur.step.n);
        cur.step = Step {
            n: n.clamp(1, 7),
            total: 7,
            text: match step.get("text").and_then(text) {
                Some(t) => format!("{name}: {t}"),
                None => name,
            },
        };
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

    pub fn push(&mut self, e: &Value) {
        let v = e.get("v").and_then(Value::as_i64);
        if v != Some(SUPPORTED_V) {
            if self.unknown_v.is_none() && e.get("type").is_some() {
                self.unknown_v = Some(v.unwrap_or(0));
            }
            return;
        }
        if let Some(seq) = e.get("seq").and_then(int) {
            let expected = self.prev_seq.map_or(1, |p| p + 1);
            if seq != expected {
                self.seq_gaps += 1;
            }
            self.prev_seq = Some(seq);
        }
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
        if !KNOWN.contains(&ty) {
            return;
        }
        self.events += 1;
        if self.run.is_none() {
            self.run = e.get("run").and_then(text);
        }
        let ts = e.get("ts").and_then(Value::as_str);
        if let Some((ts, secs)) = ts.and_then(|t| parse_ts(t).map(|s| (t, s))) {
            self.updated_at = Some(ts.to_owned());
            self.started_at.get_or_insert_with(|| ts.to_owned());
            self.last_ts = Some(secs);
        }
        let tone = e
            .get("tone")
            .and_then(Value::as_str)
            .filter(|t| TONES.contains(t))
            .unwrap_or("neutral");
        let issue = e.get("issue").filter(|i| i.is_object());
        let nr = issue
            .and_then(|i| i.get("number"))
            .and_then(Value::as_u64)
            .filter(|n| *n >= 1);
        let pr_url = e.get("prUrl").and_then(text);
        if let (Some(n), Some(issue)) = (nr, issue) {
            if let Some(title) = issue.get("title").and_then(text) {
                self.titles.set(n, title);
            }
            if let Some(url) = issue.get("url").and_then(text) {
                self.urls.set(n, url);
            }
            if let Some(url) = &pr_url
                && ty != "release.updated"
            {
                self.prs.set(n, url.clone());
            }
        }
        let is_current = nr.is_some() && self.current.as_ref().map(|c| c.number) == nr;
        self.tone = tone.to_owned();
        let step = e.get("step").filter(|s| s.is_object()).and_then(|s| {
            let name = s.get("name").and_then(text)?;
            Some(match s.get("text").and_then(text) {
                Some(t) => format!("{name}: {t}"),
                None => name,
            })
        });
        let result = ["result", "outcome"]
            .iter()
            .find_map(|k| e.get(*k).and_then(text));
        self.rows.push(RunEvent {
            seq: e.get("seq").and_then(int),
            ts: ts.map(str::to_owned),
            kind: ty.to_owned(),
            tone: tone.to_owned(),
            issue: nr,
            step,
            result,
            pr_url: pr_url.clone(),
        });

        match ty {
            "run.started" => {
                for (key, slot) in [
                    ("harness", &mut self.harness),
                    ("model", &mut self.model),
                    ("repository", &mut self.repository),
                    ("source", &mut self.source),
                ] {
                    if let Some(s) = e.get(key).and_then(text) {
                        *slot = Some(s);
                    }
                }
                if let Some(h) = e.get("host").filter(|h| h.is_object()) {
                    self.host = Host {
                        poimen_terminal: h.get("poimenTerminal").and_then(text),
                        herdr_pane: h.get("herdrPane").and_then(text),
                    };
                }
                if let Some(m) = e.get("milestone").filter(|m| m.is_object()) {
                    let field = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                    self.milestone = Some(Milestone {
                        key: field("key"),
                        title: field("title"),
                        url: field("url"),
                    });
                }
                self.pid = e
                    .get("pid")
                    .and_then(Value::as_u64)
                    .and_then(|p| u32::try_from(p).ok())
                    .or(self.pid);
                if let Some(ts) = ts.filter(|t| parse_ts(t).is_some()) {
                    self.started_at = Some(ts.to_owned());
                }
            }
            "milestone.progress" => {
                if let Some(c) = e.get("closed").and_then(int) {
                    self.closed = c;
                }
                if let Some(t) = e.get("total").and_then(int) {
                    self.total = t;
                }
            }
            "issue.started" => {
                if let Some(n) = nr {
                    let mut cur = self.new_current(n);
                    if let Some(step) = e.get("step") {
                        Self::set_step(&mut cur, step);
                    }
                    self.current = Some(cur);
                }
            }
            "step.finished" => {
                if let Some(n) = nr
                    && !is_current
                {
                    self.current = Some(self.new_current(n));
                }
                if let Some(cur) = self.current.as_mut() {
                    if let Some(step) = e.get("step") {
                        Self::set_step(cur, step);
                    }
                    if let Some(url) = &pr_url {
                        cur.pr_url = Some(url.clone());
                    }
                }
            }
            "attention" => {
                if is_current && let (Some(cur), Some(url)) = (self.current.as_mut(), &pr_url) {
                    cur.pr_url = Some(url.clone());
                }
                if let Some(n) = nr
                    && e.get("needsYou") == Some(&Value::Bool(true))
                {
                    let reason = if e.get("kind").and_then(Value::as_str) == Some("release_test") {
                        "RC test".to_owned()
                    } else {
                        e.get("reason")
                            .and_then(text)
                            .unwrap_or_else(|| "needs you".into())
                    };
                    let item = self.item(n, &reason);
                    self.needs.set(n, item);
                }
            }
            "issue.finished" => {
                let Some(n) = nr else { return };
                if self.needs.get(n).is_some_and(|i| i.reason == "RC test") {
                    self.needs.remove(n);
                }
                let reason = |default: &str| {
                    e.get("reason")
                        .and_then(text)
                        .unwrap_or_else(|| default.into())
                };
                match e.get("outcome").and_then(Value::as_str).unwrap_or("") {
                    "delivered" => {
                        let url = self.issue_url(n);
                        let pr_url = self.prs.get(n).cloned();
                        self.delivered.set(
                            n,
                            Delivered {
                                pbi: n,
                                url,
                                pr_url,
                            },
                        );
                        self.blocked.remove(n);
                        self.paused.remove(n);
                        self.needs.remove(n);
                    }
                    "skipped" => {
                        self.blocked.remove(n);
                        self.paused.remove(n);
                        self.needs.remove(n);
                    }
                    "blocked" => {
                        self.paused.remove(n);
                        let item = self.item(n, &reason("blocked"));
                        self.blocked.set(n, item);
                    }
                    "paused" => {
                        self.blocked.remove(n);
                        let item = self.item(n, &reason("paused"));
                        self.paused.set(n, item);
                    }
                    _ => {}
                }
                if is_current {
                    self.current = None;
                }
            }
            "release.updated" => {
                for (key, slot) in [
                    ("prUrl", &mut self.release.pr_url),
                    ("rcUrl", &mut self.release.rc_url),
                    ("rcVersion", &mut self.release.rc_version),
                ] {
                    if let Some(s) = e.get(key).and_then(text) {
                        *slot = Some(s);
                    }
                }
                if let Some(r) = e.get("reviewRound").and_then(int).filter(|r| *r >= 1) {
                    self.release.review_round = Some(r);
                }
                let test = e.get("testIssue").filter(|t| t.is_object());
                if let Some(t) = test
                    .and_then(|t| t.get("number"))
                    .and_then(Value::as_u64)
                    .filter(|t| *t >= 1)
                {
                    if let Some(url) = test.and_then(|t| t.get("url")).and_then(text) {
                        self.urls.set(t, url);
                    }
                    if let Some(old) = self.release.test_issue.as_ref().map(|o| o.number)
                        && old != t
                        && self.needs.get(old).is_some_and(|i| i.reason == "RC test")
                    {
                        self.needs.remove(old);
                    }
                    self.release.test_issue = Some(TestIssue {
                        number: t,
                        url: self.issue_url(t),
                    });
                    let item = self.item(t, "RC test");
                    self.needs.set(t, item);
                }
            }
            "run.finished" => {
                let result = e.get("result").and_then(Value::as_str).unwrap_or("");
                self.result = Some(if RESULTS.contains(&result) {
                    result.to_owned()
                } else {
                    "done".into()
                });
            }
            _ => {}
        }
    }

    /// The run as of `now` (epoch seconds): `None` before the first event. `pid_alive` is asked
    /// for a run without `run.finished` that is not older than an hour.
    pub fn snapshot(&self, now: f64, pid_alive: &dyn Fn(u32) -> bool) -> Option<Run> {
        if self.events == 0 {
            return None;
        }
        let result = match &self.result {
            Some(r) => r.clone(),
            None => {
                let old = self.last_ts.is_some_and(|t| now - t > ABANDONED_AFTER_SECS);
                let dead = self.pid.is_some_and(|p| !pid_alive(p));
                if old || dead {
                    "abandoned".into()
                } else {
                    "running".into()
                }
            }
        };
        let finished = result != "running";
        let state = RunState {
            run: self.run.clone().unwrap_or_default(),
            harness: self.harness.clone(),
            model: self.model.clone().unwrap_or_else(|| "default".into()),
            repository: self.repository.clone(),
            milestone: self.milestone.clone(),
            closed: self.closed,
            total: self.total,
            current: if finished { None } else { self.current.clone() },
            delivered: self.delivered.values(),
            blocked: self.blocked.values(),
            paused: self.paused.values(),
            needs_you: self.needs.values(),
            release: self.release.clone(),
            started_at: self.started_at.clone(),
            updated_at: self.updated_at.clone(),
            result: result.clone(),
        };
        let tone = if result == "abandoned" {
            "neutral".to_owned()
        } else {
            self.tone.clone()
        };
        Some(Run {
            state,
            source: self.source.clone(),
            tone,
            host: self.host.clone(),
            events: self.rows.clone(),
        })
    }
}

/// Folds the whole text of an event file.
#[cfg(test)]
pub fn reduce(text: &str, now: f64, pid_alive: &dyn Fn(u32) -> bool) -> Option<Run> {
    let mut reducer = Reducer::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
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

    const DIR: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/progress-events"
    );

    fn now() -> f64 {
        parse_ts("2026-11-07T12:00:00Z").unwrap()
    }

    fn fixture(name: &str, ext: &str) -> String {
        std::fs::read_to_string(format!("{DIR}/{name}.{ext}")).unwrap()
    }

    /// Every pair of GravionLabs/skills: events in, run state out (`-Now 12:00`, no pid check).
    #[test]
    fn every_fixture_pair_of_the_skills_reduces_to_its_state() {
        let mut names: Vec<String> = std::fs::read_dir(DIR)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".jsonl").map(str::to_owned)
            })
            .collect();
        names.sort();
        assert!(names.len() >= 9, "{names:?}");
        for name in names {
            let run = reduce(&fixture(&name, "jsonl"), now(), &|_| true)
                .unwrap_or_else(|| panic!("{name}: no run"));
            let got = serde_json::to_value(&run.state).unwrap();
            let want: Value = serde_json::from_str(&fixture(&name, "state.json")).unwrap();
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn tone_and_source_come_from_the_events() {
        let blocked = reduce(&fixture("blocked", "jsonl"), now(), &|_| true).unwrap();
        assert_eq!(blocked.tone, "danger");
        assert_eq!(blocked.source.as_deref(), Some("loop"));
        let done = reduce(&fixture("done", "jsonl"), now(), &|_| true).unwrap();
        assert_eq!(done.tone, "success");
        let running = reduce(&fixture("running", "jsonl"), now(), &|_| true).unwrap();
        assert_eq!(running.state.result, "running");
        let abandoned = reduce(&fixture("abandoned", "jsonl"), now(), &|_| true).unwrap();
        assert_eq!(abandoned.tone, "neutral");
    }

    #[test]
    fn rows_and_host_come_from_the_events() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-07T10:00:00Z","run":"r","type":"run.started","tone":"neutral","host":{"poimenTerminal":"t7","herdrPane":"w1:p2"}}
{"v":1,"seq":2,"ts":"2026-11-07T10:01:00Z","run":"r","type":"step.finished","tone":"info","issue":{"number":5,"title":"T","url":"u"},"step":{"name":"implement","text":"green"},"prUrl":"https://github.com/o/r/pull/9"}
{"v":1,"seq":3,"ts":"2026-11-07T10:02:00Z","run":"r","type":"something.new","tone":"danger"}
{"v":1,"seq":4,"ts":"2026-11-07T10:03:00Z","run":"r","type":"attention","tone":"warning","issue":{"number":5}}"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.host.poimen_terminal.as_deref(), Some("t7"));
        assert_eq!(run.host.herdr_pane.as_deref(), Some("w1:p2"));
        let kinds: Vec<&str> = run.events.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["run.started", "step.finished", "attention"]);
        assert_eq!(run.events[1].step.as_deref(), Some("implement: green"));
        assert_eq!(run.events[1].issue, Some(5));
        assert_eq!(run.events[1].tone, "info");
        assert_eq!(
            run.events[1].pr_url.as_deref(),
            Some("https://github.com/o/r/pull/9")
        );
        assert_eq!(run.events[2].tone, "warning");
    }

    #[test]
    fn a_delivered_entry_without_a_pull_request_event_has_no_pr_url() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-07T10:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":1,"seq":2,"ts":"2026-11-07T10:01:00Z","run":"r","type":"issue.finished","tone":"success","issue":{"number":7,"title":"T","url":"https://github.com/o/r/issues/7"},"outcome":"delivered"}"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.state.delivered.len(), 1);
        assert_eq!(
            run.state.delivered[0].url,
            "https://github.com/o/r/issues/7"
        );
        assert_eq!(run.state.delivered[0].pr_url, None);
        let json = serde_json::to_value(&run.state.delivered[0]).unwrap();
        assert!(json["prUrl"].is_null());
    }

    #[test]
    fn the_rc_test_is_needs_you_and_does_not_block_the_run() {
        let run = reduce(&fixture("rc-test-open", "jsonl"), now(), &|_| true).unwrap();
        assert_eq!(run.state.release.rc_version.as_deref(), Some("3.2.0-rc.1"));
        assert_eq!(run.state.needs_you[0].reason, "RC test");
        assert!(run.state.blocked.is_empty());
        assert_eq!(run.state.result, "done");
        assert_eq!(run.tone, "success");
    }

    #[test]
    fn a_dead_pid_abandons_a_fresh_run_without_run_finished() {
        let fresh = parse_ts("2026-11-07T11:31:00Z").unwrap();
        let text = fixture("running", "jsonl");
        assert_eq!(
            reduce(&text, fresh, &|_| true).unwrap().state.result,
            "running"
        );
        let dead = reduce(&text, fresh, &|_| false).unwrap();
        assert_eq!(dead.state.result, "abandoned");
        assert!(dead.state.current.is_none());
    }

    #[test]
    fn events_of_another_version_are_skipped_and_reported() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-07T10:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":2,"seq":2,"ts":"2026-11-07T10:01:00Z","run":"r","type":"run.finished","tone":"success","result":"done"}
"#;
        let mut reducer = Reducer::new();
        for line in text.lines() {
            reducer.push_line(line);
        }
        assert_eq!(reducer.unknown_v, Some(2));
        let run = reducer.snapshot(now(), &|_| true).unwrap();
        assert_eq!(
            run.state.updated_at.as_deref(),
            Some("2026-11-07T10:00:00Z")
        );
        // Only unknown versions: nothing to show.
        let mut only = Reducer::new();
        only.push_line(r#"{"v":9,"type":"run.started"}"#);
        assert!(only.is_empty());
        assert_eq!(only.unknown_v, Some(9));
    }

    #[test]
    fn unknown_types_and_fields_are_ignored() {
        let text = r#"{"v":1,"seq":1,"ts":"2026-11-07T11:59:00Z","run":"r","type":"run.started","tone":"neutral","future":{"a":1},"pid":1}
{"v":1,"seq":2,"ts":"2026-11-07T11:59:10Z","run":"r","type":"something.new","tone":"danger"}
not json
"#;
        let run = reduce(text, now(), &|_| true).unwrap();
        assert_eq!(run.state.result, "running");
        assert_eq!(run.tone, "neutral");
        assert_eq!(
            run.state.updated_at.as_deref(),
            Some("2026-11-07T11:59:00Z")
        );
    }

    #[test]
    fn a_seq_gap_is_counted_and_folded() {
        let mut reducer = Reducer::new();
        reducer.push_line(r#"{"v":1,"seq":1,"ts":"2026-11-07T11:59:00Z","run":"r","type":"milestone.progress","closed":1,"total":3}"#);
        reducer.push_line(r#"{"v":1,"seq":4,"ts":"2026-11-07T11:59:30Z","run":"r","type":"milestone.progress","closed":2,"total":3}"#);
        assert_eq!(reducer.seq_gaps, 1);
        assert_eq!(reducer.snapshot(now(), &|_| true).unwrap().state.closed, 2);
    }

    #[test]
    fn an_empty_or_unreadable_file_has_no_run() {
        assert!(reduce("", now(), &|_| true).is_none());
        assert!(reduce("{\"v\":1,\"ty", now(), &|_| true).is_none());
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
