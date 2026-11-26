//! The alerts of the scheduler (#1308, ADR 0020 "Visibility"): every failed run, and the skips
//! collapsed to one alert per schedule and reason, kept in `server.json` until an app
//! acknowledges them or the cap drops them.

use poimen_protocol::{ScheduleAlert, ScheduleOutcome};
use serde::{Deserialize, Serialize};

/// At most this many are kept; the oldest go first.
const MAX_ALERTS: usize = 100;

/// What `server.json` keeps of the alerts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Alerts {
    /// The last `seq` given out; it survives the acknowledgement of every alert.
    #[serde(default)]
    seq: u64,
    /// In the order of `seq`, newest last.
    #[serde(default)]
    list: Vec<ScheduleAlert>,
}

/// The schedule an alert is about.
pub(crate) struct Subject<'a> {
    pub(crate) key: &'a str,
    pub(crate) dir: &'a str,
    pub(crate) mode: &'a str,
}

impl Alerts {
    pub(crate) fn is_unused(&self) -> bool {
        self.seq == 0 && self.list.is_empty()
    }

    /// Records a failed run or a skip; other outcomes give no alert.
    pub(crate) fn add(
        &mut self,
        schedule: Subject<'_>,
        outcome: ScheduleOutcome,
        reason: Option<&str>,
        at_ms: u64,
    ) {
        if !matches!(outcome, ScheduleOutcome::Failed | ScheduleOutcome::Skipped) {
            return;
        }
        self.seq += 1;
        let same = self.list.iter().position(|a| {
            outcome == ScheduleOutcome::Skipped
                && a.outcome == outcome
                && a.key == schedule.key
                && a.reason.as_deref() == reason
        });
        let alert = match same {
            Some(i) => {
                let mut alert = self.list.remove(i);
                alert.seq = self.seq;
                alert.at_ms = at_ms;
                alert.count = alert.count.saturating_add(1);
                alert
            }
            None => ScheduleAlert {
                id: self.seq,
                seq: self.seq,
                key: schedule.key.to_owned(),
                dir: schedule.dir.to_owned(),
                mode: schedule.mode.to_owned(),
                outcome,
                reason: reason.map(str::to_owned),
                at_ms,
                count: 1,
            },
        };
        self.list.push(alert);
        let over = self.list.len().saturating_sub(MAX_ALERTS);
        self.list.drain(..over);
    }

    /// Removes the alerts up to `up_to`; `false` when there was none.
    pub(crate) fn ack(&mut self, up_to: u64) -> bool {
        let before = self.list.len();
        self.list.retain(|a| a.seq > up_to);
        self.list.len() != before
    }

    pub(crate) fn list(&self) -> &[ScheduleAlert] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(alerts: &mut Alerts, key: &str, outcome: ScheduleOutcome, reason: &str, at: u64) {
        let subject = Subject {
            key,
            dir: "/r",
            mode: "job",
        };
        alerts.add(subject, outcome, Some(reason), at);
    }

    #[test]
    fn skips_collapse_per_schedule_and_reason_and_failures_do_not() {
        let mut a = Alerts::default();
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 1);
        add(&mut a, "k", ScheduleOutcome::Failed, "exit code 1", 2);
        add(&mut a, "other", ScheduleOutcome::Skipped, "paused", 3);
        add(&mut a, "k", ScheduleOutcome::Skipped, "busy", 4);
        add(&mut a, "k", ScheduleOutcome::Failed, "exit code 1", 5);
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 6);
        let got: Vec<_> = a
            .list()
            .iter()
            .map(|a| (a.id, a.seq, a.at_ms, a.count))
            .collect();
        assert_eq!(
            got,
            [
                (2, 2, 2, 1),
                (3, 3, 3, 1),
                (4, 4, 4, 1),
                (5, 5, 5, 1),
                (1, 6, 6, 2)
            ]
        );
    }

    #[test]
    fn a_run_that_is_ok_or_running_gives_no_alert() {
        let mut a = Alerts::default();
        add(&mut a, "k", ScheduleOutcome::Ok, "", 1);
        add(&mut a, "k", ScheduleOutcome::Running, "", 2);
        assert!(a.list().is_empty() && a.is_unused());
    }

    #[test]
    fn an_acknowledgement_keeps_what_changed_after_it_was_read() {
        let mut a = Alerts::default();
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 1);
        add(&mut a, "k", ScheduleOutcome::Failed, "exit code 1", 2);
        // A skip arrives between the read (up to 2) and the acknowledgement.
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 3);
        assert!(a.ack(2));
        assert_eq!(a.list().len(), 1);
        assert_eq!((a.list()[0].id, a.list()[0].count), (1, 2));
        assert!(!a.ack(2));
        assert!(a.ack(3));
        // The next skip is a new alert and its `seq` is not given twice.
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 4);
        assert_eq!(
            (a.list()[0].id, a.list()[0].seq, a.list()[0].count),
            (4, 4, 1)
        );
    }

    #[test]
    fn the_cap_drops_the_oldest_and_a_repeated_skip_is_not_the_oldest() {
        let mut a = Alerts::default();
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 0);
        for i in 1..100 {
            add(&mut a, "k", ScheduleOutcome::Failed, "exit code 1", i);
        }
        assert_eq!(a.list().len(), 100);
        add(&mut a, "k", ScheduleOutcome::Skipped, "paused", 100);
        add(&mut a, "k", ScheduleOutcome::Failed, "exit code 1", 101);
        assert_eq!(a.list().len(), 100);
        assert_eq!(a.list()[0].at_ms, 2);
        assert_eq!(a.list()[98].count, 2);
    }
}
