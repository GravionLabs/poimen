//! What the writers of run files (`report_done`, `record_run`) share (#1417): globally unique run
//! ids, the `origin` of `run.started` and the append to the run file in the shared directory.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use poimen_core::agent_events::{Origin, RepoRef};
use poimen_core::git;
use serde_json::{Value, json};

use super::run_files;

/// A globally unique, time-ordered run id: `prefix` and a UUIDv7 (agent-events ADR 0006). The
/// random bits come from the keys std draws from the operating system for its hash maps.
pub(super) fn new_run_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let random = || RandomState::new().hash_one((now.as_nanos(), std::process::id()));
    let (a, b) = (random(), random());
    let millis = now.as_millis() as u64;
    format!(
        "{prefix}{:08x}-{:04x}-7{:03x}-{:04x}-{:012x}",
        (millis >> 16) & 0xffff_ffff,
        millis & 0xffff,
        a & 0xfff,
        0x8000 | (b >> 50),
        b & 0xffff_ffff_ffff,
    )
}

/// The `origin` of a run written for a terminal in `dir` of the repository `repo` (`None` outside
/// one); fields that do not exist are left out.
pub(super) fn origin(dir: &str, repo: Option<&RepoRef>) -> Origin {
    let branch = repo.and_then(|_| {
        let out = git::run_bytes(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?;
        Some(String::from_utf8_lossy(&out).trim().to_owned()).filter(|b| !b.is_empty())
    });
    Origin {
        repo: repo.map(|r| r.name.clone()),
        branch,
        worktree: repo.map(|r| r.top.clone()),
        host: Some(run_files::host_name()).filter(|h| !h.is_empty()),
    }
}

/// Appends `events` to the run `run` in the shared directory `shared`. `before` are the events
/// the run has so far: a run that is only in a legacy directory (started before the upgrade) gets
/// its `run.started`, with `origin`, as the first line of its shared file, so a reader of the
/// shared directory alone knows the run and its repository.
pub(super) fn append(
    shared: &Path,
    run: &str,
    before: &[Value],
    origin: &Origin,
    events: &[Value],
) -> std::io::Result<()> {
    std::fs::create_dir_all(shared)?;
    let path = shared.join(format!("{run}.jsonl"));
    let mut text = String::new();
    if !path.exists()
        && let Some(mut started) = before
            .iter()
            .find(|e| e["type"] == json!("run.started"))
            .cloned()
    {
        if started.get("origin").is_none() {
            started["origin"] = json!(origin);
        }
        text.push_str(&format!("{started}\n"));
    }
    for e in events {
        text.push_str(&format!("{e}\n"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}
