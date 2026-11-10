//! What the TUI reads from the user config (`~/.config/poimen/config.json`, ADR 0006): the prefix
//! key (`console.prefix`), the keys after it (`console.keys`), the sidebar rows (`sidebar.rows`,
//! parsed by `poimen_core::settings`) and the color scheme (`colorSchema`, a user scheme of
//! `~/.config/poimen/themes`). Invalid entries warn and keep the default.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use poimen_core::schemes::Scheme;
use poimen_core::settings::{SidebarRows, resolve_sidebar_rows};
use serde_json::Value;

/// A key with modifiers, written like `C-b`, `ctrl+b`, `M-x`, `d` or `up`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeySpec {
    pub const fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            mods: KeyModifiers::CONTROL,
        }
    }

    pub const fn plain(code: KeyCode) -> Self {
        Self {
            code,
            mods: KeyModifiers::NONE,
        }
    }

    /// Parses `C-b`, `Ctrl+B`, `alt-x`, `S-tab`, `d`, `up`, `pagedown`, `space`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty key".into());
        }
        // Everything before the last character (on a char boundary, the key may be non-ASCII).
        let mut head = text.chars();
        head.next_back();
        let sep = if head.as_str().contains('+') {
            '+'
        } else {
            '-'
        };
        let mut parts: Vec<&str> = text.split(sep).collect();
        // `C--` and `C-+`: the key is the separator itself.
        if parts.len() > 1 && parts.last() == Some(&"") {
            parts.pop();
            if parts.last() == Some(&"") {
                parts.pop();
            }
            parts.push(if sep == '+' { "+" } else { "-" });
        }
        let (key, modifiers) = parts.split_last().ok_or("empty key")?;
        let mut mods = KeyModifiers::NONE;
        for m in modifiers {
            mods |= match m.to_ascii_lowercase().as_str() {
                "c" | "ctrl" | "control" => KeyModifiers::CONTROL,
                "m" | "alt" | "meta" | "a" => KeyModifiers::ALT,
                "s" | "shift" => KeyModifiers::SHIFT,
                other => return Err(format!("unknown modifier \"{other}\" in \"{text}\"")),
            };
        }
        let code = match key.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if mods.contains(KeyModifiers::CONTROL) => {
                        KeyCode::Char(c.to_ascii_lowercase())
                    }
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(format!("unknown key \"{key}\" in \"{text}\"")),
                }
            }
        };
        if matches!(code, KeyCode::Char(_)) {
            mods.remove(KeyModifiers::SHIFT);
        }
        Ok(Self { code, mods })
    }

    /// Whether a pressed key is this one. Shift is ignored for characters (it is in the
    /// character already), and a control character is compared in lowercase.
    pub fn matches(&self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) => {
                let mut mods = key.modifiers;
                mods.remove(KeyModifiers::SHIFT);
                let c = if mods.contains(KeyModifiers::CONTROL) {
                    c.to_ascii_lowercase()
                } else {
                    c
                };
                self.code == KeyCode::Char(c) && self.mods == mods
            }
            code => self.code == code && self.mods == key.modifiers,
        }
    }

    /// The byte `poimen attach` filters for: the control character of a `Ctrl+<letter>` key.
    pub fn ctrl_byte(&self) -> Option<u8> {
        match self.code {
            KeyCode::Char(c) if self.mods == KeyModifiers::CONTROL && c.is_ascii_lowercase() => {
                Some(c as u8 & 0x1f)
            }
            _ => None,
        }
    }

    /// `C-b`, `M-x`, `d`, `up`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push_str("C-");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push_str("M-");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            out.push_str("S-");
        }
        match self.code {
            KeyCode::Char(' ') => out.push_str("space"),
            KeyCode::Char(c) => out.push(c),
            KeyCode::Up => out.push('↑'),
            KeyCode::Down => out.push('↓'),
            KeyCode::Left => out.push('←'),
            KeyCode::Right => out.push('→'),
            other => out.push_str(&format!("{other}").to_lowercase()),
        }
        out
    }
}

/// The default prefix, `Ctrl+B` (ADR 0006).
pub const DEFAULT_PREFIX: KeySpec = KeySpec::ctrl('b');

/// The keys after the prefix (`console.keys`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// Leaves the TUI; the terminals keep running.
    pub detach: KeySpec,
    /// Shows or hides the sidebar.
    pub sidebar: KeySpec,
    /// Selects (and attaches to) the next terminal of the sidebar.
    pub next: KeySpec,
    /// Selects the previous terminal.
    pub previous: KeySpec,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            detach: KeySpec::plain(KeyCode::Char('d')),
            sidebar: KeySpec::plain(KeyCode::Char('b')),
            next: KeySpec::plain(KeyCode::Down),
            previous: KeySpec::plain(KeyCode::Up),
        }
    }
}

/// The action names of `console.keys`.
pub const KEY_ACTIONS: [&str; 4] = ["detach", "sidebar", "next", "previous"];

impl Keys {
    fn slot(&mut self, action: &str) -> Option<&mut KeySpec> {
        match action {
            "detach" => Some(&mut self.detach),
            "sidebar" => Some(&mut self.sidebar),
            "next" => Some(&mut self.next),
            "previous" => Some(&mut self.previous),
            _ => None,
        }
    }

    fn all(&self) -> [KeySpec; 4] {
        [self.detach, self.sidebar, self.next, self.previous]
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub prefix: KeySpec,
    pub keys: Keys,
    /// `None` per row type keeps the default lines of that type.
    pub rows: SidebarRows,
    /// The user scheme `colorSchema` names; `None` uses the colors of the local terminal.
    pub scheme: Option<Scheme>,
    pub warnings: Vec<String>,
}

impl Default for KeySpec {
    fn default() -> Self {
        DEFAULT_PREFIX
    }
}

/// The config of the TUI from the parsed `config.json` and the user schemes.
pub fn parse(root: &Value, schemes: &[Scheme]) -> Config {
    let mut c = Config::default();
    if let Some(console) = root.get("console") {
        parse_console(console, &mut c);
    }
    if let Some(sidebar) = root.get("sidebar") {
        c.rows = resolve_sidebar_rows(sidebar, &mut c.warnings);
    }
    if let Some(name) = scheme_name(root) {
        c.scheme = schemes.iter().find(|s| s.name == name).cloned();
    }
    c
}

fn parse_console(console: &Value, c: &mut Config) {
    let Some(console) = console.as_object() else {
        c.warnings.push("console must be an object".into());
        return;
    };
    match console.get("prefix") {
        None => {}
        Some(Value::String(s)) => match KeySpec::parse(s) {
            Ok(k) => c.prefix = k,
            Err(e) => c.warnings.push(format!("console.prefix: {e}; using C-b")),
        },
        Some(other) => c.warnings.push(format!(
            "console.prefix {other} must be a string; using C-b"
        )),
    }
    let Some(keys) = console.get("keys") else {
        return;
    };
    let Some(keys) = keys.as_object() else {
        c.warnings.push("console.keys must be an object".into());
        return;
    };
    let mut parsed = Keys::default();
    for (action, value) in keys {
        let Some(slot) = parsed.slot(action) else {
            c.warnings.push(format!(
                "console.keys.{action} is not a known action ({}); ignored",
                KEY_ACTIONS.join(", ")
            ));
            continue;
        };
        match value.as_str().map(KeySpec::parse) {
            Some(Ok(k)) => *slot = k,
            Some(Err(e)) => c
                .warnings
                .push(format!("console.keys.{action}: {e}; using the default")),
            None => c.warnings.push(format!(
                "console.keys.{action} must be a key string; using the default"
            )),
        }
    }
    let all = parsed.all();
    let clash = (0..all.len()).any(|i| all[i] == c.prefix || all[i + 1..].contains(&all[i]));
    if clash {
        c.warnings.push(
            "console.keys: two actions share a key, or one uses the prefix; using the default keys"
                .into(),
        );
    } else {
        c.keys = parsed;
    }
}

/// The scheme `colorSchema` asks for: the name, or of a light/dark pair the side `theme` picks
/// (`light`, else dark).
fn scheme_name(root: &Value) -> Option<String> {
    match root.get("colorSchema")? {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Object(pair) => {
            let side = if root.get("theme").and_then(Value::as_str) == Some("light") {
                "light"
            } else {
                "dark"
            };
            pair.get(side)
                .and_then(Value::as_str)
                .map(|s| s.trim().to_owned())
        }
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

/// The config under `home`; a missing or unreadable file is the default.
pub fn load(home: Option<&Path>) -> Config {
    let Some(home) = home else {
        return Config::default();
    };
    let root = std::fs::read_to_string(poimen_core::settings::config_path(home))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let schemes = poimen_core::schemes::load(home);
    parse(&root, &schemes.schemes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;
    use serde_json::json;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn key_specs_parse_in_every_spelling() {
        assert_eq!(KeySpec::parse("C-b"), Ok(KeySpec::ctrl('b')));
        assert_eq!(KeySpec::parse("Ctrl+B"), Ok(KeySpec::ctrl('b')));
        assert_eq!(KeySpec::parse("control-a"), Ok(KeySpec::ctrl('a')));
        assert_eq!(
            KeySpec::parse("M-x"),
            Ok(KeySpec {
                code: KeyCode::Char('x'),
                mods: KeyModifiers::ALT
            })
        );
        assert_eq!(KeySpec::parse("d"), Ok(KeySpec::plain(KeyCode::Char('d'))));
        assert_eq!(KeySpec::parse("up"), Ok(KeySpec::plain(KeyCode::Up)));
        assert_eq!(KeySpec::parse("C--"), Ok(KeySpec::ctrl('-')));
        assert_eq!(KeySpec::parse("-"), Ok(KeySpec::plain(KeyCode::Char('-'))));
        assert_eq!(
            KeySpec::parse("S-tab"),
            Ok(KeySpec {
                code: KeyCode::Tab,
                mods: KeyModifiers::SHIFT
            })
        );
    }

    #[test]
    fn non_ascii_keys_parse_without_panicking() {
        assert_eq!(KeySpec::parse("é"), Ok(KeySpec::plain(KeyCode::Char('é'))));
        assert_eq!(KeySpec::parse("C-é"), Ok(KeySpec::ctrl('é')));
        assert_eq!(KeySpec::parse("Ctrl+é"), Ok(KeySpec::ctrl('é')));
        assert_eq!(
            KeySpec::parse("M-ü"),
            Ok(KeySpec {
                code: KeyCode::Char('ü'),
                mods: KeyModifiers::ALT
            })
        );
        assert_eq!(KeySpec::parse("C-+"), Ok(KeySpec::ctrl('+')));
        assert_eq!(KeySpec::parse("Ctrl++"), Ok(KeySpec::ctrl('+')));
        assert_eq!(KeySpec::parse("+"), Ok(KeySpec::plain(KeyCode::Char('+'))));
        assert!(KeySpec::parse("C-éé").unwrap_err().contains("unknown key"));
        assert!(KeySpec::parse("é+").unwrap_err().contains("unknown key"));
        assert!(KeySpec::parse("Hyper-é").unwrap_err().contains("modifier"));
    }

    #[test]
    fn non_ascii_console_entries_parse_or_warn() {
        let c = parse(
            &json!({"console": {"prefix": "C-é", "keys": {"detach": "ü"}}}),
            &[],
        );
        assert_eq!(c.prefix, KeySpec::ctrl('é'));
        assert_eq!(c.keys.detach, KeySpec::plain(KeyCode::Char('ü')));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);

        let bad = parse(
            &json!({"console": {"prefix": "é-é", "keys": {"detach": "C-éé"}}}),
            &[],
        );
        assert_eq!(bad.prefix, DEFAULT_PREFIX);
        assert_eq!(bad.keys, Keys::default());
        assert_eq!(bad.warnings.len(), 2, "{:?}", bad.warnings);
    }

    #[test]
    fn bad_key_specs_are_errors() {
        assert!(KeySpec::parse("").is_err());
        assert!(KeySpec::parse("Hyper-b").unwrap_err().contains("modifier"));
        assert!(KeySpec::parse("C-bb").unwrap_err().contains("unknown key"));
    }

    #[test]
    fn matching_ignores_shift_on_characters_and_case_with_ctrl() {
        let k = KeySpec::ctrl('b');
        assert!(k.matches(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        assert!(k.matches(&key(
            KeyCode::Char('B'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
        assert!(!k.matches(&key(KeyCode::Char('b'), KeyModifiers::NONE)));
        let d = KeySpec::plain(KeyCode::Char('D'));
        assert!(d.matches(&key(KeyCode::Char('D'), KeyModifiers::SHIFT)));
        assert!(!d.matches(&key(KeyCode::Char('d'), KeyModifiers::NONE)));
        let up = KeySpec::plain(KeyCode::Up);
        assert!(!up.matches(&key(KeyCode::Up, KeyModifiers::SHIFT)));
    }

    #[test]
    fn ctrl_byte_and_label() {
        assert_eq!(KeySpec::ctrl('b').ctrl_byte(), Some(0x02));
        assert_eq!(KeySpec::ctrl('a').ctrl_byte(), Some(0x01));
        assert_eq!(KeySpec::plain(KeyCode::Char('b')).ctrl_byte(), None);
        assert_eq!(KeySpec::ctrl('b').label(), "C-b");
        assert_eq!(KeySpec::plain(KeyCode::Down).label(), "↓");
    }

    #[test]
    fn defaults_without_a_config() {
        let c = parse(&json!({}), &[]);
        assert_eq!(c.prefix, DEFAULT_PREFIX);
        assert_eq!(c.keys, Keys::default());
        assert_eq!(c.scheme, None);
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn console_prefix_and_keys_are_read() {
        let c = parse(
            &json!({"console": {"prefix": "C-a", "keys": {"detach": "q", "next": "j"}}}),
            &[],
        );
        assert_eq!(c.prefix, KeySpec::ctrl('a'));
        assert_eq!(c.keys.detach, KeySpec::plain(KeyCode::Char('q')));
        assert_eq!(c.keys.next, KeySpec::plain(KeyCode::Char('j')));
        assert_eq!(c.keys.previous, KeySpec::plain(KeyCode::Up));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn invalid_console_entries_warn_and_keep_defaults() {
        let c = parse(
            &json!({"console": {"prefix": "Hyper-x", "keys": {"fly": "f", "detach": 3}}}),
            &[],
        );
        assert_eq!(c.prefix, DEFAULT_PREFIX);
        assert_eq!(c.keys, Keys::default());
        assert_eq!(c.warnings.len(), 3, "{:?}", c.warnings);

        let clash = parse(&json!({"console": {"keys": {"detach": "b"}}}), &[]);
        assert_eq!(clash.keys, Keys::default());
        assert!(clash.warnings[0].contains("share a key"));

        let on_prefix = parse(&json!({"console": {"keys": {"detach": "C-b"}}}), &[]);
        assert_eq!(on_prefix.keys, Keys::default());

        let not_object = parse(&json!({"console": 1}), &[]);
        assert_eq!(not_object.warnings, ["console must be an object"]);
    }

    fn scheme(name: &str) -> Scheme {
        let text = format!(
            "[colors]\nforeground = \"#ffffff\"\nbackground = \"#000000\"\n\
             ansi = [\"#000000\", \"#111111\", \"#222222\", \"#333333\", \"#444444\", \"#555555\", \"#666666\", \"#777777\"]\n\
             brights = [\"#888888\", \"#999999\", \"#aaaaaa\", \"#bbbbbb\", \"#cccccc\", \"#dddddd\", \"#eeeeee\", \"#ffffff\"]\n\
             [metadata]\nname = \"{name}\"\n"
        );
        poimen_core::schemes::parse_scheme(&text, "x").unwrap()
    }

    #[test]
    fn color_schema_picks_a_user_scheme_or_none() {
        let schemes = [scheme("mine"), scheme("day")];
        let named = parse(&json!({"colorSchema": "mine"}), &schemes);
        assert_eq!(named.scheme.map(|s| s.name).as_deref(), Some("mine"));
        let pair = json!({"colorSchema": {"light": "day", "dark": "mine"}, "theme": "light"});
        assert_eq!(
            parse(&pair, &schemes).scheme.map(|s| s.name).as_deref(),
            Some("day")
        );
        let dark = json!({"colorSchema": {"light": "day", "dark": "mine"}});
        assert_eq!(
            parse(&dark, &schemes).scheme.map(|s| s.name).as_deref(),
            Some("mine")
        );
        // A bundled scheme of the app (not a user file) leaves the terminal's own colors.
        assert_eq!(
            parse(&json!({"colorSchema": "poimen"}), &schemes).scheme,
            None
        );
    }

    #[test]
    fn sidebar_rows_come_from_the_shared_parser() {
        let c = parse(
            &json!({"sidebar": {"rows": {"terminal": [["name", "$cost"]]}}}),
            &[],
        );
        let lines = c.rows.terminal.unwrap();
        assert_eq!(lines[0][1].token, "$cost");
        assert!(c.rows.workspace.is_none());
    }

    #[test]
    fn load_without_home_or_file_is_the_default() {
        assert_eq!(load(None), Config::default());
        let c = load(Some(Path::new("/nonexistent/poimen-home")));
        assert_eq!(c, Config::default());
    }
}
