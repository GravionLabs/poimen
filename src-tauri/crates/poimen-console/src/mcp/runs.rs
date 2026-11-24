//! The run tools of `poimen mcp`: `list_runs` and `get_events` (#1142), read-only over
//! `<common-dir>/.agents/events/*.jsonl`.

use super::{Fail, text_arg};
use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Reducer};
use poimen_core::git;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Rejects an argument the tool does not know.
pub(super) fn only_keys(args: &Value, known: &[&str]) -> Result<(), Fail> {
    let Some(map) = args.as_object() else {
        return Ok(());
    };
    match map.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(Fail::Tool(format!("unknown parameter `{k}`"))),
        None => Ok(()),
    }
}

pub(super) fn repository_arg(args: &Value) -> Result<&str, Fail> {
    args.get("repository")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| Fail::Tool("missing argument `repository`".into()))
}

/// An optional positive integer argument.
fn count_arg(args: &Value, key: &str, min: u64) -> Result<Option<u64>, Fail> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= min)
            .map(Some)
            .ok_or_else(|| Fail::Tool(format!("`{key}` must be an integer of at least {min}"))),
    }
}

const RUNS_DEFAULT_LIMIT: usize = 20;
const EVENTS_DEFAULT_LIMIT: usize = 100;
const EVENTS_MAX_LIMIT: usize = 1000;

/// What `list_runs` keeps.
pub(super) struct RunFilter {
    status: Option<String>,
    limit: usize,
    compact: bool,
}

impl RunFilter {
    pub(super) fn parse(args: &Value) -> Result<Self, Fail> {
        let status = match args.get("status") {
            None | Some(Value::Null) => None,
            Some(Value::String(s))
                if ["running", "finished", "needs-you"].contains(&s.as_str()) =>
            {
                Some(s.clone())
            }
            Some(_) => {
                return Err(Fail::Tool(
                    "`status` must be `running`, `finished` or `needs-you`".into(),
                ));
            }
        };
        let compact = match args.get("compact") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(Fail::Tool("`compact` must be a boolean".into())),
        };
        let limit = count_arg(args, "limit", 1)?.map_or(RUNS_DEFAULT_LIMIT, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        });
        Ok(Self {
            status,
            limit,
            compact,
        })
    }

    fn keeps(&self, run: &Value) -> bool {
        match self.status.as_deref() {
            None => true,
            Some("needs-you") => run["needsYou"] == json!(true),
            Some(s) => run["status"] == json!(s),
        }
    }
}

/// The directory of the event files of the repository containing `dir`; a directory outside a
/// repository is an error.
pub(super) fn events_dir(dir: &str) -> Result<PathBuf, String> {
    if !Path::new(dir).is_dir() {
        return Err(format!("`{dir}` is not a directory"));
    }
    let (_, common) =
        git::roots(dir).ok_or_else(|| format!("`{dir}` is not inside a git repository"))?;
    Ok(Path::new(&common).join(agent_events::EVENTS_PATH))
}

/// The complete lines of an event file (a half-written last line waits); `None` for a file that
/// is not regular, too big or unreadable.
fn read_complete(path: &Path) -> Option<String> {
    let regular = std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_EVENTS_BYTES);
    let text = regular
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()?;
    let end = text.rfind('\n').map_or(0, |i| i + 1);
    Some(text[..end].to_owned())
}

/// `get_events {repository, run, after?, type?, limit?}`: the events of one run with `seq >
/// after`, in file order, as written.
pub(super) fn get_events(args: &Value) -> Result<Value, Fail> {
    let dir = repository_arg(args)?;
    let run = text_arg(args, "run").ok_or_else(|| Fail::Tool("missing argument `run`".into()))?;
    let after = count_arg(args, "after", 0)?.map_or(0, |n| i64::try_from(n).unwrap_or(i64::MAX));
    let limit = count_arg(args, "limit", 1)?.map_or(EVENTS_DEFAULT_LIMIT, |n| {
        usize::try_from(n)
            .unwrap_or(usize::MAX)
            .min(EVENTS_MAX_LIMIT)
    });
    let kind = match args.get("type") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => return Err(Fail::Tool("`type` must be a string".into())),
    };
    let events = events_dir(dir).map_err(Fail::Tool)?;
    let path = events.join(format!("{run}.jsonl"));
    // The run id names a file: a separator or a dot segment never leaves the events directory.
    let known = !run.contains(['/', '\\']) && agent_events::run_of(&path) == Some(run);
    let text = known
        .then(|| read_complete(&path))
        .flatten()
        .ok_or_else(|| Fail::Tool(format!("unknown run `{run}` in `{dir}`")))?;
    let mut found = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e["seq"].as_i64().is_some_and(|s| s > after))
        .filter(|e| kind.is_none_or(|k| e["type"] == json!(k)));
    let list: Vec<Value> = found.by_ref().take(limit).collect();
    let more = found.next().is_some();
    Ok(json!({"repository": dir, "run": run, "events": list, "more": more}))
}

/// The runs of the repository containing `dir`, from `<common-dir>/.agents/events/*.jsonl`
/// folded by the core reducer (the same one the app uses), filtered, newest first (by the last
/// event, then the run id) and cut to the limit. No events directory is an empty list; a
/// directory outside a repository is an error.
pub(super) fn runs_of(dir: &str, filter: &RunFilter) -> Result<Vec<Value>, String> {
    let events = events_dir(dir)?;
    let Ok(entries) = std::fs::read_dir(&events) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| agent_events::is_run_file(p))
        .collect();
    files.sort();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mut runs: Vec<(f64, Value)> = Vec::new();
    for path in files {
        let Some(complete) = read_complete(&path) else {
            continue;
        };
        let mut reducer = Reducer::new();
        for line in complete.lines().filter(|l| !l.trim().is_empty()) {
            reducer.push_line(line);
        }
        if let Some(run) = reducer.snapshot(now, &agent_events::pid_alive) {
            let mut v = serde_json::to_value(&run).map_err(|e| e.to_string())?;
            if let Some(o) = v.as_object_mut() {
                o.remove("events");
                if filter.compact {
                    o.remove("finished");
                    o.remove("links");
                }
            }
            if filter.keeps(&v) {
                let at = v["updated"]
                    .as_str()
                    .and_then(agent_events::parse_ts)
                    .unwrap_or(0.0);
                runs.push((at, v));
            }
        }
    }
    // Newest first; the stable sort keeps the later file name first on a tie.
    runs.reverse();
    runs.sort_by(|a, b| b.0.total_cmp(&a.0));
    runs.truncate(filter.limit);
    Ok(runs.into_iter().map(|(_, v)| v).collect())
}
