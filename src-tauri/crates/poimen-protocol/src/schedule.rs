//! `schedule.list` (#1000, ADR 0020): the schedules of the user config with their next run, last
//! result and the runs the server started for them. `schedule.alerts` and `schedule.ack_alerts`
//! (#1308): the failed runs and skips no app has acknowledged.

use serde::{Deserialize, Serialize};

/// What became of a due time or of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleOutcome {
    Running,
    /// The command exited with 0.
    Ok,
    /// It exited with another code, could not be started, or its end is unknown.
    Failed,
    /// Nothing was started; the reason says why.
    Skipped,
}

/// The last thing that happened to a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleResult {
    pub outcome: ScheduleOutcome,
    /// Milliseconds since the epoch.
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A run the server started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRun {
    pub terminal_id: String,
    /// The due time the run belongs to.
    pub due_ms: u64,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
    /// Absent while it runs, and when its end is not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    pub outcome: ScheduleOutcome,
}

/// One schedule as the server sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleInfo {
    /// Identifies the schedule: its directory, mode, cron expression and time zone.
    pub key: String,
    pub cron: String,
    pub timezone: String,
    pub dir: String,
    pub mode: String,
    pub enabled: bool,
    /// The next due time; absent for a switched off schedule, during the global pause and for an
    /// expression that never matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<ScheduleResult>,
    /// Newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<ScheduleRun>,
}

/// Result of `schedule.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleList {
    /// `triggers.paused` of the user config: nothing starts.
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub schedules: Vec<ScheduleInfo>,
}

/// A failed run or collapsed skips that no app has acknowledged yet (#1308).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleAlert {
    /// Stays the same while skips are added to the alert.
    pub id: u64,
    /// Grows with every change of any alert; `schedule.ack_alerts` removes up to one.
    pub seq: u64,
    /// The key of the schedule.
    pub key: String,
    pub dir: String,
    pub mode: String,
    /// `failed` or `skipped`.
    pub outcome: ScheduleOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The last time, milliseconds since the epoch.
    pub at_ms: u64,
    /// How often it happened; always 1 for a failed run.
    pub count: u32,
}

/// Result of `schedule.alerts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleAlerts {
    /// In the order of `seq`, newest last.
    #[serde(default)]
    pub alerts: Vec<ScheduleAlert>,
}

/// Params of `schedule.ack_alerts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleAckAlerts {
    /// Every alert with a `seq` up to this one is removed.
    pub up_to: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_list_parses_with_runs_and_without_the_optional_fields() {
        let full = r#"{"paused":true,"schedules":[{"key":"/r|loop|0 9 * * *|Europe/Zurich",
            "cron":"0 9 * * *","timezone":"Europe/Zurich","dir":"/r","mode":"loop",
            "enabled":true,"next_ms":2000,
            "last":{"outcome":"skipped","at_ms":1000,"reason":"paused"},
            "runs":[{"terminal_id":"t3","due_ms":900,"started_ms":901,"ended_ms":950,
            "exit_code":1,"outcome":"failed"},
            {"terminal_id":"t4","due_ms":960,"started_ms":961,"outcome":"running"}]}]}"#;
        let list: ScheduleList = serde_json::from_str(full).unwrap();
        assert!(list.paused);
        let s = &list.schedules[0];
        assert_eq!(s.next_ms, Some(2000));
        assert_eq!(s.last.as_ref().unwrap().outcome, ScheduleOutcome::Skipped);
        assert_eq!(s.runs[0].exit_code, Some(1));
        assert_eq!(s.runs[1].outcome, ScheduleOutcome::Running);
        assert_eq!(s.runs[1].ended_ms, None);
        let back: ScheduleList =
            serde_json::from_str(&serde_json::to_string(&list).unwrap()).unwrap();
        assert_eq!(back, list);

        let bare: ScheduleList = serde_json::from_str(
            r#"{"schedules":[{"key":"k","cron":"* * * * *","timezone":"UTC","dir":"/r",
                "mode":"m","enabled":false}]}"#,
        )
        .unwrap();
        assert!(!bare.paused);
        assert_eq!(bare.schedules[0].next_ms, None);
        assert!(bare.schedules[0].runs.is_empty());
    }

    #[test]
    fn schedule_alerts_and_their_acknowledgement_parse() {
        let text = r#"{"alerts":[{"id":1,"seq":4,"key":"k","dir":"/r","mode":"loop",
            "outcome":"skipped","reason":"paused","at_ms":1000,"count":3},
            {"id":5,"seq":5,"key":"k","dir":"/r","mode":"loop","outcome":"failed",
            "at_ms":2000,"count":1}]}"#;
        let alerts: ScheduleAlerts = serde_json::from_str(text).unwrap();
        assert_eq!(alerts.alerts[0].count, 3);
        assert_eq!(alerts.alerts[0].seq, 4);
        assert_eq!(alerts.alerts[0].reason.as_deref(), Some("paused"));
        assert_eq!(alerts.alerts[1].outcome, ScheduleOutcome::Failed);
        assert_eq!(alerts.alerts[1].reason, None);
        let back: ScheduleAlerts =
            serde_json::from_str(&serde_json::to_string(&alerts).unwrap()).unwrap();
        assert_eq!(back, alerts);
        let ack: ScheduleAckAlerts = serde_json::from_str(r#"{"up_to":5}"#).unwrap();
        assert_eq!(ack, ScheduleAckAlerts { up_to: 5 });
    }
}
