use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poimen-history-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn history(dir: &Path, per_terminal: u64, total: u64) -> History {
    History::start(
        dir.join("history"),
        Limits {
            per_terminal,
            total,
        },
        dir.join("server.log"),
    )
}

fn out(text: &str) -> Record {
    Record::Output(text.as_bytes().to_vec())
}

#[test]
fn crc32_matches_the_standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn output_and_resize_round_trip_in_order() {
    let dir = temp("round");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"hello ");
    h.resize("t1", 100, 30);
    h.output("t1", "wörld\x1b[0m".as_bytes());
    h.output("t2", b"other");
    h.flush();
    assert_eq!(
        read(&dir.join("history").join("t1")),
        vec![
            out("hello "),
            Record::Resize {
                cols: 100,
                rows: 30
            },
            out("wörld\x1b[0m")
        ]
    );
    assert_eq!(read(&dir.join("history").join("t2")), vec![out("other")]);
    assert!(read(&dir.join("history").join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_torn_last_record_is_ignored() {
    let dir = temp("torn");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"one");
    h.output("t1", b"two");
    h.flush();
    let path = dir.join("history").join("t1");
    let data = std::fs::read(&path).unwrap();
    for cut in 1..(FRAME_HEAD + 4) {
        std::fs::write(&path, &data[..data.len() - cut]).unwrap();
        assert_eq!(read(&path), vec![out("one")], "cut {cut}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_flipped_bit_ends_the_list_at_the_damage() {
    let dir = temp("flip");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    for text in ["one", "two", "three"] {
        h.output("t1", text.as_bytes());
    }
    h.flush();
    let path = dir.join("history").join("t1");
    let mut data = std::fs::read(&path).unwrap();
    // Inside the payload of the second record.
    let second = MAGIC.len() + FRAME_HEAD + 1 + 3 + FRAME_HEAD + 2;
    data[second] ^= 0x01;
    std::fs::write(&path, &data).unwrap();
    assert_eq!(read(&path), vec![out("one")]);
    std::fs::write(&path, b"not a history").unwrap();
    assert!(read(&path).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_per_terminal_cap_drops_the_oldest_records() {
    let dir = temp("cap");
    let cap = 2000;
    let h = history(&dir, cap, DEFAULT_TOTAL);
    for i in 0..100 {
        h.output("t1", format!("{i:04}{}", "x".repeat(46)).as_bytes());
    }
    h.flush();
    let path = dir.join("history").join("t1");
    assert!(std::fs::metadata(&path).unwrap().len() <= cap);
    let records = read(&path);
    let Some(Record::Output(last)) = records.last() else {
        panic!("no records");
    };
    assert!(last.starts_with(b"0099"), "the newest is kept");
    let Some(Record::Output(first)) = records.first() else {
        panic!("no records");
    };
    assert!(!first.starts_with(b"0000"), "the oldest is gone");
    // The kept records are consecutive.
    let numbers: Vec<u32> = records
        .iter()
        .map(|r| match r {
            Record::Output(b) => std::str::from_utf8(&b[..4]).unwrap().parse().unwrap(),
            _ => panic!("not an output"),
        })
        .collect();
    assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_total_cap_drops_the_oldest_data_of_the_largest_log() {
    let dir = temp("total");
    let h = history(&dir, DEFAULT_PER_TERMINAL, 3000);
    for i in 0..10 {
        h.output("small", format!("s{i}").as_bytes());
    }
    for i in 0..100 {
        h.output("big", format!("{i:04}{}", "y".repeat(46)).as_bytes());
    }
    h.flush();
    let size = |id: &str| {
        std::fs::metadata(dir.join("history").join(id))
            .unwrap()
            .len()
    };
    assert!(size("small") + size("big") <= 3000);
    assert_eq!(read(&dir.join("history").join("small")).len(), 10);
    let big = read(&dir.join("history").join("big"));
    assert!(big.len() < 100);
    assert!(matches!(big.last(), Some(Record::Output(b)) if b.starts_with(b"0099")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_writer_continues_a_log_and_counts_it_in_the_total() {
    let dir = temp("again");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"before");
    h.flush();
    drop(h);
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"after");
    h.flush();
    assert_eq!(
        read(&dir.join("history").join("t1")),
        vec![out("before"), out("after")]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn the_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("mode");
    let h = history(&dir, 2000, DEFAULT_TOTAL);
    for _ in 0..100 {
        h.output("t1", &[b'z'; 50]);
    }
    h.flush();
    let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dir.join("history")), 0o700);
    // Also after a shrink rewrote the file.
    assert_eq!(mode(dir.join("history").join("t1")), 0o600);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_write_error_is_logged_once_and_does_not_stop_the_writer() {
    let dir = temp("error");
    // A file where the directory should be: every write fails.
    std::fs::write(dir.join("history"), b"in the way").unwrap();
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    for _ in 0..5 {
        h.output("t1", b"lost");
    }
    h.output("t2", b"lost");
    h.flush();
    let log = std::fs::read_to_string(dir.join("server.log")).unwrap();
    assert_eq!(
        log.matches("terminal t1: could not write").count(),
        1,
        "{log}"
    );
    assert_eq!(
        log.matches("terminal t2: could not write").count(),
        1,
        "{log}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_id_with_a_path_separator_writes_nothing() {
    let dir = temp("id");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("../evil", b"x");
    h.output("", b"x");
    h.flush();
    assert!(!dir.join("evil").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_disabled_history_writes_nothing() {
    let h = History::disabled();
    h.output("t1", b"x");
    h.resize("t1", 1, 1);
    h.flush();
}

#[test]
fn appends_after_a_torn_tail_of_an_earlier_run_stay_readable() {
    let dir = temp("resume");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"one");
    h.output("t1", b"two");
    h.flush();
    drop(h);
    let path = dir.join("history").join("t1");
    let data = std::fs::read(&path).unwrap();
    std::fs::write(&path, &data[..data.len() - 2]).unwrap();
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"three");
    h.flush();
    assert_eq!(read(&path), vec![out("one"), out("three")]);
    let _ = std::fs::remove_dir_all(dir);
}

fn checkpoint(seq: u64, grid: &str) -> Checkpoint {
    Checkpoint {
        seq,
        cols: 80,
        rows: 24,
        grid: grid.as_bytes().to_vec(),
    }
}

#[test]
fn a_checkpoint_and_the_seq_of_the_output_round_trip() {
    let dir = temp("checkpoint");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output_at("t1", b"abc", 7);
    h.checkpoint("t1", &checkpoint(7, "grid"));
    h.output_at("t1", b"def", 0);
    h.flush();
    assert_eq!(
        read(&dir.join("history").join("t1")),
        vec![
            out("abc"),
            Record::Seq(7),
            Record::Checkpoint(checkpoint(7, "grid")),
            out("def")
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_short_checkpoint_does_not_hide_the_records_after_it() {
    let dir = temp("short-checkpoint");
    let path = dir.join("log");
    let mut data = MAGIC.to_vec();
    data.extend(frame(&[KIND_CHECKPOINT, 1, 2]));
    data.extend(frame(&[KIND_OUTPUT, b'x']));
    std::fs::write(&path, data).unwrap();
    let records = read(&path);
    assert_eq!(records.len(), 2);
    assert!(matches!(&records[0], Record::Checkpoint(c) if c.cols == 0));
    assert_eq!(records[1], out("x"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eviction_keeps_a_checkpoint_as_the_first_record() {
    let dir = temp("evict-checkpoint");
    let h = history(&dir, 4000, DEFAULT_TOTAL);
    h.checkpoint("t1", &checkpoint(0, "empty"));
    for n in 0..60u32 {
        h.output_at(
            "t1",
            format!("{n:04}{}", "x".repeat(96)).as_bytes(),
            u64::from(n) + 1,
        );
        if n % 20 == 19 {
            h.checkpoint("t1", &checkpoint(u64::from(n) + 1, "grid"));
        }
    }
    h.flush();
    let records = read(&dir.join("history").join("t1"));
    assert!(
        matches!(records.first(), Some(Record::Checkpoint(_))),
        "{records:?}"
    );
    let size = std::fs::metadata(dir.join("history").join("t1"))
        .unwrap()
        .len();
    assert!(size <= 4000, "{size}");
    // What follows the first checkpoint is not cut: the output after it is consecutive.
    let numbers: Vec<u32> = records
        .iter()
        .filter_map(|r| match r {
            Record::Output(b) => Some(std::str::from_utf8(&b[..4]).unwrap().parse().unwrap()),
            _ => None,
        })
        .collect();
    assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1));
    assert_eq!(numbers.last(), Some(&59));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eviction_with_the_only_checkpoint_far_back_keeps_it_and_does_not_rewrite_on_every_write() {
    let dir = temp("evict-stuck");
    let h = history(&dir, 1000, DEFAULT_TOTAL);
    h.checkpoint("t1", &checkpoint(0, &"g".repeat(1500)));
    for n in 0..10u32 {
        h.output_at("t1", format!("{n:04}").as_bytes(), u64::from(n) + 1);
    }
    h.flush();
    let records = read(&dir.join("history").join("t1"));
    assert!(matches!(records.first(), Some(Record::Checkpoint(_))));
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, Record::Output(_)))
            .count(),
        10
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reset_removes_the_log_and_the_next_record_starts_a_new_one() {
    let dir = temp("reset");
    let h = history(&dir, DEFAULT_PER_TERMINAL, DEFAULT_TOTAL);
    h.output("t1", b"old");
    h.reset("t1");
    h.output("t1", b"new");
    h.flush();
    assert_eq!(read(&dir.join("history").join("t1")), vec![out("new")]);
    let _ = std::fs::remove_dir_all(dir);
}
