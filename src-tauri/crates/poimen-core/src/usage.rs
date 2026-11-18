//! Provider usage limits for the status bar (#784): provider-neutral [`Window`]s and the Claude
//! source, which asks the usage endpoint the Claude CLI itself uses with the user's own Claude
//! Code login. The endpoint is not a documented public API, so every failure is an error text the
//! caller shows once and then hides the item; nothing else depends on it.
//!
//! The token is read from the credentials file (or, on macOS, the Keychain), handed to `curl`
//! through its standard input (never argv) and goes nowhere but the endpoint: it is not written
//! to disk, not logged and not part of any error text.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;
use serde_json::Value;

use crate::gui_path;

/// The endpoint of the Claude usage numbers.
pub const CLAUDE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Value of the `anthropic-beta` header the endpoint needs.
pub const CLAUDE_BETA: &str = "oauth-2025-04-20";
/// Timeout of the request in seconds.
pub const TIMEOUT_SECS: u32 = 8;
/// The Keychain item of Claude Code's login (macOS).
pub const KEYCHAIN_ITEM: &str = "Claude Code-credentials";

/// One limit window of a provider, such as the 5 hours of Claude.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// Short name: `5h`, `7d`, `7d Opus`.
    pub label: String,
    /// Used share 0 to 100; `None` when the provider did not say.
    pub used_percent: Option<f64>,
    /// When the window resets, as the provider wrote it (RFC 3339); `None` when unknown.
    pub resets_at: Option<String>,
}

/// The windows of the Claude endpoint in the order they are shown, with their JSON key.
const CLAUDE_WINDOWS: &[(&str, &str)] = &[
    ("five_hour", "5h"),
    ("seven_day", "7d"),
    ("seven_day_opus", "7d Opus"),
    ("seven_day_sonnet", "7d Sonnet"),
];

/// `utilization` as 0 to 100: below 1 it is a fraction, otherwise already a percentage.
pub fn to_percent(utilization: f64) -> f64 {
    let percent = if utilization < 1.0 {
        utilization * 100.0
    } else {
        utilization
    };
    percent.clamp(0.0, 100.0)
}

/// Parses the endpoint's answer. A window that is absent or `null` is left out, unknown fields are
/// ignored; an answer that is not a JSON object or has none of the known windows is an error (the
/// shape changed).
pub fn parse_claude(body: &str) -> Result<Vec<Window>, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| "the usage answer is not JSON".to_owned())?;
    let Some(root) = value.as_object() else {
        return Err("the usage answer is not an object".into());
    };
    let windows: Vec<Window> = CLAUDE_WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let w = root.get(*key)?.as_object()?;
            Some(Window {
                label: (*label).to_owned(),
                used_percent: w
                    .get("utilization")
                    .and_then(Value::as_f64)
                    .filter(|u| u.is_finite())
                    .map(to_percent),
                resets_at: w
                    .get("resets_at")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect();
    if windows.is_empty() {
        return Err("the usage answer has none of the known windows".into());
    }
    Ok(windows)
}

/// The window with the highest known use; windows of unknown use only when there is no other.
pub fn tightest(windows: &[Window]) -> Option<&Window> {
    windows
        .iter()
        .filter(|w| w.used_percent.is_some())
        .max_by(|a, b| a.used_percent.partial_cmp(&b.used_percent).unwrap())
}

/// Reads the OAuth access token out of the text of Claude Code's credentials (file or Keychain).
pub fn token_of(credentials: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(credentials)
        .map_err(|_| "the Claude Code credentials are not JSON".to_owned())?;
    value
        .pointer("/claudeAiOauth/accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "no Claude Code login found (no access token in the credentials)".into())
}

/// The directory of Claude Code's files: `CLAUDE_CONFIG_DIR` when set, else `~/.claude`.
pub fn claude_dir(config_dir: Option<&str>, home: &Path) -> PathBuf {
    match config_dir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join(".claude"),
    }
}

/// Where the Claude credentials and the usage endpoint come from; faked in tests.
pub trait Source {
    /// Text of `<claude dir>/.credentials.json`; `None` when there is no such file.
    fn credentials_file(&self) -> Option<String>;
    /// Text of the Keychain item `Claude Code-credentials`; `None` off macOS or without the item.
    fn keychain(&self) -> Option<String>;
    /// `GET` of the usage endpoint with `token`; the body of a success, else a text without the token.
    fn fetch(&self, token: &str) -> Result<String, String>;
}

/// Reads the token (file first, then the Keychain on macOS) and asks the endpoint.
pub fn claude_windows(source: &dyn Source) -> Result<Vec<Window>, String> {
    let credentials = source
        .credentials_file()
        .or_else(|| source.keychain())
        .ok_or("no Claude Code login found (no credentials file and no Keychain item)")?;
    let token = token_of(&credentials)?;
    parse_claude(&source.fetch(&token)?)
}

/// The real source: files, `security` and `curl`.
pub struct System {
    pub home: PathBuf,
}

impl Source for System {
    fn credentials_file(&self) -> Option<String> {
        let dir = claude_dir(
            std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(),
            &self.home,
        );
        std::fs::read_to_string(dir.join(".credentials.json")).ok()
    }

    fn keychain(&self) -> Option<String> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = gui_path::command("/usr/bin/security")
            .args(["find-generic-password", "-s", KEYCHAIN_ITEM, "-w"])
            .stdin(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn fetch(&self, token: &str) -> Result<String, String> {
        let program = gui_path::resolve_program("curl", &gui_path::child_path())
            .ok_or("curl is not on the PATH")?;
        let mut child = gui_path::command(&program)
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--max-time",
                &TIMEOUT_SECS.to_string(),
                "--max-filesize",
                "1048576",
                "--config",
                "-",
                "--",
                CLAUDE_URL,
            ])
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        // The headers go through the config on stdin so the token never shows in a process list.
        let config = format!(
            "header = \"Authorization: Bearer {}\"\nheader = \"anthropic-beta: {CLAUDE_BETA}\"\n",
            token.replace(['\\', '"', '\n', '\r'], "")
        );
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(config.as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.trim();
            // curl names the status ("returned error: 401") and never echoes the config.
            return Err(format!("the usage request failed: {why}"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERCENT: &str = r#"{
        "five_hour": {"utilization": 62.5, "resets_at": "2026-11-18T20:00:00Z"},
        "seven_day": {"utilization": 30, "resets_at": "2026-11-23T10:00:00Z"},
        "seven_day_opus": null,
        "extra_usage": {"is_enabled": true}
    }"#;

    #[test]
    fn parses_percentages_missing_windows_and_unknown_fields() {
        let w = parse_claude(PERCENT).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].label, "5h");
        assert_eq!(w[0].used_percent, Some(62.5));
        assert_eq!(w[0].resets_at.as_deref(), Some("2026-11-18T20:00:00Z"));
        assert_eq!(w[1].label, "7d");
    }

    #[test]
    fn a_utilization_below_one_is_a_fraction() {
        let w = parse_claude(r#"{"five_hour":{"utilization":0.62},"seven_day":{"utilization":1}}"#)
            .unwrap();
        assert!((w[0].used_percent.unwrap() - 62.0).abs() < 1e-9);
        assert_eq!(w[1].used_percent, Some(1.0));
        assert_eq!(to_percent(0.0), 0.0);
        assert_eq!(to_percent(250.0), 100.0);
    }

    #[test]
    fn a_window_without_numbers_keeps_unknown_use() {
        let w = parse_claude(r#"{"seven_day_sonnet":{"utilization":"x"}}"#).unwrap();
        assert_eq!(w[0].label, "7d Sonnet");
        assert_eq!(w[0].used_percent, None);
        assert_eq!(w[0].resets_at, None);
    }

    #[test]
    fn an_unknown_shape_is_an_error() {
        assert!(parse_claude("nope").is_err());
        assert!(parse_claude("[1]").is_err());
        assert!(parse_claude(r#"{"error":"x"}"#).is_err());
        assert!(parse_claude("{}").is_err());
    }

    #[test]
    fn the_tightest_window_has_the_highest_known_use() {
        let w = parse_claude(
            r#"{"five_hour":{"utilization":20},"seven_day":{"utilization":81},"seven_day_opus":{}}"#,
        )
        .unwrap();
        assert_eq!(tightest(&w).unwrap().label, "7d");
        assert!(tightest(&[]).is_none());
    }

    #[test]
    fn the_token_comes_from_the_credentials() {
        assert_eq!(
            token_of(r#"{"claudeAiOauth":{"accessToken":" abc "}}"#).unwrap(),
            "abc"
        );
        assert!(token_of(r#"{"claudeAiOauth":{}}"#).is_err());
        assert!(token_of(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_err());
        assert!(token_of("x").is_err());
    }

    #[test]
    fn config_dir_wins_over_home() {
        let home = Path::new("/h");
        assert_eq!(claude_dir(Some("/c"), home), PathBuf::from("/c"));
        assert_eq!(claude_dir(Some(" "), home), PathBuf::from("/h/.claude"));
        assert_eq!(claude_dir(None, home), PathBuf::from("/h/.claude"));
    }

    struct Fake {
        file: Option<&'static str>,
        keychain: Option<&'static str>,
        answer: Result<&'static str, &'static str>,
    }

    impl Source for Fake {
        fn credentials_file(&self) -> Option<String> {
            self.file.map(str::to_owned)
        }
        fn keychain(&self) -> Option<String> {
            self.keychain.map(str::to_owned)
        }
        fn fetch(&self, token: &str) -> Result<String, String> {
            assert_eq!(token, "tok");
            self.answer.map(str::to_owned).map_err(str::to_owned)
        }
    }

    const LOGIN: &str = r#"{"claudeAiOauth":{"accessToken":"tok"}}"#;

    #[test]
    fn windows_use_the_file_then_the_keychain() {
        let from_file = Fake {
            file: Some(LOGIN),
            keychain: None,
            answer: Ok(PERCENT),
        };
        assert_eq!(claude_windows(&from_file).unwrap().len(), 2);
        let from_keychain = Fake {
            file: None,
            keychain: Some(LOGIN),
            answer: Ok(PERCENT),
        };
        assert_eq!(claude_windows(&from_keychain).unwrap().len(), 2);
    }

    #[test]
    fn every_failure_is_an_error() {
        let none = Fake {
            file: None,
            keychain: None,
            answer: Ok(PERCENT),
        };
        assert!(
            claude_windows(&none)
                .unwrap_err()
                .contains("no Claude Code login")
        );
        let no_token = Fake {
            file: Some("{}"),
            keychain: None,
            answer: Ok(PERCENT),
        };
        assert!(claude_windows(&no_token).is_err());
        let http = Fake {
            file: Some(LOGIN),
            keychain: None,
            answer: Err("the usage request failed: 401"),
        };
        assert!(claude_windows(&http).unwrap_err().contains("401"));
        let shape = Fake {
            file: Some(LOGIN),
            keychain: None,
            answer: Ok("{}"),
        };
        assert!(claude_windows(&shape).is_err());
    }
}
