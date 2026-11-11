//! The bounded output ring of a host: the latest PTY output records with their `seq`.

use std::collections::VecDeque;

/// Output records, oldest first, bounded by bytes and by count. The newest record is always kept,
/// so a client can tell where the stream stands.
#[derive(Debug)]
pub struct Ring {
    records: VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
    max_bytes: usize,
    max_records: usize,
    last_seq: u64,
}

/// What a client that has seen everything up to `since` gets.
#[derive(Debug, PartialEq, Eq)]
pub struct Replay<'a> {
    /// `from..=to` scrolled out of the ring.
    pub gap: Option<(u64, u64)>,
    pub records: Vec<(u64, &'a [u8])>,
}

/// `since` is a `seq` this host has not handed out yet.
#[derive(Debug, PartialEq, Eq)]
pub struct Ahead {
    pub last_seq: u64,
}

impl Ring {
    pub fn new(max_bytes: usize, max_records: usize) -> Self {
        Self {
            records: VecDeque::new(),
            bytes: 0,
            max_bytes,
            max_records: max_records.max(1),
            last_seq: 0,
        }
    }

    /// The `seq` of the newest record; 0 before the first.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Appends a record and returns its `seq` (one more than the previous, starting at 1).
    pub fn push(&mut self, data: Vec<u8>) -> u64 {
        self.last_seq += 1;
        self.bytes += data.len();
        self.records.push_back((self.last_seq, data));
        while self.records.len() > 1
            && (self.bytes > self.max_bytes || self.records.len() > self.max_records)
        {
            if let Some((_, old)) = self.records.pop_front() {
                self.bytes -= old.len();
            }
        }
        self.last_seq
    }

    /// The records after `since`, with the gap before them when the ring has moved past it.
    pub fn replay(&self, since: u64) -> Result<Replay<'_>, Ahead> {
        if since > self.last_seq {
            return Err(Ahead {
                last_seq: self.last_seq,
            });
        }
        let oldest = self
            .records
            .front()
            .map_or(self.last_seq + 1, |(seq, _)| *seq);
        let gap = (since + 1 < oldest).then(|| (since + 1, oldest - 1));
        let records = self
            .records
            .iter()
            .filter(|(seq, _)| *seq > since)
            .map(|(seq, data)| (*seq, data.as_slice()))
            .collect();
        Ok(Replay { gap, records })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_of(max_bytes: usize, max_records: usize, chunks: &[&str]) -> Ring {
        let mut ring = Ring::new(max_bytes, max_records);
        for c in chunks {
            ring.push(c.as_bytes().to_vec());
        }
        ring
    }

    fn seqs(replay: &Replay<'_>) -> Vec<u64> {
        replay.records.iter().map(|(s, _)| *s).collect()
    }

    #[test]
    fn seq_starts_at_one_and_rises_by_one_also_when_records_drop_out() {
        let mut ring = Ring::new(4, 100);
        assert_eq!(ring.last_seq(), 0);
        let got: Vec<u64> = (0..6).map(|_| ring.push(b"abc".to_vec())).collect();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(ring.last_seq(), 6);
    }

    #[test]
    fn an_empty_ring_replays_nothing_without_a_gap() {
        let ring = Ring::new(16, 16);
        assert_eq!(
            ring.replay(0),
            Ok(Replay {
                gap: None,
                records: vec![]
            })
        );
    }

    #[test]
    fn replay_gives_the_records_after_since() {
        let ring = ring_of(100, 100, &["a", "bb", "ccc"]);
        let all = ring.replay(0).unwrap();
        assert_eq!(all.gap, None);
        assert_eq!(
            all.records,
            vec![(1, &b"a"[..]), (2, &b"bb"[..]), (3, &b"ccc"[..])]
        );
        let rest = ring.replay(2).unwrap();
        assert_eq!(rest.records, vec![(3, &b"ccc"[..])]);
        assert_eq!(ring.replay(3).unwrap().records, vec![]);
    }

    #[test]
    fn the_byte_bound_drops_the_oldest_and_replay_reports_the_gap() {
        // 4 bytes: "cc" + "dd" stay, "a" and "bb" went.
        let ring = ring_of(4, 100, &["a", "bb", "cc", "dd"]);
        let replay = ring.replay(0).unwrap();
        assert_eq!(replay.gap, Some((1, 2)));
        assert_eq!(seqs(&replay), vec![3, 4]);
        // A client that saw record 1 misses record 2 only.
        assert_eq!(ring.replay(1).unwrap().gap, Some((2, 2)));
        // One that saw record 2 misses nothing.
        let after = ring.replay(2).unwrap();
        assert_eq!(after.gap, None);
        assert_eq!(seqs(&after), vec![3, 4]);
    }

    #[test]
    fn the_record_bound_applies_too() {
        let ring = ring_of(1000, 2, &["a", "b", "c"]);
        let replay = ring.replay(0).unwrap();
        assert_eq!(replay.gap, Some((1, 1)));
        assert_eq!(seqs(&replay), vec![2, 3]);
    }

    #[test]
    fn a_record_larger_than_the_ring_is_still_kept() {
        let ring = ring_of(2, 10, &["a", "0123456789"]);
        let replay = ring.replay(0).unwrap();
        assert_eq!(replay.gap, Some((1, 1)));
        assert_eq!(replay.records, vec![(2, &b"0123456789"[..])]);
    }

    #[test]
    fn a_since_ahead_of_the_ring_is_refused() {
        let ring = ring_of(10, 10, &["a"]);
        assert_eq!(ring.replay(2), Err(Ahead { last_seq: 1 }));
        assert_eq!(Ring::new(1, 1).replay(1), Err(Ahead { last_seq: 0 }));
    }
}
