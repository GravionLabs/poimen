//! `terminalHistory` (#1196, ADR 0015): whether the server logs terminal output to disk and how
//! much of it it keeps. The server reads it from the user config while it runs; the app reads it
//! for the settings dialog.

use serde_json::Value;

/// Default of `terminalHistory.perTerminalMiB`.
pub const DEFAULT_PER_TERMINAL_MIB: u64 = 8;
/// Default of `terminalHistory.totalMiB`.
pub const DEFAULT_TOTAL_MIB: u64 = 256;
/// Upper bound of both limits (1 TiB), so a size in bytes never overflows.
pub const MAX_MIB: u64 = 1024 * 1024;

/// The resolved setting; the app sends it to the frontend as `enabled`, `perTerminalMib`,
/// `totalMib`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalHistory {
    pub enabled: bool,
    pub per_terminal_mib: u64,
    pub total_mib: u64,
}

impl Default for TerminalHistory {
    fn default() -> Self {
        Self {
            enabled: true,
            per_terminal_mib: DEFAULT_PER_TERMINAL_MIB,
            total_mib: DEFAULT_TOTAL_MIB,
        }
    }
}

fn limit(object: &Value, key: &str, default: u64, warnings: &mut Vec<String>) -> u64 {
    match object.get(key) {
        None | Some(Value::Null) => default,
        Some(v) => match v.as_u64().filter(|n| (1..=MAX_MIB).contains(n)) {
            Some(n) => n,
            None => {
                warnings.push(format!(
                    "terminalHistory.{key} {v} must be a whole number of MiB from 1 to {MAX_MIB}; using {default}"
                ));
                default
            }
        },
    }
}

/// Resolves the value of the `terminalHistory` key; an invalid field warns and keeps its default.
pub fn resolve(value: &Value, warnings: &mut Vec<String>) -> TerminalHistory {
    let mut out = TerminalHistory::default();
    if !value.is_object() {
        warnings.push("terminalHistory must be an object".into());
        return out;
    }
    match value.get("enabled") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(b)) => out.enabled = *b,
        Some(_) => warnings.push("terminalHistory.enabled must be true or false".into()),
    }
    out.per_terminal_mib = limit(value, "perTerminalMiB", DEFAULT_PER_TERMINAL_MIB, warnings);
    out.total_mib = limit(value, "totalMiB", DEFAULT_TOTAL_MIB, warnings);
    out
}

/// The setting of a whole config `root`; absent is the default.
pub fn from_root(root: &Value, warnings: &mut Vec<String>) -> TerminalHistory {
    root.get("terminalHistory")
        .map_or_else(TerminalHistory::default, |v| resolve(v, warnings))
}

/// The setting from the config `text`; unreadable text is the default (history on).
pub fn from_text(text: &str) -> TerminalHistory {
    serde_json::from_str::<Value>(text)
        .map(|root| from_root(&root, &mut Vec::new()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> (TerminalHistory, Vec<String>) {
        let mut w = Vec::new();
        let v: Value = serde_json::from_str(json).unwrap();
        (from_root(&v, &mut w), w)
    }

    #[test]
    fn defaults_are_on_8_and_256() {
        let (s, w) = read("{}");
        assert_eq!(s, TerminalHistory::default());
        assert_eq!((s.enabled, s.per_terminal_mib, s.total_mib), (true, 8, 256));
        assert!(w.is_empty());
    }

    #[test]
    fn reads_every_field() {
        let (s, w) =
            read(r#"{"terminalHistory":{"enabled":false,"perTerminalMiB":2,"totalMiB":10}}"#);
        assert_eq!((s.enabled, s.per_terminal_mib, s.total_mib), (false, 2, 10));
        assert!(w.is_empty());
    }

    #[test]
    fn an_invalid_field_warns_and_keeps_its_default() {
        let (s, w) =
            read(r#"{"terminalHistory":{"enabled":"no","perTerminalMiB":0,"totalMiB":1.5}}"#);
        assert_eq!(s, TerminalHistory::default());
        assert_eq!(w.len(), 3, "{w:?}");
        let (s, w) = read(r#"{"terminalHistory":3}"#);
        assert_eq!((s, w.len()), (TerminalHistory::default(), 1));
    }

    #[test]
    fn unreadable_text_is_the_default() {
        assert_eq!(from_text("not json"), TerminalHistory::default());
        assert!(!from_text(r#"{"terminalHistory":{"enabled":false}}"#).enabled);
    }
}
