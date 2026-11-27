//! The run tools of `poimen mcp`: `list_runs` and `get_events` (#1142), read-only over the
//! per-user event directory and the legacy directory of a repository (`run_files`, #1417).

use super::{Fail, run_files, text_arg};
use poimen_core::agent_events;
use serde_json::{Value, json};
use std::path::Path;

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

/// An optional non-empty text argument.
pub(super) fn optional_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, Fail> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s)),
        Some(_) => Err(Fail::Tool(format!("`{key}` must be a non-empty string"))),
    }
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

/// `get_events {run, repository?, after?, type?, limit?}`: the events of one run with `seq >
/// after`, as written.
pub(super) fn get_events(shared: Option<&Path>, args: &Value) -> Result<Value, Fail> {
    only_keys(args, &["repository", "run", "after", "type", "limit"])?;
    let dir = optional_text(args, "repository")?;
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
    let repo = dir
        .map(run_files::repository)
        .transpose()
        .map_err(Fail::Tool)?;
    if repo.is_none() && shared.is_none() {
        return Err(Fail::Tool(run_files::NO_SHARED.into()));
    }
    let legacy = repo.as_ref().map(|r| agent_events::legacy_dir(&r.common));
    let files = run_files::files_of(
        shared,
        legacy.as_deref(),
        repo.as_ref(),
        run,
        &mut run_files::Attribution::default(),
    );
    if files.is_empty() {
        return Err(unknown_run(shared, dir, run));
    }
    let mut found = run_files::events(&files)
        .into_iter()
        .filter(|e| e["seq"].as_i64().is_some_and(|s| s > after))
        .filter(|e| kind.is_none_or(|k| e["type"] == json!(k)));
    let list: Vec<Value> = found.by_ref().take(limit).collect();
    let more = found.next().is_some();
    Ok(json!({"repository": dir, "run": run, "events": list, "more": more}))
}

/// The error for a run id that no read directory has; it names them.
pub(super) fn unknown_run(shared: Option<&Path>, repository: Option<&str>, run: &str) -> Fail {
    let shared = shared.map(|d| format!("`{}`", d.display()));
    let places: Vec<String> = shared
        .into_iter()
        .chain(repository.map(|r| format!("`{r}`")))
        .collect();
    Fail::Tool(format!("unknown run `{run}` in {}", places.join(" or ")))
}

/// `list_runs {repository?, repo?, status?, limit?, compact?}`: the runs folded by the core
/// reducer (the same one the app uses), filtered, newest first (by the last event, then the run
/// id) and cut to the limit. Without `repository` the runs of the shared directory, with it those
/// of that repository, from the shared and its legacy directory.
pub(super) fn list_runs(shared: Option<&Path>, args: &Value) -> Result<Value, Fail> {
    only_keys(args, &["repository", "repo", "status", "limit", "compact"])?;
    let dir = optional_text(args, "repository")?;
    let name = optional_text(args, "repo")?;
    let filter = RunFilter::parse(args)?;
    let repo = dir
        .map(run_files::repository)
        .transpose()
        .map_err(Fail::Tool)?;
    let found = match &repo {
        Some(repo) => {
            run_files::of_repository(shared, repo, &mut run_files::Attribution::default())
        }
        None => {
            let shared = shared.ok_or_else(|| Fail::Tool(run_files::NO_SHARED.into()))?;
            run_files::list(shared)
                .into_iter()
                // Another repository's run is not read when its first line says so.
                .filter(|(_, path)| {
                    name.is_none_or(|n| {
                        run_files::origin_of(path).is_some_and(|o| o.repo.as_deref() == Some(n))
                    })
                })
                .map(|(run, path)| (run, vec![path]))
                .collect()
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mut runs: Vec<(f64, Value)> = Vec::new();
    for files in found.into_values() {
        let reducer = run_files::fold(&run_files::events(&files));
        let Some(run) = reducer.snapshot(now, &agent_events::pid_alive) else {
            continue;
        };
        if let Some(name) = name {
            // A run without origin is the repository's whose legacy directory lists it.
            let of = match &run.origin {
                Some(origin) => origin.repo.as_deref(),
                None => repo.as_ref().map(|r| r.name.as_str()),
            };
            if of != Some(name) {
                continue;
            }
        }
        let mut v = serde_json::to_value(&run)?;
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
    // Newest first; the stable sort keeps the later run id first on a tie.
    runs.reverse();
    runs.sort_by(|a, b| b.0.total_cmp(&a.0));
    runs.truncate(filter.limit);
    let runs: Vec<Value> = runs.into_iter().map(|(_, v)| v).collect();
    Ok(json!({"repository": dir, "runs": runs}))
}
