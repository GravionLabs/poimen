//! Schedules of command task modes (`schedules[]` and `triggers` of the user config, #1000,
//! ADR 0020): parsing, the due times of a cron expression in a time zone, and the in-place edits
//! of the settings page. The timer itself is `poimen-server`'s. Only the user config is read: a
//! project file cannot define or change a schedule.

use std::path::Path;
use std::str::FromStr;

use croner::Cron;
use jiff::civil::DateTime;
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, ToSpan};
use serde_json::Value;

use crate::settings::{set_key_text, set_path_text};
use crate::tasks::{CommandSpec, TaskModes, placeholders, user_modes};

/// A missed run is caught up when it is at most this old (ADR 0020).
pub const GRACE_MS: u64 = 12 * 60 * 60 * 1000;
/// `triggers.maxRunsPerDay` without a value.
pub const DEFAULT_MAX_RUNS_PER_DAY: u32 = 24;
const MAX_RUNS_PER_DAY: u64 = 96;
/// The placeholders that need an issue; a scheduled command has none.
const ISSUE_PLACEHOLDERS: &[&str] = &["nr", "title", "url", "branch"];

/// A cron expression of five fields in a time zone.
#[derive(Debug, Clone)]
pub struct Timing {
    cron: Cron,
    zone: TimeZone,
}

impl Timing {
    pub fn parse(cron: &str, timezone: &str) -> Result<Self, String> {
        if cron.split_whitespace().count() != 5 {
            return Err("cron must have five fields".into());
        }
        let zone =
            TimeZone::get(timezone).map_err(|_| format!("unknown time zone {timezone:?}"))?;
        let cron = Cron::from_str(cron).map_err(|e| format!("invalid cron expression: {e}"))?;
        Ok(Self { cron, zone })
    }

    /// The instant of a local time: the first of a repeated one, and for one that daylight
    /// saving skips the first minute after the gap.
    fn instant(&self, mut local: DateTime) -> Option<Timestamp> {
        // Gaps are hours, a skipped day (Samoa, 2011) is the largest.
        for _ in 0..=48 * 60 {
            let found = self.zone.to_ambiguous_timestamp(local);
            if !matches!(found.offset(), AmbiguousOffset::Gap { .. }) {
                return found.earlier().ok();
            }
            local = local.checked_add(1.minute()).ok()?;
        }
        None
    }

    /// The due times from a few hours before `from_ms` on, in order (milliseconds).
    fn occurrences(&self, from_ms: u64) -> impl Iterator<Item = u64> + '_ {
        // The expression is matched against local wall-clock times, written as if they were UTC,
        // so that daylight saving is only handled by `instant`.
        let mut at = i64::try_from(from_ms)
            .ok()
            .and_then(|ms| Timestamp::from_millisecond(ms).ok())
            .and_then(|t| {
                let local = self.zone.to_datetime(t).checked_sub(3.hours()).ok()?;
                local.to_zoned_datetime(TimeZone::UTC).ok()
            });
        std::iter::from_fn(move || {
            let next = self.cron.find_next_occurrence(at.as_ref()?, false).ok();
            at = next.clone();
            next
        })
        .filter_map(|local| self.instant(local.datetime()))
        .filter_map(|t| u64::try_from(t.as_millisecond()).ok())
    }

    /// The first due time after `ms`; `None` for an expression that never matches.
    pub fn next_after(&self, ms: u64) -> Option<u64> {
        self.occurrences(ms).find(|t| *t > ms)
    }

    /// The newest due time after `from_ms` and not after `to_ms`.
    pub fn latest_between(&self, from_ms: u64, to_ms: u64) -> Option<u64> {
        self.occurrences(from_ms)
            .take_while(|t| *t <= to_ms)
            .filter(|t| *t > from_ms)
            .last()
    }
}

/// A valid entry of `schedules[]`.
#[derive(Debug, Clone)]
pub struct Schedule {
    /// `<dir>|<mode>|<cron>|<timezone>`.
    pub key: String,
    pub cron: String,
    pub timezone: String,
    pub timing: Timing,
    /// The repository directory, absolute.
    pub dir: String,
    pub mode: String,
    /// Must exit with 0, else the run is skipped.
    pub precheck: Option<Vec<String>>,
    /// `enabled`, default true: the switch of the settings page.
    pub enabled: bool,
    /// The command of the mode.
    pub command: CommandSpec,
}

/// The schedules of a user config with the limits that hold for all of them.
#[derive(Debug, Clone, Default)]
pub struct Schedules {
    pub list: Vec<Schedule>,
    /// `triggers.paused`.
    pub paused: bool,
    /// `triggers.maxRunsPerDay`.
    pub max_runs_per_day: u32,
}

fn expand_home(dir: &str, home: Option<&Path>) -> String {
    let rest = dir.strip_prefix("~/").or_else(|| dir.strip_prefix("~\\"));
    match (home, rest) {
        (Some(home), Some(rest)) => home.join(rest).to_string_lossy().into_owned(),
        (Some(home), None) if dir == "~" => home.to_string_lossy().into_owned(),
        _ => dir.to_owned(),
    }
}

fn entry(value: &Value, modes: &TaskModes, home: Option<&Path>) -> Result<Schedule, String> {
    let object = value.as_object().ok_or("must be an object")?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{key} must be a non-empty string"))
    };
    let cron = text("cron")?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let timezone = text("timezone")?.to_owned();
    let timing = Timing::parse(&cron, &timezone)?;
    let dir = expand_home(text("dir")?, home);
    if !Path::new(&dir).is_absolute() {
        return Err("dir must be an absolute directory (or start with ~/)".into());
    }
    let name = text("mode")?.to_owned();
    let precheck = match object.get("precheck") {
        None => None,
        Some(v) => Some(
            v.as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|i| i.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()
                })
                .filter(|argv| argv.first().is_some_and(|p| !p.is_empty()))
                .ok_or("precheck must be an array of strings starting with a program")?,
        ),
    };
    let enabled = match object.get("enabled") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("enabled must be true or false".into()),
    };
    let mode = modes
        .modes
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("your task modes have no mode {name:?}"))?;
    let Some(command) = mode.command.clone() else {
        return Err(format!("the task mode {name:?} has no command"));
    };
    let parts = match &command {
        CommandSpec::Argv(argv) => argv.clone(),
        CommandSpec::Shell(line) => vec![line.clone()],
    };
    if let Some(p) = parts
        .iter()
        .flat_map(|part| placeholders(part))
        .find(|p| ISSUE_PLACEHOLDERS.contains(&p.as_str()))
    {
        return Err(format!(
            "the command of the task mode {name:?} needs an issue ({{{p}}})"
        ));
    }
    if mode.worktree {
        return Err(format!(
            "the task mode {name:?} makes a worktree; set \"worktree\": false"
        ));
    }
    Ok(Schedule {
        key: format!("{dir}|{name}|{cron}|{timezone}"),
        cron,
        timezone,
        timing,
        dir,
        mode: name,
        precheck,
        enabled,
        command,
    })
}

/// How a warning names the entry `i`.
fn label(i: usize, value: &Value) -> String {
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    match (text("mode"), text("dir")) {
        (Some(mode), Some(dir)) => format!("schedules[{i}] (mode \"{mode}\" in {dir})"),
        _ => format!("schedules[{i}]"),
    }
}

/// The schedules of the user config `root`. Task modes come from the same config only. An entry
/// that is invalid, or whose mode cannot be scheduled, warns and is left out.
pub fn parse(root: &Value, home: Option<&Path>, warnings: &mut Vec<String>) -> Schedules {
    let mut out = Schedules {
        max_runs_per_day: DEFAULT_MAX_RUNS_PER_DAY,
        ..Schedules::default()
    };
    match root.get("triggers") {
        None => {}
        Some(Value::Object(triggers)) => {
            match triggers.get("paused") {
                None => {}
                Some(Value::Bool(b)) => out.paused = *b,
                Some(_) => {
                    // Someone who meant to pause must not get runs.
                    out.paused = true;
                    warnings.push("triggers.paused must be true or false; paused".into());
                }
            }
            if let Some(v) = triggers.get("maxRunsPerDay") {
                match v.as_u64().filter(|n| (1..=MAX_RUNS_PER_DAY).contains(n)) {
                    Some(n) => out.max_runs_per_day = n as u32,
                    None => warnings.push(format!(
                        "triggers.maxRunsPerDay must be 1 to {MAX_RUNS_PER_DAY}; using \
                         {DEFAULT_MAX_RUNS_PER_DAY}"
                    )),
                }
            }
        }
        Some(_) => {
            out.paused = true;
            warnings.push("triggers must be an object; schedules are paused".into());
        }
    }
    let Some(value) = root.get("schedules") else {
        return out;
    };
    let Some(items) = value.as_array() else {
        warnings.push("schedules must be an array".into());
        return out;
    };
    let modes = user_modes(root, &mut Vec::new());
    for (i, item) in items.iter().enumerate() {
        match entry(item, &modes, home) {
            Ok(s) if out.list.iter().any(|o| o.key == s.key) => warnings.push(format!(
                "{}: the same schedule twice; the first one wins",
                label(i, item)
            )),
            Ok(s) => out.list.push(s),
            Err(e) => warnings.push(format!("{}: {e}; ignored", label(i, item))),
        }
    }
    out
}

/// The config `text` with `enabled` set on the schedule `key`; the others stay as they are.
pub fn set_enabled_text(
    text: &str,
    home: Option<&Path>,
    key: &str,
    enabled: bool,
) -> Result<String, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| format!("config.json is not valid JSON: {e}"))?;
    let modes = user_modes(&root, &mut Vec::new());
    let mut items = root
        .get("schedules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let item = items
        .iter_mut()
        .find(|item| entry(item, &modes, home).is_ok_and(|s| s.key == key))
        .ok_or("the schedule is no longer in config.json")?;
    item["enabled"] = Value::Bool(enabled);
    let value = serde_json::to_string_pretty(&Value::Array(items)).map_err(|e| e.to_string())?;
    set_key_text(text, "schedules", &value)
}

/// The config `text` with the global pause `triggers.paused` set.
pub fn set_paused_text(text: &str, paused: bool) -> Result<String, String> {
    set_path_text(text, &["triggers", "paused"], &paused.to_string())
}

#[cfg(test)]
#[path = "schedules_tests.rs"]
mod tests;
