//! The command that watches the deliver runs; the watch itself is in `deliver.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use poimen_core::agent_events::RepoRef;
use tauri::{AppHandle, Emitter, State};

use crate::{DeliverWatch, Shared, deliver};

/// How old the terminal list may be before a run it does not name asks the backend again.
const TERMINALS_TTL: Duration = Duration::from_secs(2);

/// The directory of the terminal of a run, from the backend's snapshot.
fn terminal_dir(backend: Shared) -> deliver::TerminalDir {
    let cache: Mutex<(Option<Instant>, HashMap<String, String>)> = Mutex::default();
    Arc::new(move |run| {
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        let stale = cache.0.is_none_or(|at| at.elapsed() >= TERMINALS_TTL);
        if stale && !cache.1.contains_key(run) {
            if let Ok(snapshot) = backend.snapshot() {
                cache.1 = snapshot
                    .terminals
                    .into_iter()
                    .filter_map(|t| Some((t.run?, t.cwd?)))
                    .collect();
            }
            cache.0 = Some(Instant::now());
        }
        cache.1.get(run).cloned()
    })
}

/// Watches the deliver runs (progress events in the shared event directory and in
/// `<git-common-dir>/.agents/events`, else the status file `.agents/deliver-loop/status.json`) of the repositories of `dirs` (one watch per
/// repository: its worktrees count as one, named by the toplevel of the first), replacing the
/// watched set; each change arrives as a `deliver` event `{ root, read }`, first with the current
/// content. Returns the watched roots. Read-only.
#[tauri::command]
pub async fn deliver_watch(
    app: AppHandle,
    watch: State<'_, DeliverWatch>,
    backend: State<'_, Shared>,
    dirs: Vec<String>,
) -> Result<Vec<String>, String> {
    let (roots, repos) = tauri::async_runtime::spawn_blocking(move || {
        let roots = crate::git::repo_roots(&dirs);
        let repos: Vec<RepoRef> = roots.iter().filter_map(|r| RepoRef::of(r)).collect();
        (roots, repos)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
    *watches.context.repos.lock().map_err(|e| e.to_string())? = repos;
    if watches.context.terminal_dir.is_none() {
        watches.context.terminal_dir = Some(terminal_dir(backend.inner().clone()));
    }
    let current = watches.handles.keys().cloned().collect();
    let (stop, start) = deliver::plan(&current, &roots);
    for root in stop {
        watches.handles.remove(&root);
    }
    for root in start {
        let (app, name) = (app.clone(), root.clone());
        let context = watches.context.clone();
        let handle = deliver::watch(PathBuf::from(&root), context, move |read| {
            let _ = app.emit("deliver", serde_json::json!({ "root": name, "read": read }));
        });
        watches.handles.insert(root, handle);
    }
    Ok(roots)
}
