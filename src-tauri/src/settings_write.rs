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

/// Writes only `shortcuts.<action>` (the settings page "Keyboard"): `accelerator` null resets the
/// action to its default, an empty text unbinds it, other text is validated and refused when
/// another action has it. The settings watcher emits the change and rebuilds the menu.
#[tauri::command]
pub fn set_shortcut(action: String, accelerator: Option<String>) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    poimen_core::settings::save_with(&home, |text| {
        crate::settings::set_shortcut_text(text, &action, accelerator.as_deref())
    })
}

/// Revokes the trust of the project file of `root`; the settings watcher emits the change.
#[tauri::command]
pub fn revoke_project(root: String) -> Result<(), String> {
    let home = crate::home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    crate::settings::revoke_project(&home, &root)
}
