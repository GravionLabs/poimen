//! Self-update from the latest GitHub release (`tauri-plugin-updater`, [docs/updater.md]).
//! It only runs when the user asks: `update_check` is the one network call poimen makes for it,
//! `update_install` downloads, verifies the signature, installs and restarts.

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
    app.restart()
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
