//! Where the run files of the MCP tools are (#1417, jsonl-file binding of agent-events 0.3): the
//! per-user shared directory ([`agent_events::shared_dir`]) and, for one minor version, the legacy
//! `<common-dir>/.agents/events` of a repository (remove from 0.4). A run in both is one run: its
//! events are merged by `(run, seq)`, the shared file first.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Origin, Reducer, RepoRef};
use poimen_core::gui_path;
use serde_json::Value;

/// The error of a tool that needs the shared directory when there is none.
pub(super) const NO_SHARED: &str =
    "there is no event directory: set AGENT_EVENTS_DIR or the home directory";

/// The repository containing `dir`; a directory outside one is an error.
pub(super) fn repository(dir: &str) -> Result<RepoRef, String> {
    if !Path::new(dir).is_dir() {
        return Err(format!("`{dir}` is not a directory"));
    }
    RepoRef::of(dir).ok_or_else(|| format!("`{dir}` is not inside a git repository"))
}

/// Which runs belong to which repository; git is asked once per directory.
#[derive(Default)]
pub(super) struct Attribution {
    repos: HashMap<String, Option<RepoRef>>,
}

impl Attribution {
    /// Whether `origin` names `repo`: its worktree has the same common git dir, or its
    /// `owner/repo` is the repository's.
    pub(super) fn belongs(&mut self, origin: &Origin, repo: &RepoRef) -> bool {
        agent_events::attribute(
            Some(origin),
            std::slice::from_ref(repo),
            |dir| {
                self.repos
                    .entry(dir.to_owned())
                    .or_insert_with(|| RepoRef::of(dir))
                    .clone()
            },
            || None,
        )
        .is_some()
    }
}

/// The `origin` in the first line of a run file.
pub(super) fn origin_of(path: &Path) -> Option<Origin> {
    agent_events::first_head(path).and_then(|h| h.origin)
}

fn is_readable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= MAX_EVENTS_BYTES)
}

/// The run files of `dir` by run id.
pub(super) fn list(dir: &Path) -> BTreeMap<String, PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter_map(|p| {
            let run = agent_events::run_of(&p)?.to_owned();
            is_readable(&p).then_some((run, p))
        })
        .collect()
}

/// The runs of `repo` with their files, the shared one first: the shared runs whose origin names
/// it and the runs of its legacy directory. A shared file without origin belongs to the run the
/// legacy directory lists.
pub(super) fn of_repository(
    shared: Option<&Path>,
    repo: &RepoRef,
    attribution: &mut Attribution,
) -> BTreeMap<String, Vec<PathBuf>> {
    let mut out: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut unnamed = BTreeMap::new();
    for (run, path) in shared.map(list).unwrap_or_default() {
        match origin_of(&path) {
            Some(origin) if attribution.belongs(&origin, repo) => {
                out.insert(run, vec![path]);
            }
            Some(_) => {}
            None => {
                unnamed.insert(run, path);
            }
        }
    }
    for (run, path) in list(&agent_events::legacy_dir(&repo.common)) {
        let files = out.entry(run.clone()).or_default();
        if files.is_empty() {
            files.extend(unnamed.remove(&run));
        }
        files.push(path);
    }
    out
}

/// The path of the run `run` in `dir`; `None` for an id that is no run file name, so an argument
/// never leaves the directory.
pub(super) fn path_of(dir: &Path, run: &str) -> Option<PathBuf> {
    let path = dir.join(format!("{run}.jsonl"));
    (!run.contains(['/', '\\']) && agent_events::run_of(&path) == Some(run)).then_some(path)
}

/// The files of the run `run`, the shared one first; `legacy` is the legacy directory of `repo`.
/// A shared file of another repository's run of the same id is left out when the legacy directory
/// has the run.
pub(super) fn files_of(
    shared: Option<&Path>,
    legacy: Option<&Path>,
    repo: Option<&RepoRef>,
    run: &str,
    attribution: &mut Attribution,
) -> Vec<PathBuf> {
    let at = |dir: Option<&Path>| dir.and_then(|d| path_of(d, run)).filter(|p| is_readable(p));
    let (mut shared, legacy) = (at(shared), at(legacy));
    if legacy.is_some()
        && let Some(repo) = repo
        && let Some(origin) = shared.as_deref().and_then(origin_of)
        && !attribution.belongs(&origin, repo)
    {
        shared = None;
    }
    shared.into_iter().chain(legacy).collect()
}

/// The complete lines of an event file (a half-written last line waits); `None` for a file that
/// is not regular, too big or unreadable.
fn read_complete(path: &Path) -> Option<String> {
    let text = is_readable(path)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()?;
    let end = text.rfind('\n').map_or(0, |i| i + 1);
    Some(text[..end].to_owned())
}

/// The JSON objects among `lines`.
pub(super) fn parse<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Value> {
    lines
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(Value::is_object)
        .collect()
}

/// The events of the files of one run. One file: as written. More: each `seq` once, the first
/// file that has it wins (an event without `seq` counts from the first file only), in `seq` order.
pub(super) fn merge<'a>(per_file: impl ExactSizeIterator<Item = &'a [Value]>) -> Vec<Value> {
    let single = per_file.len() == 1;
    let mut seen = HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    for (index, events) in per_file.enumerate() {
        out.extend(
            events
                .iter()
                .filter(|e| single || e["seq"].as_i64().map_or(index == 0, |s| seen.insert(s)))
                .cloned(),
        );
    }
    if !single {
        out.sort_by_key(|e| e["seq"].as_i64().unwrap_or(i64::MAX));
    }
    out
}

/// The merged events of the files of one run.
pub(super) fn events(files: &[PathBuf]) -> Vec<Value> {
    let per_file: Vec<Vec<Value>> = files
        .iter()
        .filter_map(|p| read_complete(p))
        .map(|text| parse(text.lines()))
        .collect();
    merge(per_file.iter().map(Vec::as_slice))
}

/// The core reducer fed with `events`.
pub(super) fn fold(events: &[Value]) -> Reducer {
    let mut reducer = Reducer::new();
    for e in events {
        reducer.push(e);
    }
    reducer
}

/// The host name of this machine (`origin.host`), empty when unknown.
pub(super) fn host_name() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            let out = gui_path::command("hostname").output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        })
        .map(|h| h.trim().to_owned())
        .unwrap_or_default()
}
