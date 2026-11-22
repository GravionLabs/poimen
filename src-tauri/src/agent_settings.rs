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

/// Returns `text` with `terminalHistory` set (#1196), touching nothing else.
pub fn set_terminal_history_text(
    text: &str,
    value: &poimen_core::terminal_history::TerminalHistory,
) -> Result<String, String> {
    use poimen_core::settings::set_path_text;
    let key = |name| ["terminalHistory", name];
    let text = set_path_text(text, &key("enabled"), &value.enabled.to_string())?;
    let text = set_path_text(
        &text,
        &key("perTerminalMiB"),
        &value.per_terminal_mib.to_string(),
    )?;
    set_path_text(&text, &key("totalMiB"), &value.total_mib.to_string())
}

/// Writes only `terminalHistory` (#1196); the server reads it from the file itself.
#[tauri::command]
pub fn set_terminal_history(
    enabled: bool,
    per_terminal_mib: u64,
    total_mib: u64,
) -> Result<(), String> {
    let range = 1..=poimen_core::terminal_history::MAX_MIB;
    if !range.contains(&per_terminal_mib) || !range.contains(&total_mib) {
        return Err(format!(
            "the limits must be whole numbers of MiB from 1 to {}",
            poimen_core::terminal_history::MAX_MIB
        ));
    }
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    let value = poimen_core::terminal_history::TerminalHistory {
        enabled,
        per_terminal_mib,
        total_mib,
    };
    poimen_core::settings::save_with(&home, |text| set_terminal_history_text(text, &value))
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

    #[test]
    fn terminal_history_defaults_on_and_is_written_alone() {
        let d = rows("{}").terminal_history;
        assert_eq!((d.enabled, d.per_terminal_mib, d.total_mib), (true, 8, 256));
        let s = rows(r#"{"terminalHistory":{"enabled":false,"perTerminalMiB":4,"totalMiB":64}}"#);
        assert_eq!(
            (
                s.terminal_history.enabled,
                s.terminal_history.per_terminal_mib,
                s.terminal_history.total_mib
            ),
            (false, 4, 64)
        );
        assert_eq!(
            rows(r#"{"terminalHistory":{"totalMiB":0}}"#).warnings.len(),
            1
        );
        let out = set_terminal_history_text(
            "{\"theme\": \"dark\"}",
            &poimen_core::terminal_history::TerminalHistory {
                enabled: false,
                per_terminal_mib: 2,
                total_mib: 9,
            },
        )
        .unwrap();
        let back = resolve(Some(&out), || None);
        assert_eq!(back.theme, "dark");
        assert_eq!(
            (
                back.terminal_history.enabled,
                back.terminal_history.per_terminal_mib,
                back.terminal_history.total_mib
            ),
            (false, 2, 9)
        );
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    }

    #[test]
    fn terminal_history_is_imported_and_an_invalid_field_is_rejected() {
        use crate::settings_transfer::{Mode, Scope, plan};
        let obj = |v: serde_json::Value| v.as_object().unwrap().clone();
        let file = obj(serde_json::json!({
            "terminalHistory": { "enabled": false, "perTerminalMiB": 4, "totalMiB": 64 }
        }));
        let (p, after) = plan(&Default::default(), &file, Mode::Merge, Scope::All);
        assert!(p.invalid.is_empty(), "{:?}", p.invalid);
        assert_eq!(p.added.len(), 1);
        assert_eq!(after["terminalHistory"], file["terminalHistory"]);
        let bad = obj(serde_json::json!({ "terminalHistory": { "perTerminalMiB": 0 } }));
        let (p, after) = plan(&Default::default(), &bad, Mode::Merge, Scope::All);
        assert_eq!(p.invalid.len(), 1);
        assert_eq!(p.invalid[0].path, "terminalHistory.perTerminalMiB");
        assert!(!after.contains_key("terminalHistory"));
    }
}
