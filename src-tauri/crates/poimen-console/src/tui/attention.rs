//! Attention marks and the notification list of the TUI, the rules of the app's
//! `core/attention.ts`: a terminal that turns blocked while not viewed rings, one that turns done
//! while not viewed counts as unread, viewing clears both. Everything here is pure (the clock and
//! the statuses are parameters); `mod.rs` feeds it after every read of the server's model.
//!
//! poimen sends no system notifications (#676); the list (`N`) is in the TUI only.

use std::collections::{HashMap, HashSet};

use poimen_client::protocol::AgentStatus;

/// The most entries the notification list keeps.
pub const LOG_CAP: usize = 50;

/// What a status change is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Blocked,
    Done,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Blocked => "blocked",
            Kind::Done => "done",
        }
    }
}

/// A terminal whose agent turned blocked or done since the previous read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub terminal_id: String,
    pub kind: Kind,
}

/// Marks of agent terminals. UI bookkeeping only: the server owns the status.
#[derive(Debug, Default)]
pub struct Attention {
    previous: Option<HashMap<String, AgentStatus>>,
    /// Terminals that turned blocked while not viewed.
    attention: HashSet<String>,
    /// Terminals that finished while not viewed.
    unread: HashSet<String>,
}

impl Attention {
    pub fn attention(&self) -> &HashSet<String> {
        &self.attention
    }

    pub fn unread(&self) -> &HashSet<String> {
        &self.unread
    }

    pub fn has_attention(&self, id: &str) -> bool {
        self.attention.contains(id)
    }

    pub fn is_unread(&self, id: &str) -> bool {
        self.unread.contains(id)
    }

    /// Terminals with a mark (ringing or unread).
    pub fn count(&self) -> usize {
        self.attention.union(&self.unread).count()
    }

    /// Folds the statuses of the agent terminals in; `viewed` are the terminals on screen.
    /// Returns those that turned blocked or done (terminals seen for the first time do not
    /// count as changes).
    pub fn observe(
        &mut self,
        statuses: &HashMap<String, AgentStatus>,
        viewed: &HashSet<String>,
    ) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut ids: Vec<&String> = statuses.keys().collect();
        ids.sort();
        for id in ids {
            let status = statuses[id];
            let before = self.previous.as_ref().and_then(|p| p.get(id)).copied();
            let kind = match status {
                AgentStatus::Blocked => Some(Kind::Blocked),
                AgentStatus::Done => Some(Kind::Done),
                _ => None,
            };
            if let (Some(before), Some(kind)) = (before, kind)
                && before != status
            {
                changes.push(Change {
                    terminal_id: id.clone(),
                    kind,
                });
            }
            if viewed.contains(id) {
                self.attention.remove(id);
                self.unread.remove(id);
            } else if before != Some(status) {
                if status == AgentStatus::Blocked {
                    self.attention.insert(id.clone());
                } else if status == AgentStatus::Done && before.is_some() {
                    self.unread.insert(id.clone());
                }
            }
            if status != AgentStatus::Blocked {
                self.attention.remove(id);
            }
        }
        self.attention.retain(|id| statuses.contains_key(id));
        self.unread.retain(|id| statuses.contains_key(id));
        self.previous = Some(statuses.clone());
        changes
    }
}

/// One line of the notification list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub terminal_id: String,
    /// The terminal's name when it turned.
    pub name: String,
    pub agent: Option<String>,
    pub kind: Kind,
    /// The agent's notification text, else a line saying what happened.
    pub text: String,
    /// Milliseconds since the epoch.
    pub at_ms: u64,
}

/// The notification list, newest first.
#[derive(Debug, Default)]
pub struct Log {
    entries: Vec<Entry>,
}

impl Log {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn push(&mut self, entry: Entry) {
        self.entries.insert(0, entry);
        self.entries.truncate(LOG_CAP);
    }

    /// Drops the entries of terminals that no longer exist.
    pub fn retain_terminals(&mut self, exists: impl Fn(&str) -> bool) {
        self.entries.retain(|e| exists(&e.terminal_id));
    }
}

/// `5 s`, `3 min`, `2 h`, `4 d`: how long ago `at_ms` was at `now_ms`.
pub fn age(now_ms: u64, at_ms: u64) -> String {
    let s = now_ms.saturating_sub(at_ms) / 1000;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        3600..=86_399 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentStatus::{Blocked, Done, Idle, Working};

    fn st(items: &[(&str, AgentStatus)]) -> HashMap<String, AgentStatus> {
        items.iter().map(|(i, s)| ((*i).into(), *s)).collect()
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).into()).collect()
    }

    fn change(id: &str, kind: Kind) -> Change {
        Change {
            terminal_id: id.into(),
            kind,
        }
    }

    #[test]
    fn blocked_while_not_viewed_rings_until_viewed() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working)]), &set(&[]));
        let changes = a.observe(&st(&[("a", Blocked)]), &set(&[]));
        assert_eq!(changes, vec![change("a", Kind::Blocked)]);
        assert!(a.has_attention("a"));
        assert_eq!(a.count(), 1);
        a.observe(&st(&[("a", Blocked)]), &set(&["a"]));
        assert!(!a.has_attention("a"));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn done_while_not_viewed_is_unread_until_viewed() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working)]), &set(&[]));
        let changes = a.observe(&st(&[("a", Done)]), &set(&[]));
        assert_eq!(changes, vec![change("a", Kind::Done)]);
        assert!(a.is_unread("a"));
        assert!(!a.has_attention("a"));
        // Still done on the next read: still unread, not a new change.
        assert!(a.observe(&st(&[("a", Done)]), &set(&[])).is_empty());
        assert!(a.is_unread("a"));
        a.observe(&st(&[("a", Done)]), &set(&["a"]));
        assert!(!a.is_unread("a"));
    }

    #[test]
    fn a_viewed_terminal_gets_no_marks_but_still_reports_the_change() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working)]), &set(&["a"]));
        let changes = a.observe(&st(&[("a", Blocked)]), &set(&["a"]));
        assert_eq!(changes.len(), 1);
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn leaving_blocked_clears_the_ring() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working)]), &set(&[]));
        a.observe(&st(&[("a", Blocked)]), &set(&[]));
        a.observe(&st(&[("a", Working)]), &set(&[]));
        assert!(!a.has_attention("a"));
    }

    #[test]
    fn first_sight_rings_when_blocked_but_done_is_neither_unread_nor_a_change() {
        let mut a = Attention::default();
        let changes = a.observe(&st(&[("b", Blocked), ("d", Done)]), &set(&[]));
        assert!(changes.is_empty());
        assert!(a.has_attention("b"));
        assert!(!a.is_unread("d"));
    }

    #[test]
    fn gone_terminals_lose_their_marks() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working), ("b", Working)]), &set(&[]));
        a.observe(&st(&[("a", Blocked), ("b", Done)]), &set(&[]));
        assert_eq!(a.count(), 2);
        a.observe(&st(&[]), &set(&[]));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn idle_is_not_a_change() {
        let mut a = Attention::default();
        a.observe(&st(&[("a", Working)]), &set(&[]));
        assert!(a.observe(&st(&[("a", Idle)]), &set(&[])).is_empty());
    }

    #[test]
    fn age_picks_the_unit() {
        assert_eq!(age(10_000, 5_000), "5 s");
        assert_eq!(age(200_000, 0), "3 min");
        assert_eq!(age(7_300_000, 0), "2 h");
        assert_eq!(age(200_000_000, 0), "2 d");
        assert_eq!(age(0, 5_000), "0 s");
    }

    #[test]
    fn the_log_is_newest_first_capped_and_pruned() {
        let mut log = Log::default();
        for i in 0..(LOG_CAP + 5) {
            log.push(Entry {
                terminal_id: format!("t{i}"),
                name: "n".into(),
                agent: None,
                kind: Kind::Done,
                text: String::new(),
                at_ms: i as u64,
            });
        }
        assert_eq!(log.entries().len(), LOG_CAP);
        assert_eq!(log.entries()[0].terminal_id, format!("t{}", LOG_CAP + 4));
        log.retain_terminals(|id| id == "t30");
        assert_eq!(log.entries().len(), 1);
    }
}
