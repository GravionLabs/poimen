//! Tauri commands that write one list key of the user config after validating it.

/// Writes only `layouts` after validating it (named layouts, ADR 0011); a layout a template uses
/// cannot be dropped or renamed.
#[tauri::command]
pub fn set_layouts(layouts: serde_json::Value) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    poimen_core::settings::save_layouts(&home, &layouts)
}

/// Writes only `spaceTemplates` after validating it; the settings watcher emits the change.
#[tauri::command]
pub fn set_space_templates(templates: serde_json::Value) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    crate::settings::save_space_templates(&home, &templates)
}
