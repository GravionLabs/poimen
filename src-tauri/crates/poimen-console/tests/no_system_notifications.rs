//! poimen sends no OS notifications (#676): on macOS a waiting notification polls the
//! Notification Center from the main run loop until it is dismissed, and enough of them froze the
//! app's window after a few hours. Neither the console nor the app depends on a notification
//! crate.

use std::path::Path;

const CRATES: [&str; 3] = [
    "notify-rust",
    "mac-notification-sys",
    "tauri-plugin-notification",
];

fn manifest(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn no_manifest_depends_on_a_notification_crate() {
    let console = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [console.join("Cargo.toml"), console.join("../../Cargo.toml")] {
        let text = manifest(&path);
        for name in CRATES {
            assert!(
                !text.lines().any(|l| l.trim_start().starts_with(name)),
                "{} depends on {name}",
                path.display()
            );
        }
    }
}

#[test]
fn the_lockfile_has_no_notification_crate() {
    let lock = manifest(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"));
    for name in CRATES {
        assert!(
            !lock.contains(&format!("name = \"{name}\"")),
            "Cargo.lock still has {name}"
        );
    }
}
