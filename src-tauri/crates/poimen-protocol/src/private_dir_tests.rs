use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fresh(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poimen-pd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_0700_dir_of_ours_passes() {
    let dir = fresh("ok");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    check_private_dir(&dir, false).unwrap();
}

#[test]
fn a_0777_dir_is_fixed_with_fix_and_refused_without() {
    let dir = fresh("loose");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let err = check_private_dir(&dir, false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(mode(&dir), 0o777);
    check_private_dir(&dir, true).unwrap();
    assert_eq!(mode(&dir), 0o700);
}

#[test]
fn a_symlink_to_a_dir_is_refused_even_with_fix() {
    let target = fresh("target");
    let link = std::env::temp_dir().join(format!("poimen-pd-{}-link", std::process::id()));
    let _ = std::fs::remove_file(&link);
    symlink(&target, &link).unwrap();
    let err = check_private_dir(&link, true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    std::fs::remove_file(&link).unwrap();
}

#[test]
fn a_dir_of_another_user_is_refused() {
    // `/` is owned by root; skipped when the tests run as root.
    if current_uid().unwrap() == 0 {
        return;
    }
    let err = check_private_dir(std::path::Path::new("/"), true).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn a_tab_parent_is_optional_on_the_wire() {
    let tab: Tab = serde_json::from_value(serde_json::json!({
        "tab_id": "tab3", "space_id": "s1", "name": "logs",
        "root": {"kind": "terminal", "terminal_id": "t3"},
        "active_terminal": "t3", "parent": "tab2"
    }))
    .unwrap();
    assert_eq!(tab.parent.as_deref(), Some("tab2"));
    let top: Tab = serde_json::from_value(serde_json::json!({
        "tab_id": "tab2", "space_id": "s1", "name": "claude",
        "root": {"kind": "terminal", "terminal_id": "t2"}
    }))
    .unwrap();
    assert_eq!(top.parent, None);
    assert!(serde_json::to_value(&top).unwrap().get("parent").is_none());
}

#[test]
fn a_missing_dir_is_not_found() {
    let err = check_private_dir(&fresh("m").join("nope"), false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
