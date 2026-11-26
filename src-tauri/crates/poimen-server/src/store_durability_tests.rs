//! The tests of the durability of `server.json`: the backup, the damaged files, the version (#967).

use std::fs;
use std::path::{Path, PathBuf};

use super::tests::{layout, temp};
use super::*;

/// A fresh directory, so files of an earlier run do not count; keep the guard, it removes it on drop.
fn fresh(name: &str) -> (tempfile::TempDir, PathBuf) {
    temp(name)
}

fn damaged_files(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("server.json.damaged-"))
        .collect();
    names.sort();
    names
}

fn ids(stored: &Stored) -> Vec<&str> {
    stored.layout.spaces.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn nothing_on_disk_gives_no_spaces() {
    let (_dir, path) = fresh("missing");
    let loaded = load(&path).unwrap();
    assert!(loaded.layout.spaces.is_empty());
    assert_eq!(loaded.next_id, 1);
    assert!(damaged_files(&path).is_empty());
}

#[test]
fn a_damaged_file_is_set_aside_and_the_backup_is_loaded() {
    let (_dir, path) = fresh("bad-bak");
    save(&path, &layout(&["s1"]), 5, Vec::new(), Vec::new()).unwrap();
    save(&path, &layout(&["s1", "s2"]), 6, Vec::new(), Vec::new()).unwrap();
    fs::write(&path, "{ nope").unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(ids(&loaded), ["s1"]);
    let aside = damaged_files(&path);
    assert_eq!(aside.len(), 1);
    assert_eq!(
        fs::read_to_string(path.with_file_name(&aside[0])).unwrap(),
        "{ nope"
    );
    assert!(!path.exists(), "the damaged file is moved, not rewritten");
    // The next save writes a new main file and keeps the damaged copy.
    save(
        &path,
        &loaded.layout,
        loaded.next_id,
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(damaged_files(&path), aside);
}

#[test]
fn a_damaged_file_without_a_backup_starts_empty_and_is_kept() {
    let (_dir, path) = fresh("bad-nobak");
    fs::write(&path, "{ nope").unwrap();
    let loaded = load(&path).unwrap();
    assert!(loaded.layout.spaces.is_empty());
    assert_eq!(loaded.next_id, 1);
    assert_eq!(damaged_files(&path).len(), 1);
    assert!(!path.exists());
}

#[test]
fn a_missing_file_loads_the_backup() {
    let (_dir, path) = fresh("missing-bak");
    save(&path, &layout(&["s1"]), 5, Vec::new(), Vec::new()).unwrap();
    save(&path, &layout(&["s1", "s2"]), 6, Vec::new(), Vec::new()).unwrap();
    fs::remove_file(&path).unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(ids(&loaded), ["s1"]);
    assert!(damaged_files(&path).is_empty());
}

#[test]
fn a_damaged_backup_is_set_aside_too() {
    let (_dir, path) = fresh("bad-bak-only");
    fs::write(backup_path(&path), "[").unwrap();
    assert!(load(&path).unwrap().layout.spaces.is_empty());
    assert!(!backup_path(&path).exists());
    assert_eq!(
        damaged_files(&path).len(),
        0,
        "only server.json.* names count"
    );
    let dir = path.parent().unwrap();
    assert!(fs::read_dir(dir).unwrap().flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with("server.json.bak.damaged-")
    }));
}

#[test]
fn at_most_three_damaged_files_are_kept_and_the_oldest_goes() {
    let (_dir, path) = fresh("cap");
    let dir = path.parent().unwrap();
    for secs in [100, 200, 300] {
        fs::write(dir.join(format!("server.json.damaged-{secs}")), "x").unwrap();
    }
    fs::write(&path, "{ nope").unwrap();
    load(&path).unwrap();
    let names = damaged_files(&path);
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!names.contains(&"server.json.damaged-100".to_string()));
    assert!(names.contains(&"server.json.damaged-200".to_string()));
}

#[test]
fn a_newer_version_is_refused_and_the_file_stays_untouched() {
    let (_dir, path) = fresh("newer");
    let text = r#"{"version":2,"spaces":[],"future":{"x":1}}"#;
    fs::write(&path, text).unwrap();
    let newer = load(&path).unwrap_err();
    assert_eq!((newer.found, newer.known), (2, VERSION));
    assert!(newer.to_string().contains("version 2"), "{newer}");
    assert!(newer.to_string().contains("server.json"), "{newer}");
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert!(damaged_files(&path).is_empty());
    assert!(!backup_path(&path).exists());
}

#[test]
fn a_file_without_a_version_is_read_and_written_with_one() {
    let (_dir, path) = fresh("noversion");
    fs::write(&path, r#"{"spaces":[{"id":"s1","name":"a","cwd":"/x"}]}"#).unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(ids(&loaded), ["s1"]);
    save(
        &path,
        &loaded.layout,
        loaded.next_id,
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\": 1")
    );
}

#[test]
fn a_save_keeps_the_previous_file_as_backup() {
    let (_dir, path) = fresh("bak");
    save(&path, &layout(&["s1"]), 2, Vec::new(), Vec::new()).unwrap();
    assert!(!backup_path(&path).exists());
    save(&path, &layout(&["s2"]), 3, Vec::new(), Vec::new()).unwrap();
    assert_eq!(ids(&load(&path).unwrap()), ["s2"]);
    let previous = fs::read_to_string(backup_path(&path)).unwrap();
    assert!(previous.contains("\"s1\""), "{previous}");
    assert!(!path.with_extension("json.tmp").exists());
}

#[cfg(unix)]
#[test]
fn a_damaged_file_that_cannot_be_moved_aside_refuses_the_start_and_keeps_the_backup() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = fresh("stuck");
    save(&path, &layout(&["s1"]), 2, Vec::new(), Vec::new()).unwrap();
    save(&path, &layout(&["s1", "s2"]), 3, Vec::new(), Vec::new()).unwrap();
    fs::write(&path, "{ nope").unwrap();
    let dir = path.parent().unwrap();
    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
    let result = load(&path);
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    if fs::write(dir.join("probe"), "").is_ok() && result.is_ok() {
        return; // privileged user: permissions do not apply
    }
    let refused = result.unwrap_err();
    assert!(
        refused.to_string().contains("could not be moved aside"),
        "{refused}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ nope");
    assert!(
        fs::read_to_string(backup_path(&path))
            .unwrap()
            .contains("\"s1\"")
    );
}
