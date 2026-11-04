mod backend;
mod copilot;
mod git;
mod gui_path;
mod herdr;
mod layout;
#[cfg(test)]
mod live;
mod plugin_view;
mod schemes;
mod settings;
mod spaces;
mod stream;
mod transcripts;
mod watch;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
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
    args: String,
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

/// Lists the actions of all herdr plugins (`plugin.action.list`).
#[tauri::command]
async fn plugin_actions(client: State<'_, Client>) -> Result<Vec<herdr::PluginAction>, String> {
    let result = herdr_call(client.inner(), "plugin.action.list", json!({})).await?;
    // Names are optional: without `plugin.list` the groups fall back to the plugin id.
    let names = match herdr_call(client.inner(), "plugin.list", json!({})).await {
        Ok(list) => herdr::parse_plugin_names(&list),
        Err(_) => Default::default(),
    };
    Ok(herdr::parse_plugin_actions(&result, &names))
}

/// Runs a plugin action (`plugin.action.invoke`) with the focused pane and workspace as context.
#[tauri::command]
async fn plugin_invoke(
    client: State<'_, Client>,
    plugin_id: String,
    action_id: String,
    context: herdr::PluginContext,
) -> Result<(), String> {
    let params = herdr::plugin_invoke_params(&plugin_id, &action_id, &context);
    herdr_call(client.inner(), "plugin.action.invoke", params).await?;
    Ok(())
}

/// The deliver board for every repository among the pane directories of the herdr session: runs the
/// plugin's `data` action per repository (ADR 0004). Repositories come from herdr's own panes, so
/// only known spaces are ever passed to the plugin. Read-only.
#[tauri::command]
async fn deliver_board(
    client: State<'_, Client>,
    bin: State<'_, HerdrBin>,
) -> Result<plugin_view::BoardView, String> {
    let client = client.inner().clone();
    let herdr_bin = bin.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let list = client
            .request("plugin.list", json!({}))
            .map_err(|e| e.to_string())?;
        let panes = client
            .request("pane.list", json!({}))
            .map_err(|e| e.to_string())?;
        let mut dirs: Vec<String> = panes["panes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                ["foreground_cwd", "cwd"]
                    .iter()
                    .find_map(|k| p.get(*k)?.as_str().filter(|s| !s.is_empty()))
                    .map(str::to_owned)
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        let roots = git::repo_roots(&dirs);
        let state = plugin_state_dir("deliver-board");
        Ok(plugin_view::load_board(&list, &roots, &state, &herdr_bin))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where poimen keeps the state (the `gh` cache) of a plugin it runs itself: herdr does not report
/// the plugin's own state directory, so it is `~/.config/poimen/plugin-state/<id>`.
fn plugin_state_dir(plugin_id: &str) -> PathBuf {
    plugin_state_dir_in(&home_dir().unwrap_or_default(), plugin_id)
}

/// Under `<home>/.config/poimen` on every system, Windows included (see `settings::config_path`).
fn plugin_state_dir_in(home: &Path, plugin_id: &str) -> PathBuf {
    home.join(".config")
        .join("poimen")
        .join("plugin-state")
        .join(plugin_id)
}

/// Lists the panes plugins declare (`panes[]` of `plugin.list`).
#[tauri::command]
async fn plugin_pane_defs(client: State<'_, Client>) -> Result<Vec<herdr::PluginPaneDef>, String> {
    let result = herdr_call(client.inner(), "plugin.list", json!({})).await?;
    Ok(herdr::parse_plugin_pane_defs(&result))
}

/// Opens a plugin pane as a tab (`plugin.pane.open`); returns the pane herdr created.
#[tauri::command]
async fn plugin_pane_open(
    client: State<'_, Client>,
    plugin_id: String,
    entrypoint: String,
    workspace_id: Option<String>,
) -> Result<herdr::PluginPane, String> {
    let params = herdr::plugin_pane_open_params(&plugin_id, &entrypoint, workspace_id.as_deref());
    let result = herdr_call(client.inner(), "plugin.pane.open", params).await?;
    herdr::parse_plugin_pane(&result).ok_or_else(|| "herdr answered without a plugin pane".into())
}

/// Focuses a plugin pane in herdr (`plugin.pane.focus`).
#[tauri::command]
async fn plugin_pane_focus(client: State<'_, Client>, pane_id: String) -> Result<(), String> {
    herdr_call(
        client.inner(),
        "plugin.pane.focus",
        json!({ "pane_id": pane_id }),
    )
    .await?;
    Ok(())
}

/// Closes a plugin pane in herdr (`plugin.pane.close`); only on an explicit user action.
#[tauri::command]
async fn plugin_pane_close(client: State<'_, Client>, pane_id: String) -> Result<(), String> {
    herdr_call(
        client.inner(),
        "plugin.pane.close",
        json!({ "pane_id": pane_id }),
    )
    .await?;
    Ok(())
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
            });
        }
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
        let warnings = build_layout(&call, &dir, home.as_deref(), &result);
        Ok(spaces::SpaceOutcome {
            workspace_id,
            created: true,
            warnings,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Builds the pane layout in a just-created space: the project file (git toplevel of `dir`), else
/// `spaces.layout` of the config, else the default. Problems come back as warnings; the space then
/// keeps the panes created so far (at least its single root pane).
fn build_layout(
    call: &impl Fn(&str, Value) -> Result<Value, String>,
    dir: &str,
    home: Option<&std::path::Path>,
    created: &Value,
) -> Vec<String> {
    let root = git::toplevel(dir).unwrap_or_else(|| dir.to_owned());
    let path = std::path::Path::new(&root).join(layout::REPO_LAYOUT_FILE);
    let text = std::fs::read_to_string(&path).ok();
    let config = home.map(settings::load).and_then(|s| s.spaces_layout);
    let path = path.to_string_lossy().into_owned();
    let layout = match layout::resolve(text.as_deref().map(|t| (path.as_str(), t)), config.as_ref())
    {
        Ok(l) => l,
        Err(warning) => return vec![warning],
    };
    let Some(root_pane) = herdr::created_pane_id(created) else {
        return vec!["herdr did not report the root pane; no layout applied".into()];
    };
    match layout::apply(call, &layout, dir, &root_pane) {
        Ok(()) => Vec::new(),
        Err(e) => vec![format!("layout not fully applied: {e}")],
    }
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
        .manage(client.clone())
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
            pane_split,
            pane_close,
            space_close,
            agent_start,
            agent_rename,
            agent_prompt,
            pane_rename,
            terminal_create,
            plugin_actions,
            plugin_invoke,
            plugin_pane_defs,
            deliver_board,
            plugin_pane_open,
            plugin_pane_focus,
            plugin_pane_close,
            create_space,
            list_dirs,
            list_child_dirs,
            app_version,
            get_settings,
            get_color_schemes,
            set_theme,
            set_color_schema,
            set_sidebar_layout,
            set_explorer_roots,
            set_explorer_show_hidden,
            set_spaces_layout,
            remove_spaces_layout,
            default_spaces_layout,
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
mod plugin_state_tests {
    use super::plugin_state_dir_in;
    use std::path::Path;

    #[test]
    fn plugin_state_lives_under_the_poimen_config_of_the_home() {
        let expected = Path::new("/h")
            .join(".config")
            .join("poimen")
            .join("plugin-state")
            .join("deliver-board");
        assert_eq!(
            plugin_state_dir_in(Path::new("/h"), "deliver-board"),
            expected
        );
    }
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
