use super::*;
use crate::history::{Checkpoint, History, Limits};
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poimen-rebuild-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn history(dir: &Path) -> History {
    History::start(
        dir.join("history"),
        Limits::default(),
        dir.join("server.log"),
    )
}

fn checkpoint_of(e: &Emulator, seq: u64) -> Checkpoint {
    let (cols, rows) = e.size();
    Checkpoint {
        seq,
        cols,
        rows,
        grid: e.snapshot(),
    }
}

fn text(e: &Emulator) -> String {
    String::from_utf8_lossy(&e.snapshot()).into_owned()
}

#[test]
fn a_log_without_a_checkpoint_replays_from_its_start() {
    let dir = temp("start");
    let h = history(&dir);
    h.output_at("t", b"one\r\n", 1);
    h.resize("t", 50, 10);
    h.output_at("t", b"two", 2);
    h.flush();
    let r = rebuild(&dir.join("history").join("t"), (20, 5), 100).unwrap();
    assert_eq!(r.seq, 2);
    assert_eq!(r.emulator.size(), (50, 10));
    let s = text(&r.emulator);
    assert!(s.contains("one") && s.contains("two"), "{s:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_newest_checkpoint_is_the_start_and_the_records_after_it_are_replayed() {
    let dir = temp("newest");
    let h = history(&dir);
    let mut e = Emulator::new(20, 5, 100);
    e.feed(b"first");
    h.output_at("t", b"first", 1);
    h.checkpoint("t", &checkpoint_of(&e, 1));
    e.feed(b" second");
    h.output_at("t", b" second", 2);
    h.checkpoint("t", &checkpoint_of(&e, 2));
    h.output_at("t", b" third", 3);
    h.flush();
    let r = rebuild(&dir.join("history").join("t"), (20, 5), 100).unwrap();
    assert_eq!(r.seq, 3);
    assert!(text(&r.emulator).contains("first second third"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_invalid_checkpoint_falls_back_to_the_previous_one_else_to_the_start() {
    let dir = temp("invalid");
    let h = history(&dir);
    let mut e = Emulator::new(20, 5, 100);
    h.checkpoint("t", &checkpoint_of(&e, 0));
    e.feed(b"alpha");
    h.output_at("t", b"alpha", 1);
    h.checkpoint("t", &checkpoint_of(&e, 1));
    h.output_at("t", b" beta", 2);
    // Two ways to be invalid: no size, and a grid that is not a snapshot.
    h.checkpoint(
        "t",
        &Checkpoint {
            seq: 2,
            cols: 0,
            rows: 0,
            grid: Vec::new(),
        },
    );
    h.output_at("t", b" gamma", 3);
    h.checkpoint(
        "t",
        &Checkpoint {
            seq: 3,
            cols: 20,
            rows: 5,
            grid: b"garbage".to_vec(),
        },
    );
    h.flush();
    let path = dir.join("history").join("t");
    let r = rebuild(&path, (20, 5), 100).unwrap();
    assert_eq!(r.seq, 3);
    assert!(text(&r.emulator).contains("alpha beta gamma"));

    // Every checkpoint invalid: from the start, with the size the caller knows.
    let h = history(&dir);
    h.output_at("u", b"plain", 1);
    h.checkpoint(
        "u",
        &Checkpoint {
            seq: 1,
            cols: 0,
            rows: 0,
            grid: Vec::new(),
        },
    );
    h.flush();
    let r = rebuild(&dir.join("history").join("u"), (33, 7), 100).unwrap();
    assert_eq!(r.emulator.size(), (33, 7));
    assert!(text(&r.emulator).contains("plain"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_or_missing_log_never_fails() {
    let dir = temp("damaged");
    assert!(rebuild(&dir.join("none"), (20, 5), 100).is_none());
    let h = history(&dir);
    let mut e = Emulator::new(20, 5, 100);
    e.feed(b"kept");
    h.checkpoint("t", &checkpoint_of(&e, 1));
    h.output_at("t", b" more", 2);
    h.flush();
    let path = dir.join("history").join("t");
    let mut data = std::fs::read(&path).unwrap();
    // A flipped bit in the last record and a torn tail after it.
    let last = data.len() - 3;
    data[last] ^= 0xff;
    data.extend_from_slice(b"\x05\x00");
    std::fs::write(&path, &data).unwrap();
    let r = rebuild(&path, (20, 5), 100).unwrap();
    assert!(text(&r.emulator).contains("kept"));
    std::fs::write(&path, b"not a log at all").unwrap();
    assert!(rebuild(&path, (20, 5), 100).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_restored_checkpoint_answers_no_query_and_raises_no_signal() {
    let mut e = Emulator::new(20, 5, 100);
    e.feed(b"\x1b]7501;state=working\x07$ ");
    e.take_signals();
    let restored = restore(&checkpoint_of(&e, 1), 100).unwrap();
    let mut restored = restored;
    assert!(restored.take_signals().is_empty());
    assert!(restored.take_answers(&Colors::default()).all.is_empty());
}
