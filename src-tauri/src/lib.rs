mod herdr;
#[cfg(test)]
mod live;
mod stream;
mod watch;

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
fn close_terminal(sessions: State<'_, Sessions>, id: u32) {
    sessions.close(id);
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let view = Submenu::with_items(
        app,
        "Panes",
        true,
        &[
            &MenuItem::with_id(
                app,
                "expand_all",
                "Expand All",
                true,
                Some("CmdOrCtrl+Shift+E"),
            )?,
            &MenuItem::with_id(
                app,
                "collapse_all",
                "Collapse All",
                true,
                Some("CmdOrCtrl+Shift+C"),
            )?,
            &MenuItem::with_id(
                app,
                "close_all",
                "Close All Views",
                true,
                Some("CmdOrCtrl+Shift+W"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "refresh", "Refresh", true, Some("CmdOrCtrl+R"))?,
        ],
    )?;
    menu.append(&view)?;
    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let bin = stream::resolve_bin(|key| std::env::var(key).ok(), |path| path.is_file());
    let client = Client::from_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(client.clone())
        .manage(Sessions::default())
        .manage(HerdrBin(bin))
        .setup(move |app| {
            let handle = app.handle().clone();
            app.set_menu(build_menu(&handle)?)?;
            app.on_menu_event(|app, event| {
                let _ = app.emit("menu", event.id().0.clone());
            });
            std::thread::spawn(move || {
                watch::run(client, |event| {
                    let _ = handle.emit("herdr", event);
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            open_terminal,
            terminal_input,
            terminal_resize,
            terminal_scroll,
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
