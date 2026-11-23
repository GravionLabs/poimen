//! The command that watches the deliver runs; the watch itself is in `deliver.rs`.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::{DeliverWatch, deliver};

/// Watches the deliver runs (progress events in `<git-common-dir>/.agents/events`, else the
/// status file `.agents/deliver-loop/status.json`) of the repositories of `dirs` (one watch per
/// repository: its worktrees count as one, named by the toplevel of the first), replacing the
/// watched set; each change arrives as a `deliver` event `{ root, read }`, first with the current
/// content. Returns the watched roots. Read-only.
#[tauri::command]
pub async fn deliver_watch(
    app: AppHandle,
    watch: State<'_, DeliverWatch>,
    dirs: Vec<String>,
) -> Result<Vec<String>, String> {
    let roots = tauri::async_runtime::spawn_blocking(move || crate::git::repo_roots(&dirs))
        .await
        .map_err(|e| e.to_string())?;
    let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
    let current = watches.keys().cloned().collect();
    let (stop, start) = deliver::plan(&current, &roots);
    for root in stop {
        watches.remove(&root);
    }
    for root in start {
        let (app, name) = (app.clone(), root.clone());
        let handle = deliver::watch(PathBuf::from(&root), move |read| {
            let _ = app.emit("deliver", serde_json::json!({ "root": name, "read": read }));
        });
        watches.insert(root, handle);
    }
    Ok(roots)
}
