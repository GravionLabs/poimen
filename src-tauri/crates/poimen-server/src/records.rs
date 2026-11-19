//! The child records of a terminal (#684): what a program reports with OSC 7501 and an `id`.
//!
//! Pure state, bounded: at most [`MAX_CHILD_RECORDS`] records; the list is ordered by last update,
//! so the first one is the one evicted.

use poimen_protocol::{AgentInfo, AgentStatus, ChildRecord, MAX_CHILD_RECORDS};

use crate::emulator::{Report, ReportState};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Records {
    /// Least recently updated first, as reported (`app` is not resolved).
    list: Vec<ChildRecord>,
}

fn status_of(state: ReportState) -> Option<AgentStatus> {
    match state {
        ReportState::Idle => Some(AgentStatus::Idle),
        ReportState::Working => Some(AgentStatus::Working),
        ReportState::Blocked => Some(AgentStatus::Blocked),
        ReportState::Done => Some(AgentStatus::Done),
        ReportState::Error => Some(AgentStatus::Error),
        ReportState::Clear => None,
    }
}

/// `id` is `ancestor` or below it.
fn within(id: &str, ancestor: &str) -> bool {
    id == ancestor
        || id
            .strip_prefix(ancestor)
            .is_some_and(|r| r.starts_with('/'))
}

impl Records {
    /// The records as stored (for `server.json`); a restart gets them back through [`Self::from`].
    pub(crate) fn raw(&self) -> &[ChildRecord] {
        &self.list
    }

    /// Stored records, cut to the bounds again (a hand-edited file stays bounded).
    pub(crate) fn from(mut list: Vec<ChildRecord>) -> Self {
        list.retain(|r| poimen_protocol::valid_record_id(&r.id));
        let mut seen = std::collections::HashSet::new();
        // The last of a duplicated id is the newer.
        list.reverse();
        list.retain(|r| seen.insert(r.id.clone()));
        list.reverse();
        if list.len() > MAX_CHILD_RECORDS {
            list.drain(..list.len() - MAX_CHILD_RECORDS);
        }
        Records { list }
    }

    /// A report with an `id`: replaces the record of that id (or removes the subtree on `clear`).
    pub(crate) fn apply(&mut self, report: Report, now_ms: u64) {
        let Some(id) = report.id else { return };
        let Some(state) = report.state else { return };
        let Some(status) = status_of(state) else {
            return self.remove_tree(&id);
        };
        let before = self
            .list
            .iter()
            .position(|r| r.id == id)
            .map(|i| self.list.remove(i));
        let text = report
            .msg
            .as_deref()
            .and_then(poimen_protocol::clean_message);
        // The time of a message that did not change stays.
        let message_at = match (&text, &before) {
            (Some(t), Some(b)) if b.message.as_ref() == Some(t) => b.message_at,
            (Some(_), _) => Some(now_ms),
            (None, _) => None,
        };
        if self.list.len() >= MAX_CHILD_RECORDS {
            self.list.remove(0);
        }
        self.list.push(ChildRecord {
            id,
            status,
            app: report.app,
            message: text,
            message_at,
            progress: report.progress,
            kind: report.kind.filter(|_| status == AgentStatus::Blocked),
            title: report
                .title
                .as_deref()
                .and_then(poimen_protocol::clean_message),
        });
    }

    /// `clear` with an `id`: the record and all its children.
    pub(crate) fn remove_tree(&mut self, id: &str) {
        self.list.retain(|r| !within(&r.id, id));
    }

    /// `clear` without an `id`, RIS.
    pub(crate) fn clear(&mut self) {
        self.list.clear();
    }

    /// A new prompt (OSC 133 `A`): what was working or blocked is over.
    pub(crate) fn end_active(&mut self) {
        self.list
            .retain(|r| !matches!(r.status, AgentStatus::Working | AgentStatus::Blocked));
    }

    /// The program exited: like the root record, only `idle` and `done` stay.
    pub(crate) fn exited(&mut self) {
        self.list
            .retain(|r| matches!(r.status, AgentStatus::Idle | AgentStatus::Done));
    }

    /// The records as clients see them: `app` from the nearest ancestor record, else the root's.
    pub(crate) fn view(&self, root: Option<&AgentInfo>) -> Vec<ChildRecord> {
        self.list
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.app.is_none() {
                    r.app = self.inherited(&r.id, root);
                }
                r
            })
            .collect()
    }

    fn inherited(&self, id: &str, root: Option<&AgentInfo>) -> Option<String> {
        let mut path = id;
        while let Some((parent, _)) = path.rsplit_once('/') {
            if let Some(app) = self
                .list
                .iter()
                .find(|r| r.id == parent)
                .and_then(|r| r.app.clone())
            {
                return Some(app);
            }
            path = parent;
        }
        root.and_then(|a| a.agent.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: &str, state: ReportState) -> Report {
        Report {
            id: Some(id.into()),
            state: Some(state),
            ..Report::default()
        }
    }

    fn ids(r: &Records) -> Vec<&str> {
        r.raw().iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn a_report_replaces_its_record() {
        let mut r = Records::default();
        let mut first = report("run/a", ReportState::Working);
        first.progress = Some(10);
        first.msg = Some("hi".into());
        r.apply(first, 1);
        r.apply(report("run/a", ReportState::Done), 2);
        assert_eq!(r.raw().len(), 1);
        assert_eq!(r.raw()[0].status, AgentStatus::Done);
        assert_eq!(
            (r.raw()[0].progress, r.raw()[0].message.clone()),
            (None, None)
        );
    }

    #[test]
    fn an_unchanged_message_keeps_its_time() {
        let mut r = Records::default();
        let mut a = report("a", ReportState::Blocked);
        a.msg = Some("same".into());
        r.apply(a.clone(), 10);
        r.apply(a, 20);
        assert_eq!(r.raw()[0].message_at, Some(10));
    }

    #[test]
    fn clear_with_an_id_removes_the_subtree_only() {
        let mut r = Records::default();
        for id in ["run", "run/a", "run/a/x", "run/ab", "other"] {
            r.apply(report(id, ReportState::Working), 1);
        }
        r.apply(report("run/a", ReportState::Clear), 2);
        assert_eq!(ids(&r), ["run", "run/ab", "other"]);
        r.clear();
        assert!(r.raw().is_empty());
    }

    #[test]
    fn the_least_recently_updated_record_is_evicted() {
        let mut r = Records::default();
        for i in 0..MAX_CHILD_RECORDS {
            r.apply(report(&format!("r{i}"), ReportState::Working), 1);
        }
        // Updating r0 makes r1 the oldest.
        r.apply(report("r0", ReportState::Done), 2);
        r.apply(report("new", ReportState::Working), 3);
        assert_eq!(r.raw().len(), MAX_CHILD_RECORDS);
        assert!(!ids(&r).contains(&"r1"));
        assert!(ids(&r).contains(&"r0") && ids(&r).contains(&"new"));
    }

    #[test]
    fn app_is_inherited_from_the_nearest_ancestor_then_the_root() {
        let mut r = Records::default();
        let mut run = report("run", ReportState::Working);
        run.app = Some("codex".into());
        r.apply(run, 1);
        r.apply(report("run/a/b", ReportState::Working), 1);
        let mut own = report("run/a/c", ReportState::Working);
        own.app = Some("own".into());
        r.apply(own, 1);
        r.apply(report("lone", ReportState::Working), 1);
        let root = AgentInfo {
            reported_at: None,
            status_since: None,
            silent_since: None,
            status: AgentStatus::Working,
            agent: Some("rootapp".into()),
            session_id: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            title: None,
        };
        let apps: Vec<_> = r
            .view(Some(&root))
            .into_iter()
            .map(|r| r.app.unwrap())
            .collect();
        assert_eq!(apps, ["codex", "codex", "own", "rootapp"]);
        assert_eq!(r.view(None)[3].app, None);
    }

    #[test]
    fn lifetime_follows_the_root_record() {
        let mut r = Records::default();
        for (id, s) in [
            ("w", ReportState::Working),
            ("b", ReportState::Blocked),
            ("d", ReportState::Done),
            ("i", ReportState::Idle),
            ("e", ReportState::Error),
        ] {
            r.apply(report(id, s), 1);
        }
        r.end_active();
        assert_eq!(ids(&r), ["d", "i", "e"]);
        r.exited();
        assert_eq!(ids(&r), ["d", "i"]);
    }

    #[test]
    fn stored_records_are_bounded_again() {
        let rec = |id: &str| ChildRecord {
            id: id.into(),
            status: AgentStatus::Done,
            app: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            title: None,
        };
        let mut list: Vec<_> = (0..MAX_CHILD_RECORDS + 5)
            .map(|i| rec(&format!("r{i}")))
            .collect();
        list.push(rec("bad id"));
        list.push(rec("r10"));
        let r = Records::from(list);
        assert_eq!(r.raw().len(), MAX_CHILD_RECORDS);
        assert_eq!(r.raw().last().unwrap().id, "r10");
        assert!(!ids(&r).contains(&"bad id"));
    }
}
