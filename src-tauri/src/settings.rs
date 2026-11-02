//! User settings file (`~/.config/poimen/config.json`) and the optional Ghostty config.
//!
//! Preferences only: UI state (sidebar width, open tabs) lives in the frontend UI state.
//! Invalid entries produce a warning and fall back to the default. The only write is `save_theme`
//! (the toolbar button), which edits the `theme` value in place and keeps everything else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

pub const DEFAULT_FONT_SIZE: f64 = 13.0;
const MIN_FONT_SIZE: f64 = 6.0;
const MAX_FONT_SIZE: f64 = 72.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub background: Option<String>,
    pub foreground: Option<String>,
}

/// The resolved settings sent to the frontend (event `settings`, command `get_settings`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// `light`, `dark` or `system`.
    pub theme: String,
    /// `None` keeps the app's default monospace stack.
    pub font_family: Option<String>,
    pub font_size: f64,
    /// Terminal colors taken from Ghostty; `None` keeps the theme colors.
    pub colors: Option<Colors>,
    /// Open panes for input when no other client controls them (`terminal.autoControl`).
    pub auto_control: bool,
    /// Accelerator per action id (see `ACTIONS`), defaults merged with the valid `shortcuts`
    /// overrides; an empty string means unbound.
    pub shortcuts: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

/// Menu/shortcut actions with their default accelerators (VS Code style). `CmdOrCtrl` is Cmd on
/// macOS and Ctrl elsewhere.
pub const ACTIONS: &[(&str, &str)] = &[
    ("newTerminal", "CmdOrCtrl+T"),
    ("closeTab", "CmdOrCtrl+W"),
    ("closeAll", "CmdOrCtrl+Shift+W"),
    ("refresh", "CmdOrCtrl+R"),
    ("toggleSidebar", "CmdOrCtrl+B"),
    ("splitRight", "CmdOrCtrl+\\"),
    ("splitDown", "CmdOrCtrl+Shift+\\"),
    ("zoomIn", "CmdOrCtrl+="),
    ("zoomOut", "CmdOrCtrl+-"),
    ("zoomReset", "CmdOrCtrl+0"),
    ("nextWaiting", "CmdOrCtrl+Shift+A"),
    ("nextTab", "CmdOrCtrl+Alt+Right"),
    ("previousTab", "CmdOrCtrl+Alt+Left"),
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
fn resolve_shortcuts(value: &Value, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            colors: None,
            auto_control: true,
            shortcuts: default_shortcuts(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Ghostty {
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub background: Option<String>,
    pub foreground: Option<String>,
}

fn valid_size(size: f64) -> bool {
    size.is_finite() && (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size)
}

fn hex_color(value: &str) -> Option<String> {
    let hex = value.trim().trim_start_matches('#');
    (matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{hex}"))
}

/// Reads the keys poimen uses from a Ghostty config (`key = value`, `#` comments).
pub fn parse_ghostty(text: &str) -> Ghostty {
    let mut out = Ghostty::default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            // The first family wins; later ones are fallbacks.
            "font-family" if out.font_family.is_none() && !value.is_empty() => {
                out.font_family = Some(value.to_owned());
            }
            "font-size" => out.font_size = value.parse().ok().filter(|s| valid_size(*s)),
            "background" => out.background = hex_color(value),
            "foreground" => out.foreground = hex_color(value),
            _ => {}
        }
    }
    out
}

/// Resolves the settings from the file contents. `read_ghostty` is only called when the
/// `terminal.ghostty` flag is set.
pub fn resolve(json: Option<&str>, read_ghostty: impl FnOnce() -> Option<String>) -> Settings {
    let mut s = Settings::default();
    let Some(json) = json else { return s };
    let root: Value = match serde_json::from_str(json) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            s.warnings
                .push("config.json is not a valid JSON object; using defaults".into());
            return s;
        }
    };

    match root.get("theme") {
        None => {}
        Some(Value::String(t)) if matches!(t.as_str(), "light" | "dark" | "system") => {
            s.theme = t.clone();
        }
        Some(other) => s.warnings.push(format!(
            "theme {other} must be \"light\", \"dark\" or \"system\""
        )),
    }

    if let Some(value) = root.get("shortcuts") {
        s.shortcuts = resolve_shortcuts(value, &mut s.warnings);
    }

    let mut use_ghostty = false;
    let mut explicit_family = false;
    let mut explicit_size = false;
    if let Some(term) = root.get("terminal") {
        if let Some(term) = term.as_object() {
            match term.get("fontFamily") {
                None => {}
                Some(Value::String(f)) if !f.trim().is_empty() => {
                    s.font_family = Some(f.trim().to_owned());
                    explicit_family = true;
                }
                Some(other) => s.warnings.push(format!(
                    "terminal.fontFamily {other} must be a non-empty string"
                )),
            }
            if let Some(v) = term.get("fontSize") {
                match v.as_f64().filter(|n| valid_size(*n)) {
                    Some(n) => {
                        s.font_size = n;
                        explicit_size = true;
                    }
                    None => s.warnings.push(format!(
                        "terminal.fontSize {v} must be a number from {MIN_FONT_SIZE} to {MAX_FONT_SIZE}"
                    )),
                }
            }
            match term.get("autoControl") {
                None => {}
                Some(Value::Bool(b)) => s.auto_control = *b,
                Some(other) => s.warnings.push(format!(
                    "terminal.autoControl {other} must be true or false"
                )),
            }
            match term.get("ghostty") {
                None => {}
                Some(Value::Bool(b)) => use_ghostty = *b,
                Some(other) => s
                    .warnings
                    .push(format!("terminal.ghostty {other} must be true or false")),
            }
        } else {
            s.warnings.push("terminal must be an object".into());
        }
    }

    if use_ghostty {
        match read_ghostty() {
            None => s
                .warnings
                .push("Ghostty config not found; ignoring terminal.ghostty".into()),
            Some(text) => {
                let g = parse_ghostty(&text);
                // Explicit poimen entries win over Ghostty.
                if !explicit_family && g.font_family.is_some() {
                    s.font_family = g.font_family;
                }
                if !explicit_size && let Some(size) = g.font_size {
                    s.font_size = size;
                }
                if g.background.is_some() || g.foreground.is_some() {
                    s.colors = Some(Colors {
                        background: g.background,
                        foreground: g.foreground,
                    });
                }
            }
        }
    }
    s
}

pub fn config_path(home: &Path) -> PathBuf {
    home.join(".config/poimen/config.json")
}

pub fn ghostty_path(home: &Path) -> PathBuf {
    home.join(".config/ghostty/config")
}

/// Reads and resolves the settings below `home`; a missing file means defaults.
pub fn load(home: &Path) -> Settings {
    let json = match std::fs::read_to_string(config_path(home)) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            let mut s = Settings::default();
            s.warnings.push(format!("cannot read config.json: {e}"));
            return s;
        }
    };
    resolve(json.as_deref(), || {
        std::fs::read_to_string(ghostty_path(home)).ok()
    })
}

/// Index after a `"..."` string that starts at `i`.
fn skip_string(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// Index after whitespace and `//` / `/* */` comments starting at `i`.
fn skip_trivia(b: &[u8], mut i: usize) -> usize {
    loop {
        match b.get(i) {
            Some(c) if c.is_ascii_whitespace() => i += 1,
            Some(b'/') if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            Some(b'/') if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !b[i..].starts_with(b"*/") {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            _ => return i,
        }
    }
}

/// Returns `text` with the top-level `theme` set to `theme`, touching nothing else: other keys,
/// comments and formatting stay. A missing `theme` is inserted as the first entry.
pub fn set_theme_text(text: &str, theme: &str) -> Result<String, String> {
    if !matches!(theme, "light" | "dark" | "system") {
        return Err(format!("invalid theme {theme:?}"));
    }
    let b = text.as_bytes();
    let open = skip_trivia(b, 0);
    if b.get(open) != Some(&b'{') {
        return Err("config.json is not a JSON object".into());
    }
    let (mut i, mut depth) = (open + 1, 1usize);
    while i < b.len() {
        i = skip_trivia(b, i);
        match b.get(i) {
            None => break,
            Some(b'"') => {
                let end = skip_string(b, i);
                let is_key = depth == 1 && &text[i..end] == "\"theme\"";
                i = end;
                if is_key {
                    let colon = skip_trivia(b, i);
                    if b.get(colon) == Some(&b':') {
                        let start = skip_trivia(b, colon + 1);
                        let stop = if b.get(start) == Some(&b'"') {
                            skip_string(b, start)
                        } else {
                            let mut e = start;
                            while e < b.len() && !matches!(b[e], b',' | b'}' | b'\n') {
                                e += 1;
                            }
                            e
                        };
                        return Ok(format!("{}\"{theme}\"{}", &text[..start], &text[stop..]));
                    }
                }
            }
            Some(b'{' | b'[') => {
                depth += 1;
                i += 1;
            }
            Some(b'}' | b']') => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            Some(_) => i += 1,
        }
    }
    let first = skip_trivia(b, open + 1);
    let entry = format!("\"theme\": \"{theme}\"");
    Ok(if b.get(first) == Some(&b'}') {
        format!("{}{entry}{}", &text[..=open], &text[first..])
    } else {
        format!("{}\n  {entry},{}", &text[..=open], &text[open + 1..])
    })
}

/// Writes the `theme` setting below `home` (creating the file when missing). The watcher then
/// emits the new settings like for any external edit.
pub fn save_theme(home: &Path, theme: &str) -> Result<(), String> {
    let path = config_path(home);
    let next = match std::fs::read_to_string(&path) {
        Ok(text) => set_theme_text(&text, theme)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            set_theme_text("{}", theme).map(|t| t + "\n")?
        }
        Err(e) => return Err(format!("cannot read config.json: {e}")),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, next).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn stamp(home: &Path) -> [Option<SystemTime>; 2] {
    [config_path(home), ghostty_path(home)]
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Runs forever on the calling thread: polls both files and calls `emit` on every change.
pub fn watch(home: &Path, interval: Duration, emit: impl Fn(&Settings)) {
    let mut last = stamp(home);
    loop {
        std::thread::sleep(interval);
        let now = stamp(home);
        if now != last {
            last = now;
            emit(&load(home));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Option<String> {
        None
    }

    #[test]
    fn missing_file_gives_defaults() {
        assert_eq!(resolve(None, none), Settings::default());
    }

    #[test]
    fn reads_valid_entries() {
        let s = resolve(
            Some(r#"{"theme":"dark","terminal":{"fontFamily":"Fira Code","fontSize":15}}"#),
            none,
        );
        assert_eq!(s.theme, "dark");
        assert_eq!(s.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(s.font_size, 15.0);
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn auto_control_defaults_to_true_and_can_be_turned_off() {
        assert!(resolve(None, none).auto_control);
        assert!(resolve(Some(r#"{"terminal":{}}"#), none).auto_control);
        let off = resolve(Some(r#"{"terminal":{"autoControl":false}}"#), none);
        assert!(!off.auto_control);
        assert!(off.warnings.is_empty());
        let bad = resolve(Some(r#"{"terminal":{"autoControl":"no"}}"#), none);
        assert!(bad.auto_control);
        assert_eq!(bad.warnings.len(), 1);
    }

    #[test]
    fn invalid_entries_warn_and_fall_back() {
        let s = resolve(
            Some(r#"{"theme":"blue","terminal":{"fontFamily":3,"fontSize":500,"ghostty":"yes"}}"#),
            none,
        );
        assert_eq!(s.theme, "system");
        assert_eq!(s.font_family, None);
        assert_eq!(s.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(s.warnings.len(), 4);
    }

    #[test]
    fn broken_json_warns() {
        for bad in ["{nope", "[1]"] {
            let s = resolve(Some(bad), none);
            assert_eq!(s.theme, "system");
            assert_eq!(s.warnings.len(), 1);
        }
    }

    #[test]
    fn parses_ghostty_config() {
        let g = parse_ghostty(
            "# comment\nfont-family = \"JetBrains Mono\"\nfont-family = Menlo\nfont-size = 14\nbackground = #1e1e2e\nforeground = cdd6f4\nbad line\n",
        );
        assert_eq!(g.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(g.font_size, Some(14.0));
        assert_eq!(g.background.as_deref(), Some("#1e1e2e"));
        assert_eq!(g.foreground.as_deref(), Some("#cdd6f4"));
        assert_eq!(
            parse_ghostty("background = red\nfont-size = x"),
            Ghostty::default()
        );
    }

    #[test]
    fn ghostty_fills_gaps_but_explicit_wins() {
        let ghostty =
            || Some("font-family = Hack\nfont-size = 16\nbackground = #000000".to_owned());
        let s = resolve(
            Some(r#"{"terminal":{"ghostty":true,"fontSize":12}}"#),
            ghostty,
        );
        assert_eq!(s.font_family.as_deref(), Some("Hack"));
        assert_eq!(s.font_size, 12.0);
        assert_eq!(
            s.colors,
            Some(Colors {
                background: Some("#000000".into()),
                foreground: None
            })
        );
    }

    #[test]
    fn ghostty_ignored_unless_requested_and_warns_when_missing() {
        let never = || -> Option<String> { panic!("must not read") };
        assert_eq!(resolve(Some(r#"{"theme":"light"}"#), never).colors, None);
        let s = resolve(Some(r#"{"terminal":{"ghostty":true}}"#), none);
        assert_eq!(s.warnings.len(), 1);
    }

    #[test]
    fn load_reads_from_a_temp_home() {
        let home = std::env::temp_dir().join(format!("poimen-settings-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".config/poimen")).unwrap();
        std::fs::write(config_path(&home), r#"{"theme":"light"}"#).unwrap();
        assert_eq!(load(&home).theme, "light");
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(load(&home), Settings::default());
    }

    #[test]
    fn set_theme_replaces_the_value_and_keeps_the_rest() {
        let text = "{\n  // my theme\n  \"theme\": \"system\", /* c */\n  \"terminal\": {\"theme\": \"x\"}\n}";
        let out = set_theme_text(text, "dark").unwrap();
        assert_eq!(
            out,
            "{\n  // my theme\n  \"theme\": \"dark\", /* c */\n  \"terminal\": {\"theme\": \"x\"}\n}"
        );
    }

    #[test]
    fn set_theme_inserts_when_missing_and_ignores_nested_keys() {
        let out = set_theme_text("{\"terminal\":{\"theme\":\"x\"}}", "light").unwrap();
        assert_eq!(
            out,
            "{\n  \"theme\": \"light\",\"terminal\":{\"theme\":\"x\"}}"
        );
        assert_eq!(
            set_theme_text("{ }", "dark").unwrap(),
            "{\"theme\": \"dark\"}"
        );
        assert_eq!(resolve(Some(&out), none).theme, "light");
    }

    #[test]
    fn set_theme_rejects_bad_input() {
        assert!(set_theme_text("{}", "blue").is_err());
        assert!(set_theme_text("[1]", "dark").is_err());
    }

    #[test]
    fn save_theme_writes_and_creates_the_file() {
        let home = std::env::temp_dir().join(format!("poimen-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        save_theme(&home, "dark").unwrap();
        assert_eq!(load(&home).theme, "dark");
        std::fs::write(
            config_path(&home),
            r#"{"terminal":{"fontSize":15},"theme":"dark"}"#,
        )
        .unwrap();
        save_theme(&home, "light").unwrap();
        let s = load(&home);
        assert_eq!((s.theme.as_str(), s.font_size), ("light", 15.0));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(v["fontSize"], 13.0);
        assert!(v["fontFamily"].is_null());
        assert_eq!(v["autoControl"], true);
        assert_eq!(v["shortcuts"]["toggleSidebar"], "CmdOrCtrl+B");
    }

    #[test]
    fn normalizes_accelerators() {
        assert_eq!(
            normalize_accelerator("cmd+shift+w").as_deref(),
            Some("Cmd+Shift+W")
        );
        assert_eq!(
            normalize_accelerator("shift+ctrl+arrowleft").as_deref(),
            Some("Ctrl+Shift+Left")
        );
        assert_eq!(
            normalize_accelerator("CmdOrCtrl+\\").as_deref(),
            Some("CmdOrCtrl+\\")
        );
        assert_eq!(normalize_accelerator("f5").as_deref(), Some("F5"));
    }

    #[test]
    fn rejects_invalid_accelerators() {
        for bad in ["", "W", "Shift+W", "Hyper+W", "Cmd+", "Cmd+Foo", "Cmd+F13"] {
            assert_eq!(normalize_accelerator(bad), None, "{bad}");
        }
    }

    #[test]
    fn shortcuts_default_without_overrides() {
        let s = resolve(Some("{}"), none);
        assert_eq!(s.shortcuts, default_shortcuts());
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
    }

    #[test]
    fn shortcut_overrides_apply_and_can_unbind() {
        let s = resolve(
            Some(r#"{"shortcuts":{"toggleSidebar":"ctrl+alt+b","refresh":""}}"#),
            none,
        );
        assert_eq!(s.shortcuts["toggleSidebar"], "Ctrl+Alt+B");
        assert_eq!(s.shortcuts["refresh"], "");
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn invalid_shortcuts_warn_and_fall_back() {
        let s = resolve(
            Some(
                r#"{"shortcuts":{"closeTab":"nope","bogus":"Cmd+X","zoomIn":3,"refresh":"CmdOrCtrl+B"}}"#,
            ),
            none,
        );
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
        assert_eq!(s.shortcuts["zoomIn"], "CmdOrCtrl+=");
        assert_eq!(s.shortcuts["refresh"], "CmdOrCtrl+R");
        assert!(!s.shortcuts.contains_key("bogus"));
        assert_eq!(s.warnings.len(), 4, "{:?}", s.warnings);
        let bad = resolve(Some(r#"{"shortcuts":[]}"#), none);
        assert_eq!(bad.shortcuts, default_shortcuts());
        assert_eq!(bad.warnings.len(), 1);
    }
}
