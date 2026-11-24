//! The Test button of Settings > Notifications (#1017): runs the configured `notify` hook of a
//! status with a sample message, here in the app (the server runs the real ones).

use std::path::Path;
use std::time::Duration;

use poimen_core::notify;

/// Runs the hook of `status` (`blocked` or `done`) from the config below `home` with a test
/// message; the error says what failed or that nothing is configured.
pub fn run_test(home: &Path, status: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(poimen_core::settings::config_path(home))
        .map_err(|_| "there is no config.json with a notify setting".to_string())?;
    let root: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("config.json is not valid: {e}"))?;
    let setting = notify::from_root(&root, &mut Vec::new());
    let hook = setting
        .hook_for(status)
        .ok_or_else(|| format!("notify.{status} has no command or url"))?;
    let message = notify::test_message(status, &notify::machine_name());
    notify::run(hook, &message, Duration::from_secs(setting.timeout_seconds))
}

/// Runs the configured notification of `status` once with a test text.
#[tauri::command]
pub async fn notify_test(status: String) -> Result<(), String> {
    if status != "blocked" && status != "done" {
        return Err("the status must be blocked or done".into());
    }
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    tauri::async_runtime::spawn_blocking(move || run_test(&home, &status))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;

    fn home(name: &str, config: &str) -> TestDir {
        let home = TestDir::new(&format!("poimen-notify-test-{name}-"));
        let path = poimen_core::settings::config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, config).unwrap();
        home
    }

    #[test]
    fn nothing_configured_is_an_error_that_says_so() {
        let h = home("none", "{}");
        let e = run_test(&h, "done").unwrap_err();
        assert!(e.contains("notify.done"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn a_configured_command_runs_with_the_test_message() {
        let out_dir = TestDir::new("poimen-notify-test-out-");
        let out = out_dir.join("out");
        let config = serde_json::json!({ "notify": { "done": { "command":
            ["sh", "-c", format!("echo \"$0\" > '{}'", out.display()), "{agent}:{status}"] } } });
        let h = home("run", &config.to_string());
        run_test(&h, "done").unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "test:done");
        let bad =
            serde_json::json!({ "notify": { "blocked": { "command": ["sh", "-c", "exit 2"] } } });
        let h = home("bad", &bad.to_string());
        assert!(run_test(&h, "blocked").unwrap_err().contains("failed"));
    }
}
