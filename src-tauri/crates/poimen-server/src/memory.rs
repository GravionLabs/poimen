//! Resident memory per terminal (#971): the child of a PTY host and its descendants.
//!
//! One `ps -A -o pid=,ppid=,rss=` run (Linux procps, macOS) gives the process table; it is kept
//! for [`MAX_AGE`] and read again only by a request after that, never by a poll. Windows has no
//! such command: the value stays absent there.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use poimen_core::gui_path;
use poimen_protocol::{Terminal, TerminalList, TerminalStatus};

use crate::{ApiResult, Shared, lock};

/// How old the process table may be before `terminal.list` reads it again.
pub(crate) const MAX_AGE: Duration = Duration::from_secs(30);

/// `pid` to (parent pid, resident kilobytes).
pub(crate) type Table = HashMap<u32, (u32, u64)>;

/// The process table and when it was read.
#[derive(Default)]
pub(crate) struct Cache {
    read: Option<(Instant, Option<Table>)>,
}

impl Cache {
    /// The table, read again when it is older than [`MAX_AGE`] at `now`. `None` where it cannot
    /// be read; that answer is cached as well, so a missing `ps` is not retried at every list.
    pub fn table(&mut self, now: Instant, read: impl FnOnce() -> Option<Table>) -> Option<&Table> {
        let fresh =
            matches!(&self.read, Some((at, _)) if now.saturating_duration_since(*at) < MAX_AGE);
        if !fresh {
            self.read = Some((now, read()));
        }
        self.read.as_ref().and_then(|(_, t)| t.as_ref())
    }
}

/// `terminal.list`: the terminals with the memory of the running ones (the table is read at
/// most every [`MAX_AGE`]).
pub(crate) fn terminal_list(shared: &Shared, p: TerminalList) -> ApiResult {
    shared.expire_tokens();
    let inner = lock(&shared.inner);
    let mut cache = lock(&shared.processes);
    let table = cache.table(Instant::now(), read_table);
    let terminals: Vec<Terminal> = inner
        .terminals
        .iter()
        .filter(|t| p.space_id.as_ref().is_none_or(|id| &t.info.space_id == id))
        .map(|t| {
            let mut info = t.info.clone();
            if info.status == TerminalStatus::Running {
                info.memory_bytes = table
                    .zip(t.host.as_ref().and_then(|h| h.child_pid()))
                    .and_then(|(table, pid)| tree_bytes(table, pid));
            }
            info
        })
        .collect();
    Ok(serde_json::json!({ "terminals": terminals }))
}

/// Runs `ps` and parses it; `None` when it is missing or fails.
pub(crate) fn read_table() -> Option<Table> {
    if cfg!(windows) {
        return None;
    }
    let out = gui_path::command("ps")
        .args(["-A", "-o", "pid=,ppid=,rss="])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Parses `pid ppid rss` lines (rss in kilobytes); lines that do not fit are skipped.
pub(crate) fn parse(text: &str) -> Table {
    text.lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace().map(|v| v.parse::<u64>().ok());
            let (pid, ppid, rss) = (f.next()??, f.next()??, f.next()??);
            Some((u32::try_from(pid).ok()?, (u32::try_from(ppid).ok()?, rss)))
        })
        .collect()
}

/// The resident bytes of `root` and all its descendants; `None` when `root` is not in the table.
pub(crate) fn tree_bytes(table: &Table, root: u32) -> Option<u64> {
    let (_, own) = table.get(&root)?;
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, (ppid, _)) in table {
        children.entry(*ppid).or_default().push(*pid);
    }
    let mut total = *own;
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::from([root]);
    while let Some(pid) = stack.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if seen.insert(*child) {
                total += table[child].1;
                stack.push(*child);
            }
        }
    }
    Some(total * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "    1     0  1000\n  10     1   200\n  11    10   300\n  12    11    50\n  20     1   999\nbad line\n";

    #[test]
    fn sums_the_child_and_its_descendants_only() {
        let t = parse(PS);
        assert_eq!(t.len(), 5);
        assert_eq!(tree_bytes(&t, 10), Some(550 * 1024));
        assert_eq!(tree_bytes(&t, 12), Some(50 * 1024));
        assert_eq!(tree_bytes(&t, 99), None);
    }

    #[test]
    fn a_cycle_is_counted_once() {
        let t = parse("1 2 10\n2 1 20\n");
        assert_eq!(tree_bytes(&t, 1), Some(30 * 1024));
    }

    #[test]
    fn the_table_is_read_at_most_every_30_seconds() {
        let mut cache = Cache::default();
        let start = Instant::now();
        let mut reads = 0;
        for secs in [0, 10, 29, 31, 40] {
            let now = start + Duration::from_secs(secs);
            cache.table(now, || {
                reads += 1;
                Some(parse(PS))
            });
        }
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_failed_read_is_cached_too() {
        let mut cache = Cache::default();
        let now = Instant::now();
        assert!(cache.table(now, || None).is_none());
        assert!(cache.table(now, || panic!("read again")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_real_table_holds_this_process() {
        let table = read_table().expect("ps");
        assert!(tree_bytes(&table, std::process::id()).is_some_and(|b| b > 0));
    }
}
