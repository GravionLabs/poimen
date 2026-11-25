//! Schedules in the app (#1000): the warnings of `schedules[]` and `triggers` of the user config,
//! `schedule.list` of the local server for the settings and the Runs view, and the switches of the
//! settings page.

use poimen_client::ClientError;
use poimen_client::protocol::{ErrorCode, ScheduleAckAlerts, ScheduleAlerts, ScheduleList};
use poimen_core::settings::parse_tasks;
use serde_json::Value;

use crate::settings::Settings;

/// `tasks` of the user config: `modes[]` and `worktreeDir` over the defaults.
pub fn resolve_tasks(value: &Value, s: &mut Settings) {
    let parsed = parse_tasks(value, "using the default agent mode", &mut s.warnings);
    if let Some(dir) = parsed.worktree_dir {
        s.tasks.worktree_dir = dir;
    }
    if let Some(modes) = parsed.modes {
        s.tasks.modes = modes;
    }
}

/// Adds the warnings of the schedules of the user config `root`: an entry that is invalid, or
/// names a task mode that cannot be scheduled.
pub fn check(root: &Value, warnings: &mut Vec<String>) {
    poimen_core::schedules::parse(root, crate::home_dir().as_deref(), warnings);
}

/// The schedules with their next run, last result and runs, as the local poimen-server has them.
#[tauri::command]
pub async fn schedules_list() -> Result<ScheduleList, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let result = poimen_client::Client::default()
            .request("schedule.list", serde_json::json!({}))
            .map_err(|e| e.to_string())?;
        serde_json::from_value(result).map_err(|e| format!("schedule.list: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The failed and skipped runs no app has acknowledged yet (#1308); none from an older server.
#[tauri::command]
pub async fn schedules_alerts() -> Result<ScheduleAlerts, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let client = poimen_client::Client::default();
        alerts_of(client.request("schedule.alerts", serde_json::json!({})))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn alerts_of(answer: Result<Value, ClientError>) -> Result<ScheduleAlerts, String> {
    match answer {
        Ok(result) => serde_json::from_value(result).map_err(|e| format!("schedule.alerts: {e}")),
        Err(ClientError::Server {
            code: ErrorCode::UnknownMethod,
            ..
        }) => Ok(ScheduleAlerts::default()),
        Err(e) => Err(e.to_string()),
    }
}

/// Removes the alerts up to the `seq` the app has added to its notifications.
#[tauri::command]
pub async fn schedules_ack_alerts(up_to: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        poimen_client::Client::default()
            .request(
                "schedule.ack_alerts",
                serde_json::json!(ScheduleAckAlerts { up_to }),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Switches the schedule `key` on or off in the user config.
#[tauri::command]
pub fn set_schedule_enabled(key: String, enabled: bool) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    poimen_core::settings::save_with(&home, |text| {
        poimen_core::schedules::set_enabled_text(text, Some(&home), &key, enabled)
    })
}

/// Sets the global pause of every schedule (`triggers.paused`).
#[tauri::command]
pub fn set_schedules_paused(paused: bool) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    poimen_core::settings::save_with(&home, |text| {
        poimen_core::schedules::set_paused_text(text, paused)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_server_has_no_alerts_and_other_errors_are_reported() {
        let error = |code| ClientError::Server {
            code,
            message: "no".into(),
        };
        let none = alerts_of(Err(error(ErrorCode::UnknownMethod)));
        assert_eq!(none, Ok(ScheduleAlerts::default()));
        assert!(alerts_of(Err(error(ErrorCode::Internal))).is_err());
        assert!(alerts_of(Ok(serde_json::json!({"alerts": 1}))).is_err());
        let one = alerts_of(Ok(
            serde_json::json!({"alerts": [{"id": 1, "seq": 2, "key": "k",
            "dir": "/r", "mode": "job", "outcome": "failed", "at_ms": 3, "count": 1}]}),
        ));
        assert_eq!(one.unwrap().alerts[0].seq, 2);
    }

    #[test]
    fn the_settings_warn_about_a_schedule_whose_mode_cannot_be_scheduled() {
        let config = r#"{"tasks":{"modes":[{"name":"work"}]},
            "schedules":[{"cron":"0 9 * * *","timezone":"UTC","dir":"~/r","mode":"work"}]}"#;
        let warnings = crate::settings::resolve(Some(config), || None).warnings;
        assert_eq!(
            warnings,
            [r#"schedules[0] (mode "work" in ~/r): the task mode "work" has no command; ignored"#]
        );
    }
}
