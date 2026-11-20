//! Writers and commands of the `agents` settings the app changes (`agents.keepAwake`,
//! `agents.claude.classicRenderer`); the loader is `settings::resolve`.

use std::path::Path;

/// Returns `text` with `agents.keepAwake` set, touching nothing else.
pub fn set_keep_awake_text(text: &str, on: bool) -> Result<String, String> {
    poimen_core::settings::set_path_text(text, &["agents", "keepAwake"], &on.to_string())
}

/// Returns `text` with `agents.claude.classicRenderer` set, touching nothing else.
pub fn set_claude_classic_renderer_text(text: &str, on: bool) -> Result<String, String> {
    poimen_core::settings::set_path_text(
        text,
        &["agents", "claude", "classicRenderer"],
        &on.to_string(),
    )
}

/// Writes `agents.keepAwake` below `home`, like `save_theme`.
pub fn save_keep_awake(home: &Path, on: bool) -> Result<(), String> {
    poimen_core::settings::save_with(home, |text| set_keep_awake_text(text, on))
}

/// Writes `agents.claude.classicRenderer` below `home`, like `save_theme`.
pub fn save_claude_classic_renderer(home: &Path, on: bool) -> Result<(), String> {
    poimen_core::settings::save_with(home, |text| set_claude_classic_renderer_text(text, on))
}

/// Writes only `agents.claude.classicRenderer`; the settings watcher emits the change and applies
/// it to the backend for terminals created afterwards.
#[tauri::command]
pub fn set_claude_classic_renderer(on: bool) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    save_claude_classic_renderer(&home, on)
}

/// Writes only `agents.keepAwake` (#987); the server reads it from the file itself.
#[tauri::command]
pub fn set_keep_awake(on: bool) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    save_keep_awake(&home, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::resolve;

    fn rows(json: &str) -> crate::settings::Settings {
        resolve(Some(json), || None)
    }

    #[test]
    fn keep_awake_defaults_off_warns_when_invalid_and_is_written_alone() {
        assert!(!resolve(None, || None).keep_awake);
        assert!(rows(r#"{"agents":{"keepAwake":true}}"#).keep_awake);
        let bad = rows(r#"{"agents":{"keepAwake":"yes"}}"#);
        assert!(!bad.keep_awake);
        assert_eq!(bad.warnings.len(), 1);
        let out = set_keep_awake_text("{\"theme\": \"dark\"}", true).unwrap();
        assert!(rows(&out).keep_awake);
        assert!(out.contains("\"theme\""));
    }
}
