//! Schedules in the app (#1000): the warnings of `schedules[]` and `triggers` of the user config,
//! `schedule.list` of the local server for the settings and the Runs view, and the switches of the
//! settings page.

use poimen_client::protocol::ScheduleList;
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
