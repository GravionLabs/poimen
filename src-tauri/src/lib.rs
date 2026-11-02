mod git;
mod herdr;
mod layout;
#[cfg(test)]
mod live;
mod schemes;
mod settings;
mod spaces;
mod stream;
mod watch;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use herdr::Client;
use stream::{Mode, OpenRequest, Sessions};

struct HerdrBin(PathBuf);

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

/// The running poimen build's version (Tauri app version).
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
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
    let home = home_dir().ok_or("HOME is not set")?;
    settings::save_theme(&home, &theme)
}

/// Branch and worktree flag per directory (read-only `git rev-parse`).
#[tauri::command]
async fn repo_info(
    dirs: Vec<String>,
) -> Result<std::collections::HashMap<String, git::RepoInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || git::infos(dirs))
        .await
        .map_err(|e| e.to_string())
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
            &item("nextWaiting", "Next Waiting Agent")?,
            &sep()?,
            &item("nextTab", "Next Tab")?,
            &item("previousTab", "Previous Tab")?,
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
    let bin = stream::resolve_bin(|key| std::env::var(key).ok(), |path| path.is_file());
    let client = Client::from_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(client.clone())
        .manage(Sessions::default())
        .manage(HerdrBin(bin))
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
            pane_split,
            pane_close,
            agent_start,
            agent_rename,
            pane_rename,
            terminal_create,
            create_space,
            list_dirs,
            list_child_dirs,
            app_version,
            get_settings,
            get_color_schemes,
            set_theme,
            repo_info,
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
