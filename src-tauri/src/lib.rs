mod backend;
mod copilot;
mod git;
mod github;
mod github_branches;
mod github_issues;
mod github_milestones;
mod github_pulls;
mod gui_path;
mod harness;
mod herdr;
mod layout;
#[cfg(test)]
mod live;
mod project;
mod schemes;
mod settings;
mod spaces;
mod stream;
mod tasks;
mod transcripts;
mod trust;
mod updater;
mod watch;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use herdr::Client;
use stream::{Mode, OpenRequest, Sessions};

struct HerdrBin(PathBuf);

/// Serializes `ensure_backend`.
#[derive(Default)]
struct StartLock(std::sync::Arc<Mutex<()>>);

/// The running subagent transcript watches by key: one per frontend follower (the output panel,
/// each visible flow tab), each with at most one watch.
#[derive(Default)]
struct SubagentWatch(Mutex<Watches<transcripts::WatchHandle>>);

/// Values by key, each with the generation of the latest command for that key.
///
/// Every command takes a generation when it starts; a result that finishes after a newer command
/// for the same key (another watch or an unwatch) began is stale and dropped instead of stored.
/// Generations are unique across keys, so a key removed and begun again never matches an older
/// in-flight command.
struct Watches<T> {
    next: u64,
    slots: HashMap<String, (u64, Option<T>)>,
}

impl<T> Default for Watches<T> {
    fn default() -> Self {
        Self {
            next: 0,
            slots: HashMap::new(),
        }
    }
}

impl<T> Watches<T> {
    /// A new command for `key` starts: older in-flight commands for it become stale.
    fn begin(&mut self, key: &str) -> u64 {
        self.next += 1;
        self.slots.entry(key.to_owned()).or_insert((0, None)).0 = self.next;
        self.next
    }

    /// Stores `value` for `key` unless a newer command began since `generation` or the key was
    /// removed; the previous value is dropped (stopped) when stored. `false` for a stale result,
    /// which is dropped.
    fn store(&mut self, key: &str, generation: u64, value: Option<T>) -> bool {
        match self.slots.get_mut(key) {
            Some(slot) if slot.0 == generation => {
                slot.1 = value;
                true
            }
            _ => false,
        }
    }

    /// Drops the value of `key` and makes every in-flight command for it stale.
    fn remove(&mut self, key: &str) {
        self.slots.remove(key);
    }
}

/// Workspaces, tabs and panes in one call; the frontend rebuilds its tree from it.
#[tauri::command]
async fn snapshot(client: State<'_, Client>) -> Result<Value, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let read = |method: &str| client.request(method, json!({})).map_err(|e| e.to_string());
        Ok(json!({
            "workspaces": read("workspace.list")?["workspaces"],
            "tabs": read("tab.list")?["tabs"],
            "panes": read("pane.list")?["panes"],
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs one herdr request off the main thread; shared by the pane commands below.
async fn herdr_call(client: &Client, method: &'static str, params: Value) -> Result<Value, String> {
    let client = client.clone();
    tauri::async_runtime::spawn_blocking(move || {
        client.request(method, params).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Splits a pane (`pane.split`); returns the new pane id. The change also arrives as events.
#[tauri::command]
async fn pane_split(
    client: State<'_, Client>,
    target: String,
    direction: herdr::SplitDirection,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    let params = herdr::split_params(&target, direction, cwd.as_deref());
    let result = herdr_call(client.inner(), "pane.split", params).await?;
    Ok(herdr::created_pane_id(&result))
}

/// Closes a pane (`pane.close`). The UI confirms first; herdr ends the pane's process.
#[tauri::command]
async fn pane_close(client: State<'_, Client>, pane_id: String) -> Result<(), String> {
    herdr_call(client.inner(), "pane.close", herdr::close_params(&pane_id)).await?;
    Ok(())
}

/// Closes a space (`workspace.close`) with all its panes; only on an explicit user action (the UI
/// confirms first when it has panes). Afterwards `focus`, the neighbouring space, is focused on a
/// best-effort basis; a failing close leaves the space untouched and returns herdr's error.
#[tauri::command]
async fn space_close(
    client: State<'_, Client>,
    workspace_id: String,
    focus: Option<String>,
) -> Result<(), String> {
    herdr_call(
        client.inner(),
        "workspace.close",
        herdr::workspace_close_params(&workspace_id),
    )
    .await?;
    if let Some(id) = focus.filter(|id| !id.is_empty()) {
        let _ = herdr_call(client.inner(), "workspace.focus", spaces::focus_params(&id)).await;
    }
    Ok(())
}

/// Starts a supported agent in an existing pane at a shell prompt (`agent.start`); can take a while.
#[tauri::command]
async fn agent_start(
    client: State<'_, Client>,
    pane_id: String,
    kind: String,
    name: Option<String>,
    args: Vec<String>,
) -> Result<(), String> {
    let params = herdr::agent_start_params(&pane_id, &kind, name.as_deref(), &args);
    herdr_call(client.inner(), "agent.start", params).await?;
    Ok(())
}

/// Renames an agent (`agent.rename`); an empty name clears it.
#[tauri::command]
async fn agent_rename(
    client: State<'_, Client>,
    target: String,
    name: Option<String>,
) -> Result<(), String> {
    let params = herdr::agent_rename_params(&target, name.as_deref());
    herdr_call(client.inner(), "agent.rename", params).await?;
    Ok(())
}

/// Sends a prompt to an agent (`agent.prompt`) without focusing its terminal; no controller needed.
/// Stalled, timed-out and blocked agents are results, not errors.
#[tauri::command]
async fn agent_prompt(
    client: State<'_, Client>,
    target: String,
    text: String,
) -> Result<herdr::PromptOutcome, String> {
    let client = client.inner().clone();
    let params = herdr::agent_prompt_params(&target, &text);
    tauri::async_runtime::spawn_blocking(move || {
        herdr::prompt_outcome(client.request("agent.prompt", params))
    })
    .await
    .map_err(|e| e.to_string())
}

/// Renames a pane (`pane.rename`); an empty label clears it.
#[tauri::command]
async fn pane_rename(
    client: State<'_, Client>,
    pane_id: String,
    label: Option<String>,
) -> Result<(), String> {
    let params = herdr::pane_rename_params(&pane_id, label.as_deref());
    herdr_call(client.inner(), "pane.rename", params).await?;
    Ok(())
}

type GithubClient = std::sync::Arc<github::Client<github::SystemRunner>>;

/// Open pull requests of the GitHub repositories among `dirs` (space and explorer directories),
/// one GraphQL query per repository. A failing repository carries its own error. Read-only.
#[tauri::command]
async fn github_pulls(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<github_pulls::Pulls, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || github_pulls::load(&client, &dirs))
        .await
        .map_err(|e| e.to_string())
}

/// The pull request (open or merged) of the branch of each directory, one GraphQL query per
/// repository; directories without one are left out. Read-only; `gh` errors give an empty answer.
#[tauri::command]
async fn branch_pulls(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<HashMap<String, github_branches::BranchPull>, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || github_branches::load(&client, dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Issues of the GitHub repositories among `dirs` with parent links and linked pull requests (open
/// ones, or the closed ones with `closed`), up to 300 per repository. Read-only.
#[tauri::command]
async fn github_issues(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
    closed: Option<bool>,
) -> Result<Vec<github_issues::RepoIssues>, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    let closed = closed.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || github_issues::load(&client, &dirs, closed))
        .await
        .map_err(|e| e.to_string())
}

/// Open milestones of the GitHub repositories among `dirs` (REST with ETag), ordered by version
/// with the current one marked. Read-only.
#[tauri::command]
async fn github_milestones(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<Vec<github_milestones::RepoMilestones>, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || github_milestones::load(&client, &dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Opens a new terminal in a workspace as a new tab (`tab.create`); returns its pane id.
#[tauri::command]
async fn terminal_create(
    client: State<'_, Client>,
    workspace_id: String,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    let params = herdr::tab_create_params(&workspace_id, cwd.as_deref());
    let result = herdr_call(client.inner(), "tab.create", params).await?;
    Ok(herdr::created_pane_id(&result))
}

/// Creates a space (`workspace.create`) for a directory and focuses it; when a space for the
/// directory exists (a pane inside it, else one labelled with its name) that one is focused.
#[tauri::command]
async fn create_space(
    client: State<'_, Client>,
    cwd: String,
    label: Option<String>,
    decision: Option<trust::Decision>,
) -> Result<spaces::SpaceOutcome, String> {
    let home = home_dir();
    let dir = spaces::resolve_dir(&cwd, home.as_deref())?;
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let call =
            |method: &str, params: Value| client.request(method, params).map_err(|e| e.to_string());
        let list = |method: &str, key: &str| {
            Ok::<Vec<Value>, String>(
                call(method, json!({}))?[key]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            )
        };
        let workspaces = list("workspace.list", "workspaces")?;
        let panes = list("pane.list", "panes")?;
        if let Some(workspace_id) = spaces::find_space(&dir, &workspaces, &panes) {
            call("workspace.focus", spaces::focus_params(&workspace_id))?;
            return Ok(spaces::SpaceOutcome {
                workspace_id,
                created: false,
                warnings: Vec::new(),
                trust: None,
            });
        }
        // The project files are read once; the trust check, the listed commands and the layout
        // all use this read.
        let root = git::toplevel(&dir).unwrap_or_else(|| dir.clone());
        let sources = trust::Sources::read(&root);
        let user = home.as_deref().map(settings::load);
        let trusted = user
            .as_ref()
            .map(|s| s.trusted_projects.clone())
            .unwrap_or_default();
        let allow = match trust::gate(&sources, &trusted, decision.as_ref()) {
            trust::Gate::Ask(request) => {
                return Ok(spaces::SpaceOutcome {
                    workspace_id: String::new(),
                    created: false,
                    warnings: Vec::new(),
                    trust: Some(request),
                });
            }
            trust::Gate::Strip => false,
            trust::Gate::Run { store } => {
                if let Some(hash) = store {
                    let home = home.as_deref().ok_or("the home directory is not known")?;
                    settings::trust_project(home, &sources.root, &hash)?;
                }
                true
            }
        };
        let label = spaces::space_label(
            &dir,
            label.as_deref().unwrap_or_default(),
            home.as_deref(),
            &workspaces,
        );
        let result = call("workspace.create", spaces::create_params(&dir, &label))?;
        let workspace_id = spaces::created_workspace_id(&result)
            .ok_or("herdr did not report the new workspace")?;
        call("workspace.focus", spaces::focus_params(&workspace_id))?;
        let user_layout = user.and_then(|s| s.spaces_layout);
        let warnings = build_layout(&call, &dir, &result, &sources, user_layout.as_ref(), allow);
        Ok(spaces::SpaceOutcome {
            workspace_id,
            created: true,
            warnings,
            trust: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Builds the pane layout in a just-created space from the already read project `sources`:
/// `.poimen/config.json`, else the deprecated `.agents/herdr-layout.json`, else `spaces.layout` of
/// the config (`user_layout`), else the default. With `allow_project_commands` false the commands
/// of a layout that comes from a project file are not typed; the panes are still created and named.
/// Problems come back as warnings; the space then keeps the panes created so far (at least its
/// single root pane).
fn build_layout(
    call: &impl Fn(&str, Value) -> Result<Value, String>,
    dir: &str,
    created: &Value,
    sources: &trust::Sources,
    user_layout: Option<&layout::Layout>,
    allow_project_commands: bool,
) -> Vec<String> {
    let (project, legacy) = sources.refs();
    let (layout, mut warnings, from_project) =
        match layout::resolve_origin(project, legacy, user_layout) {
            Ok(r) => r,
            Err(warning) => return vec![warning],
        };
    let layout = if from_project && !allow_project_commands {
        layout.without_commands()
    } else {
        layout
    };
    let Some(root_pane) = herdr::created_pane_id(created) else {
        return vec!["herdr did not report the root pane; no layout applied".into()];
    };
    if let Err(e) = layout::apply(call, &layout, dir, &root_pane) {
        warnings.push(format!("layout not fully applied: {e}"));
    }
    warnings
}

/// Revokes the trust of the project file of `root`; the settings watcher emits the change.
#[tauri::command]
fn revoke_project(root: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::revoke_project(&home, &root)
}

/// The project file `.poimen/config.json` of the repository containing `dir`, with warnings.
#[tauri::command]
async fn project_config(dir: String) -> Result<project::Loaded, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = git::toplevel(&dir).unwrap_or(dir);
        project::load(&root)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Directories matching a typed path, for the completion of path fields.
#[tauri::command]
async fn list_dirs(input: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        spaces::list_dirs(&input, home_dir().as_deref(), 50)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The subdirectories of a directory for the explorer tree; unreadable ones have none.
#[tauri::command]
async fn list_child_dirs(path: String, hidden: bool) -> Result<Vec<spaces::DirEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        spaces::child_dirs(&path, home_dir().as_deref(), hidden)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Version and protocol of the running herdr; `None` when unreachable or too old.
#[tauri::command]
async fn herdr_info(client: State<'_, Client>) -> Result<Option<herdr::ServerInfo>, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || client.info())
        .await
        .map_err(|e| e.to_string())
}

/// Starts the herdr server when nothing answers on its socket (and `herdr.autoStart` allows it),
/// then waits until it answers. Runs at start and on Retry; one at a time so a retry during a
/// start never launches a second server.
#[tauri::command]
async fn ensure_backend(
    client: State<'_, Client>,
    lock: State<'_, StartLock>,
) -> Result<backend::BackendStatus, String> {
    let client = client.inner().clone();
    let lock = lock.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use backend::Backend;
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let auto_start = home_dir()
            .map(|h| settings::load(&h).herdr_auto_start)
            .unwrap_or(true);
        backend::HerdrBackend::new(backend::SystemRunner::new(client), auto_start)
            .ensure_running()
            .into()
    })
    .await
    .map_err(|e| e.to_string())
}

/// The running poimen build's version (Tauri app version).
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The user's home directory: `HOME` on Unix, the profile (`%USERPROFILE%`) on Windows, where a
/// GUI app has no `HOME`.
fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|h| !h.as_os_str().is_empty())
}

/// The resolved user settings (`~/.config/poimen/config.json`); changes arrive as `settings` events.
#[tauri::command]
fn get_settings() -> settings::Settings {
    home_dir().map(|h| settings::load(&h)).unwrap_or_default()
}

/// The agent harnesses with availability (program found on the `PATH` of a child).
#[tauri::command]
async fn get_harnesses() -> Vec<harness::HarnessInfo> {
    let harnesses = get_settings().harnesses;
    tauri::async_runtime::spawn_blocking(move || harness::with_availability(harnesses))
        .await
        .unwrap_or_default()
}

/// The resolved task modes (`tasks` in config.json).
#[tauri::command]
fn get_task_modes() -> tasks::TaskModes {
    get_settings().tasks
}

/// The user color schemes (`~/.config/poimen/themes/*.toml`); changes arrive as `color-schemes` events.
#[tauri::command]
fn get_color_schemes() -> schemes::Schemes {
    home_dir().map(|h| schemes::load(&h)).unwrap_or_default()
}

/// Writes only the `theme` setting (the toolbar button); the settings watcher emits the change.
#[tauri::command]
fn set_theme(theme: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_theme(&home, &theme)
}

/// Writes only the `colorSchema` setting (the settings view): a name or a `{light, dark}` pair.
#[tauri::command]
fn set_color_schema(schema: serde_json::Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_color_schema(&home, &schema)
}

/// Writes only `sidebar.layout` (`tree` or `split`); the settings watcher emits the change.
#[tauri::command]
fn set_sidebar_layout(layout: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_sidebar_layout(&home, &layout)
}

/// Writes only `explorer.roots` (paths under the home directory as `~/…`); the watcher emits it.
#[tauri::command]
fn set_explorer_roots(roots: Vec<String>) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_explorer_roots(&home, &roots)
}

/// Writes only `explorer.showHidden`; the settings watcher emits the change.
#[tauri::command]
fn set_explorer_show_hidden(show: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_explorer_show_hidden(&home, show)
}

/// Writes only `notifications.<kind>` (`blocked` or `done`); the settings watcher emits the change.
#[tauri::command]
fn set_notification(kind: String, on: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_notification(&home, &kind, on)
}

/// Writes only `herdr.autoStart`; the settings watcher emits the change.
#[tauri::command]
fn set_herdr_auto_start(on: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_herdr_auto_start(&home, on)
}

/// Writes only `spaces.layout` after validating it; the settings watcher emits the change.
#[tauri::command]
fn set_spaces_layout(layout: serde_json::Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_spaces_layout(&home, &layout)
}

/// Removes `spaces.layout`, so new spaces use the built-in default again.
#[tauri::command]
fn remove_spaces_layout() -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::remove_spaces_layout(&home)
}

/// The built-in pane layout of a new space, shown by the settings view when none is configured.
#[tauri::command]
fn default_spaces_layout() -> layout::Layout {
    layout::default_layout()
}

/// Branch and worktree flag per directory (read-only `git rev-parse`).
#[tauri::command]
async fn repo_info(dirs: Vec<String>) -> Result<HashMap<String, git::RepoInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || git::infos(dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Starts following the subagent transcripts of the Claude Code or GitHub Copilot CLI agent in
/// `pane_id` (replacing the previous watch); updates arrive as `subagents` events. `false` when the
/// pane has no such session or its transcript is not found; nothing is watched then. Read-only.
#[tauri::command]
async fn subagents_watch(
    app: AppHandle,
    client: State<'_, Client>,
    watch: State<'_, SubagentWatch>,
    key: String,
    pane_id: String,
) -> Result<bool, String> {
    let generation = watch.0.lock().map_err(|e| e.to_string())?.begin(&key);
    let panes = herdr_call(client.inner(), "pane.list", json!({})).await?;
    let found = herdr::parse_agent_session(&panes, &pane_id).and_then(|s| {
        let home = home_dir()?;
        let dir = match s.agent {
            herdr::TranscriptAgent::Claude => transcripts::locate_project_dir(
                &transcripts::projects_root(&home),
                &s.cwd,
                &s.session,
            )?,
            herdr::TranscriptAgent::Copilot => {
                copilot::locate_session_dir(&copilot::sessions_root(&home), &s.session)?
            }
        };
        Some((s.agent, dir, s.session))
    });
    let Some((agent, dir, session)) = found else {
        let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
        watches.store(&key, generation, None);
        return Ok(false);
    };
    let (id, follower) = (session.clone(), key.clone());
    let emit = move |update| {
        let event = json!({ "key": follower, "session": id, "update": update });
        let _ = app.emit("subagents", event);
    };
    let handle = match agent {
        herdr::TranscriptAgent::Claude => transcripts::watch(&dir, &session, emit)?,
        herdr::TranscriptAgent::Copilot => copilot::watch(&dir, emit)?,
    };
    let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
    // A stale handle is dropped here, which stops its watcher; so is one whose key was unwatched.
    Ok(watches.store(&key, generation, Some(handle)))
}

/// Stops the subagent watch of `key` and forgets the key.
#[tauri::command]
fn subagents_unwatch(watch: State<'_, SubagentWatch>, key: String) {
    if let Ok(mut watches) = watch.0.lock() {
        watches.remove(&key);
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects app/state; the rest is the IPC payload.
fn open_terminal(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    bin: State<'_, HerdrBin>,
    mode: Mode,
    target: String,
    cols: u16,
    rows: u16,
    takeover: bool,
) -> Result<u32, String> {
    let req = OpenRequest {
        bin: &bin.0,
        mode,
        target: &target,
        cols,
        rows,
        takeover,
    };
    sessions.open(req, move |id, record| {
        let _ = app.emit("terminal", json!({ "id": id, "record": record }));
    })
}

#[tauri::command]
fn terminal_input(sessions: State<'_, Sessions>, id: u32, text: String) -> Result<(), String> {
    sessions.send(id, &stream::input_command(&text))
}

#[tauri::command]
fn terminal_resize(
    sessions: State<'_, Sessions>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    sessions.send(id, &stream::resize_command(cols, rows))
}

#[tauri::command]
fn terminal_scroll(
    sessions: State<'_, Sessions>,
    id: u32,
    up: bool,
    lines: u16,
) -> Result<(), String> {
    sessions.send(id, &stream::scroll_command(up, lines))
}

#[tauri::command]
fn terminal_mouse(
    sessions: State<'_, Sessions>,
    id: u32,
    action: String,
    button: String,
    column: u16,
    row: u16,
    modifiers: u8,
) -> Result<(), String> {
    stream::validate_mouse(&action, &button)?;
    sessions.send(
        id,
        &stream::mouse_command(&action, &button, column, row, modifiers),
    )
}

#[tauri::command]
fn close_terminal(sessions: State<'_, Sessions>, id: u32) {
    sessions.close(id);
}

/// The accelerator the native menu carries. Only Cmd bindings on macOS: elsewhere (and for Ctrl
/// or Alt bindings) the webview matches the keys itself, so a terminal that has focus keeps them.
fn native_accelerator(accelerator: &str) -> Option<String> {
    let cmd = accelerator
        .split('+')
        .any(|t| matches!(t, "Cmd" | "CmdOrCtrl"));
    if !cfg!(target_os = "macos") || !cmd {
        return None;
    }
    Some(
        accelerator
            .split('+')
            .map(|t| match t {
                "Left" => "ArrowLeft",
                "Right" => "ArrowRight",
                "Up" => "ArrowUp",
                "Down" => "ArrowDown",
                other => other,
            })
            .collect::<Vec<_>>()
            .join("+"),
    )
}

fn action_item(
    app: &AppHandle,
    shortcuts: &BTreeMap<String, String>,
    id: &str,
    text: &str,
    enabled: bool,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    let accelerator = shortcuts.get(id).and_then(|a| native_accelerator(a));
    // An accelerator muda cannot parse leaves the item without one instead of failing the menu.
    MenuItem::with_id(app, id, text, enabled, accelerator.as_deref())
        .or_else(|_| MenuItem::with_id(app, id, text, enabled, None::<&str>))
}

fn build_menu(
    app: &AppHandle,
    shortcuts: &BTreeMap<String, String>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str| action_item(app, shortcuts, id, text, true);
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::new(app)?;
    if cfg!(target_os = "macos") {
        menu.append(&Submenu::with_items(
            app,
            "poimen",
            true,
            &[
                &PredefinedMenuItem::about(app, None, None)?,
                &sep()?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &sep()?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?)?;
    }
    let new_terminal = item("newTerminal", "New Terminal")?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &new_terminal,
            &sep()?,
            &item("closeTab", "Close Tab")?,
            &item("closeAll", "Close All Tabs")?,
        ],
    )?;
    if !cfg!(target_os = "macos") {
        file.append(&sep()?)?;
        file.append(&PredefinedMenuItem::quit(app, None)?)?;
    }
    menu.append(&file)?;
    menu.append(&Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "View",
        true,
        &[
            &item("toggleSidebar", "Toggle Sidebar")?,
            &item("toggleOutput", "Toggle Output Panel")?,
            &sep()?,
            &item("splitRight", "Split Right")?,
            &item("splitDown", "Split Down")?,
            &sep()?,
            &item("zoomIn", "Zoom In")?,
            &item("zoomOut", "Zoom Out")?,
            &item("zoomReset", "Actual Size")?,
            &sep()?,
            &item("refresh", "Refresh")?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "Go",
        true,
        &[
            &item("commandPalette", "Command Palette...")?,
            &sep()?,
            &item("nextWaiting", "Next Waiting Agent")?,
            &sep()?,
            &item("nextTab", "Next Tab")?,
            &item("previousTab", "Previous Tab")?,
            &sep()?,
            &item("showShortcuts", "Keyboard Shortcuts")?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?)?;
    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let bin = stream::resolve_bin_from_env();
    let client = Client::from_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::PendingUpdate::default())
        .manage(client.clone())
        .manage::<GithubClient>(std::sync::Arc::new(github::Client::system(
            home_dir()
                .map(|h| {
                    h.join(".config")
                        .join("poimen")
                        .join("cache")
                        .join("github")
                })
                .unwrap_or_else(|| std::env::temp_dir().join("poimen-github")),
        )))
        .manage(Sessions::default())
        .manage(SubagentWatch::default())
        .manage(HerdrBin(bin))
        .manage(StartLock::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            let shortcuts = home_dir()
                .map(|h| settings::load(&h).shortcuts)
                .unwrap_or_else(settings::default_shortcuts);
            app.set_menu(build_menu(&handle, &shortcuts)?)?;
            app.on_menu_event(|app, event| {
                let _ = app.emit("menu", event.id().0.clone());
            });
            if let Some(home) = home_dir() {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    schemes::watch(&home, std::time::Duration::from_secs(1), |s| {
                        let _ = handle.emit("color-schemes", s);
                    });
                });
            }
            if let Some(home) = home_dir() {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    settings::watch(&home, std::time::Duration::from_secs(1), |s| {
                        let _ = handle.emit("settings", s);
                        // Menu items carry the accelerators; rebuild them on the main thread.
                        let (app, shortcuts) = (handle.clone(), s.shortcuts.clone());
                        let _ = handle.run_on_main_thread(move || {
                            if let Ok(menu) = build_menu(&app, &shortcuts) {
                                let _ = app.set_menu(menu);
                            }
                        });
                    });
                });
            }
            std::thread::spawn(move || {
                watch::run(client, |event| {
                    let _ = handle.emit("herdr", event);
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            herdr_info,
            ensure_backend,
            set_herdr_auto_start,
            set_notification,
            pane_split,
            pane_close,
            space_close,
            agent_start,
            agent_rename,
            agent_prompt,
            pane_rename,
            terminal_create,
            github_pulls,
            branch_pulls,
            github_issues,
            github_milestones,
            create_space,
            list_dirs,
            list_child_dirs,
            app_version,
            updater::update_check,
            updater::update_install,
            get_settings,
            get_harnesses,
            get_task_modes,
            get_color_schemes,
            set_theme,
            set_color_schema,
            set_sidebar_layout,
            set_explorer_roots,
            set_explorer_show_hidden,
            set_spaces_layout,
            revoke_project,
            remove_spaces_layout,
            default_spaces_layout,
            project_config,
            repo_info,
            subagents_watch,
            subagents_unwatch,
            open_terminal,
            terminal_input,
            terminal_resize,
            terminal_scroll,
            terminal_mouse,
            close_terminal
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Release controllers and stop the CLI children; herdr panes keep running.
                app.state::<Sessions>().close_all();
            }
        });
}

#[cfg(test)]
mod subagent_watches_tests {
    use super::Watches;

    #[test]
    fn a_newer_command_makes_an_older_result_stale() {
        let mut watches = Watches::<&str>::default();
        let older = watches.begin("panel");
        let newer = watches.begin("panel");
        assert!(watches.store("panel", newer, Some("new")));
        assert!(!watches.store("panel", older, Some("old")));
        assert_eq!(watches.slots["panel"].1, Some("new"));
    }

    #[test]
    fn a_remove_before_a_watch_finishes_is_not_undone() {
        let mut watches = Watches::<&str>::default();
        let generation = watches.begin("panel");
        watches.remove("panel");
        assert!(!watches.store("panel", generation, Some("late")));
        assert!(!watches.slots.contains_key("panel"));
    }

    #[test]
    fn a_key_begun_again_after_a_remove_does_not_take_an_older_result() {
        let mut watches = Watches::<&str>::default();
        let older = watches.begin("flow");
        watches.remove("flow");
        let newer = watches.begin("flow");
        assert!(!watches.store("flow", older, Some("old")));
        assert!(watches.store("flow", newer, Some("new")));
    }

    #[test]
    fn keys_do_not_make_each_other_stale() {
        let mut watches = Watches::<&str>::default();
        let panel = watches.begin("panel");
        let flow = watches.begin("flow");
        assert!(watches.store("panel", panel, Some("p")));
        assert!(watches.store("flow", flow, Some("f")));
        watches.remove("flow");
        assert_eq!(watches.slots["panel"].1, Some("p"));
    }

    #[test]
    fn a_current_result_replaces_the_value_and_none_clears_it() {
        let mut watches = Watches::<&str>::default();
        let g = watches.begin("panel");
        assert!(watches.store("panel", g, Some("a")));
        let g = watches.begin("panel");
        assert!(watches.store("panel", g, None));
        assert_eq!(watches.slots["panel"].1, None);
    }
}

#[cfg(test)]
mod build_layout_tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    const PROJECT_TEXT: &str = r#"{"layout":{"panes":[{"name":"agent","command":"evil"},
        {"name":"sh","command":"worse","split":{"direction":"down"}}]}}"#;

    fn sources(text: &str) -> trust::Sources {
        trust::Sources {
            root: "/work/app".into(),
            project: Some(("/work/app/.poimen/config.json".into(), text.into())),
            legacy: None,
        }
    }

    fn run(
        sources: &trust::Sources,
        user: Option<&layout::Layout>,
        allow: bool,
    ) -> Vec<(String, Value)> {
        let calls = RefCell::new(Vec::new());
        let call = |method: &str, params: Value| -> Result<Value, String> {
            let mut c = calls.borrow_mut();
            let n = c.len();
            c.push((method.to_owned(), params));
            Ok(match method {
                "pane.split" => json!({ "pane": { "pane_id": format!("w1:p{}", n + 10) } }),
                _ => json!({}),
            })
        };
        let created = json!({ "root_pane": { "pane_id": "w1:root" } });
        let warnings = build_layout(&call, "/work/app", &created, sources, user, allow);
        assert!(warnings.is_empty(), "{warnings:?}");
        calls.into_inner()
    }

    fn typed(calls: &[(String, Value)]) -> Vec<&Value> {
        calls
            .iter()
            .filter(|(m, _)| m == "pane.send_input")
            .map(|(_, p)| p)
            .collect()
    }

    #[test]
    fn untrusted_project_layout_builds_panes_but_types_nothing() {
        let calls = run(&sources(PROJECT_TEXT), None, false);
        let methods: Vec<_> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            ["pane.rename", "pane.split", "pane.rename"],
            "{calls:?}"
        );
        assert!(typed(&calls).is_empty());
    }

    #[test]
    fn trusted_project_layout_types_its_commands() {
        let calls = run(&sources(PROJECT_TEXT), None, true);
        assert_eq!(typed(&calls).len(), 2);
    }

    #[test]
    fn the_user_layout_needs_no_trust() {
        let user = layout::default_layout();
        // No project layout: the user's `claude` is typed although project commands are blocked.
        let calls = run(&sources("{\"setup\":\"make\"}"), Some(&user), false);
        assert_eq!(typed(&calls).len(), 1);
        let calls = run(&trust::Sources::default(), Some(&user), false);
        assert_eq!(typed(&calls).len(), 1);
    }
}
