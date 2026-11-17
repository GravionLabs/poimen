//! Self-update from the latest GitHub release (`tauri-plugin-updater`, [docs/updater.md]).
//! It only runs when the user asks: `update_check` is the one network call poimen makes for it,
//! `update_install` downloads, verifies the signature, installs and restarts.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use poimen_client::protocol::{CHANNEL, Channel};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Whether a build of `channel` updates itself: stable only. An rc build has no updater artifacts
/// and no endpoint (`tauri.rc.conf.json`), the plugin is not registered, and the release test
/// checks updates on the stable app.
pub fn enabled(channel: Channel) -> bool {
    channel == Channel::Stable
}

const DISABLED: &str = "The RC build does not update itself; install the next RC by hand";

/// The update found by the last check, waiting to be installed.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// A newer release than the running app (wire shape of `core/updater.ts`).
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// Asks the release page whether a newer version exists; `None` when poimen is up to date.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, String> {
    if !enabled(CHANNEL) {
        return Err(DISABLED.into());
    }
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    *pending.0.lock().unwrap_or_else(|e| e.into_inner()) = update;
    Ok(info)
}

/// Downloads and installs the update of the last check, then restarts. A failure keeps the update
/// pending, so the install can be retried without a new check.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    if !enabled(CHANNEL) {
        return Err(DISABLED.into());
    }
    let update = pending.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(update) = update else {
        return Err("There is no update to install; check for updates first".into());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    relaunch(&app)
}

/// How to start the new version once this process has ended: a program and its arguments.
#[derive(Debug, PartialEq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// The `.app` bundle that holds `exe` (`<name>.app/Contents/MacOS/<binary>`), if it is in one.
fn bundle_of(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let in_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    in_bundle.then_some(bundle)
}

/// The helper that relaunches the app on macOS: it waits until `pid` has ended, then runs
/// `open <bundle>` without `-n`, so LaunchServices reuses the Dock tile of the running app and two
/// instances never run at once. Paths are arguments of the script, never part of its text.
/// `None` when `exe` is not inside a bundle (the caller falls back to `app.restart()`).
pub fn relaunch_command(exe: &Path, pid: u32) -> Option<Relaunch> {
    let bundle = bundle_of(exe)?;
    Some(Relaunch {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#.into(),
            "poimen-relaunch".into(),
            pid.to_string().into(),
            bundle.as_os_str().to_owned(),
        ],
    })
}

/// Starts the new version. On macOS a detached helper opens the bundle after this process has
/// ended (Tauri's own restart uses `open -n` while the old instance still runs, which gives a
/// second Dock icon); everywhere else, and outside a bundle, it is `app.restart()`. If the helper
/// cannot be started the app stays open and the error says what to do.
fn relaunch(app: &AppHandle) -> Result<(), String> {
    if cfg!(target_os = "macos")
        && let Ok(exe) = std::env::current_exe()
        && let Some(plan) = relaunch_command(&exe, std::process::id())
    {
        spawn_detached(&plan).map_err(|e| {
            format!("The update is installed, but poimen could not restart itself ({e}). Restart poimen by hand.")
        })?;
        app.exit(0);
        return Ok(());
    }
    app.restart()
}

/// Runs `plan` in its own process group without the stdio of this process.
fn spawn_detached(plan: &Relaunch) -> std::io::Result<()> {
    use std::process::Stdio;
    let mut command = poimen_core::gui_path::command(&plan.program);
    command
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_stable_builds_update_themselves() {
        assert!(enabled(Channel::Stable));
        assert!(!enabled(Channel::Rc));
    }

    #[test]
    fn the_relaunch_opens_the_bundle_without_a_new_instance() {
        let exe = Path::new("/Applications/poimen.app/Contents/MacOS/poimen");
        let plan = relaunch_command(exe, 4242).unwrap();
        assert_eq!(plan.program, Path::new("/bin/sh"));
        assert!(!plan.args[1].to_string_lossy().contains("-n"));
        assert_eq!(plan.args[3], OsString::from("4242"));
        assert_eq!(plan.args[4], OsString::from("/Applications/poimen.app"));
    }

    #[test]
    fn a_bundle_path_with_spaces_stays_one_argument() {
        let exe = Path::new("/Users/me/My Apps/poimen rc.app/Contents/MacOS/poimen-rc");
        let plan = relaunch_command(exe, 1).unwrap();
        assert_eq!(plan.args.len(), 5);
        assert_eq!(
            plan.args[4],
            OsString::from("/Users/me/My Apps/poimen rc.app")
        );
        assert!(!plan.args[1].to_string_lossy().contains("My Apps"));
    }

    #[test]
    fn an_executable_outside_a_bundle_has_no_relaunch() {
        assert_eq!(
            relaunch_command(Path::new("/usr/local/bin/poimen"), 1),
            None
        );
        assert_eq!(
            relaunch_command(Path::new("/x/target/debug/poimen"), 1),
            None
        );
        assert_eq!(
            relaunch_command(Path::new("/x/poimen.app/Contents/Other/poimen"), 1),
            None
        );
    }

    #[test]
    fn the_info_serializes_camel_case() {
        let info = UpdateInfo {
            version: "0.9.0".into(),
            current_version: "0.8.0".into(),
            notes: Some("notes".into()),
            date: None,
        };
        assert_eq!(
            serde_json::to_value(info).unwrap(),
            serde_json::json!({"version":"0.9.0","currentVersion":"0.8.0","notes":"notes","date":null})
        );
    }
}
