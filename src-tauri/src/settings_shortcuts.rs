//! Shortcut table of the settings: actions, default accelerators and the validation of overrides.

use std::collections::BTreeMap;

use serde_json::Value;

/// Menu/shortcut actions with their default accelerators (VS Code style). `CmdOrCtrl` is Cmd on
/// macOS and Ctrl elsewhere.
pub const ACTIONS: &[(&str, &str)] = &[
    ("newTerminal", "CmdOrCtrl+T"),
    ("closeTab", "CmdOrCtrl+W"),
    ("closeAll", "CmdOrCtrl+Shift+W"),
    ("refresh", "CmdOrCtrl+R"),
    ("toggleSidebar", "CmdOrCtrl+B"),
    ("toggleOutput", "CmdOrCtrl+J"),
    ("splitRight", "CmdOrCtrl+\\"),
    ("splitDown", "CmdOrCtrl+Shift+\\"),
    ("zoomIn", "CmdOrCtrl+="),
    ("zoomOut", "CmdOrCtrl+-"),
    ("zoomReset", "CmdOrCtrl+0"),
    ("commandPalette", "CmdOrCtrl+Shift+P"),
    ("jump", "CmdOrCtrl+K"),
    ("nextWaiting", "CmdOrCtrl+Shift+A"),
    ("nextTab", "CmdOrCtrl+Alt+Right"),
    ("previousTab", "CmdOrCtrl+Alt+Left"),
    ("recentTab", "Ctrl+Tab"),
    ("recentTabBack", "Ctrl+Shift+Tab"),
    ("showShortcuts", "CmdOrCtrl+/"),
    ("find", "CmdOrCtrl+Shift+F"),
];

pub fn default_shortcuts() -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|(id, acc)| ((*id).to_owned(), (*acc).to_owned()))
        .collect()
}

/// Canonical form of an accelerator such as `cmd+shift+w` -> `Cmd+Shift+W`; `None` when invalid.
/// A shortcut needs a Cmd, Ctrl or Alt modifier, or is a function key.
pub fn normalize_accelerator(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last()?;
    let (mut cmd_or_ctrl, mut cmd, mut ctrl, mut alt, mut shift) =
        (false, false, false, false, false);
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "cmdorctrl" | "commandorcontrol" | "mod" => cmd_or_ctrl = true,
            "cmd" | "command" | "meta" | "super" => cmd = true,
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "tab" => Some("Tab"),
        "enter" | "return" => Some("Enter"),
        "space" => Some("Space"),
        "escape" | "esc" => Some("Escape"),
        "left" | "arrowleft" => Some("Left"),
        "right" | "arrowright" => Some("Right"),
        "up" | "arrowup" => Some("Up"),
        "down" | "arrowdown" => Some("Down"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "backspace" => Some("Backspace"),
        "delete" => Some("Delete"),
        "plus" => Some("Plus"),
        _ => None,
    };
    let function_key = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n));
    let key = if let Some(n) = named {
        n.to_owned()
    } else if function_key {
        lower.to_uppercase()
    } else if key.chars().count() == 1 && !key.chars().all(char::is_whitespace) && *key != "+" {
        key.to_uppercase()
    } else {
        return None;
    };
    if !(cmd_or_ctrl || cmd || ctrl || alt || function_key) {
        return None;
    }
    let mut out = String::new();
    for (on, name) in [
        (cmd_or_ctrl, "CmdOrCtrl"),
        (cmd, "Cmd"),
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
    ] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&key);
    Some(out)
}

/// Defaults overlaid with the `shortcuts` object; unknown actions, invalid accelerators and
/// accelerators already used by another action warn and keep the default.
pub(crate) fn resolve_shortcuts(
    value: &Value,
    warnings: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut out = default_shortcuts();
    let Some(map) = value.as_object() else {
        warnings.push("shortcuts must be an object".into());
        return out;
    };
    for key in map.keys() {
        if !ACTIONS.iter().any(|(id, _)| id == key) {
            warnings.push(format!("shortcuts.{key} is not a known action; ignored"));
        }
    }
    let mut chosen: Vec<(&str, &str, String)> = Vec::new();
    for (id, default) in ACTIONS {
        let mut acc = (*default).to_owned();
        match map.get(*id) {
            None => {}
            Some(Value::String(s)) if s.trim().is_empty() => acc = String::new(),
            Some(Value::String(s)) => match normalize_accelerator(s) {
                Some(n) => acc = n,
                None => warnings.push(format!(
                    "shortcuts.{id} \"{s}\" is not a valid shortcut; using {default}"
                )),
            },
            Some(other) => warnings.push(format!(
                "shortcuts.{id} {other} must be a string; using {default}"
            )),
        }
        chosen.push((id, default, acc));
    }
    // An override that collides with another action's shortcut loses to the default.
    let all: Vec<String> = chosen.iter().map(|(_, _, a)| a.clone()).collect();
    for (id, default, acc) in chosen {
        let clash = !acc.is_empty() && all.iter().filter(|a| **a == acc).count() > 1;
        if clash && acc != default {
            warnings.push(format!(
                "shortcuts.{id} {acc} is already used by another action; using {default}"
            ));
            out.insert(id.to_owned(), default.to_owned());
        } else {
            out.insert(id.to_owned(), acc);
        }
    }
    out
}

/// Returns `text` with the one entry `shortcuts.<action>` changed, touching nothing else. `None`
/// resets the action (removes the entry, so the default applies), `Some("")` unbinds it, any other
/// text is validated like the loader does (a modifier is needed) and refused when another action
/// has it already. An action that already has the effective value leaves the text unchanged.
pub fn set_shortcut_text(
    text: &str,
    action: &str,
    accelerator: Option<&str>,
) -> Result<String, String> {
    use poimen_core::settings::{remove_path_text, set_path_text};
    if !ACTIONS.iter().any(|(id, _)| *id == action) {
        return Err(format!("{action} is not a known shortcut action"));
    }
    let path = ["shortcuts", action];
    match accelerator {
        None => {
            let default = ACTIONS
                .iter()
                .find(|(id, _)| *id == action)
                .map(|(_, d)| *d);
            let current = super::resolve(Some(text), || None).shortcuts;
            if let Some((other, _)) = current
                .iter()
                .find(|(id, v)| *id != action && Some(v.as_str()) == default)
            {
                return Err(format!(
                    "{} is already used by {other}",
                    default.unwrap_or_default()
                ));
            }
            remove_path_text(text, &path)
        }
        Some(a) if a.trim().is_empty() => set_path_text(text, &path, "\"\""),
        Some(a) => {
            let acc = normalize_accelerator(a).ok_or_else(|| {
                format!("\"{a}\" is not a valid shortcut (it needs Cmd, Ctrl or Alt, or is a function key)")
            })?;
            let current = super::resolve(Some(text), || None).shortcuts;
            if let Some((other, _)) = current.iter().find(|(id, v)| *id != action && **v == acc) {
                return Err(format!("{acc} is already used by {other}"));
            }
            set_path_text(text, &path, &Value::String(acc).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shortcuts(text: &str) -> BTreeMap<String, String> {
        super::super::resolve(Some(text), || None).shortcuts
    }

    #[test]
    fn set_writes_only_that_entry_and_keeps_the_rest() {
        let text = "{\n  // keep\n  \"theme\": \"dark\"\n}\n";
        let out = set_shortcut_text(text, "newTerminal", Some("cmd+shift+n")).unwrap();
        assert!(
            out.contains("// keep") && out.contains("\"theme\": \"dark\""),
            "{out}"
        );
        let v: Value = serde_json::from_str(&out.replace("// keep", "")).unwrap();
        assert_eq!(v["shortcuts"], json!({ "newTerminal": "Cmd+Shift+N" }));
    }

    #[test]
    fn a_second_entry_keeps_the_first() {
        let a = set_shortcut_text("{}", "newTerminal", Some("Ctrl+Alt+N")).unwrap();
        let b = set_shortcut_text(&a, "closeTab", Some("Ctrl+Alt+X")).unwrap();
        let s = shortcuts(&b);
        assert_eq!(
            (s["newTerminal"].as_str(), s["closeTab"].as_str()),
            ("Ctrl+Alt+N", "Ctrl+Alt+X")
        );
    }

    #[test]
    fn a_shortcut_without_a_modifier_or_a_taken_one_is_refused() {
        assert!(set_shortcut_text("{}", "newTerminal", Some("N")).is_err());
        assert!(set_shortcut_text("{}", "newTerminal", Some("Shift+N")).is_err());
        let err = set_shortcut_text("{}", "newTerminal", Some("CmdOrCtrl+W")).unwrap_err();
        assert!(err.contains("closeTab"), "{err}");
        assert!(set_shortcut_text("{}", "nope", Some("Ctrl+N")).is_err());
        // A function key needs no modifier, like the loader.
        assert!(set_shortcut_text("{}", "newTerminal", Some("F5")).is_ok());
    }

    #[test]
    fn clear_unbinds_and_reset_restores_the_default() {
        let cleared = set_shortcut_text("{}", "closeTab", Some("")).unwrap();
        assert_eq!(shortcuts(&cleared)["closeTab"], "");
        // The freed accelerator can go to another action.
        let moved = set_shortcut_text(&cleared, "newTerminal", Some("CmdOrCtrl+W")).unwrap();
        assert_eq!(shortcuts(&moved)["newTerminal"], "CmdOrCtrl+W");
        let reset = set_shortcut_text(&cleared, "closeTab", None).unwrap();
        assert_eq!(shortcuts(&reset)["closeTab"], "CmdOrCtrl+W");
        assert_eq!(set_shortcut_text("{}", "closeTab", None).unwrap(), "{}");
        // Reset is refused while another action holds the default.
        let taken = set_shortcut_text(&cleared, "newTerminal", Some("CmdOrCtrl+W")).unwrap();
        let err = set_shortcut_text(&taken, "closeTab", None).unwrap_err();
        assert!(err.contains("newTerminal"), "{err}");
    }
}
