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
