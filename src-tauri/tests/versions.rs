//! One version for the app and the poimen-* crates (bug #477): the crates inherit the workspace version,
//! and the manifests that `release.yml` rewrites agree with it.

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn app_and_crates_share_the_workspace_version() {
    let workspace: toml::Value = read("Cargo.toml").parse().unwrap();
    let version = workspace["workspace"]["package"]["version"]
        .as_str()
        .unwrap();
    assert_eq!(
        version,
        env!("CARGO_PKG_VERSION"),
        "the app must inherit the workspace version"
    );
    for krate in ["poimen-protocol", "poimen-client", "poimen-server"] {
        let manifest: toml::Value = read(&format!("crates/{krate}/Cargo.toml")).parse().unwrap();
        assert_eq!(
            manifest["package"]["version"]["workspace"].as_bool(),
            Some(true),
            "{krate} must use `version.workspace = true`, or a release leaves it behind"
        );
    }
}

#[test]
fn lockfile_and_frontend_manifests_match() {
    let version = env!("CARGO_PKG_VERSION");
    let lock: toml::Value = read("Cargo.lock").parse().unwrap();
    for name in [
        "poimen-app",
        "poimen-protocol",
        "poimen-client",
        "poimen-server",
    ] {
        let entry = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{name} missing from Cargo.lock"));
        assert_eq!(
            entry["version"].as_str(),
            Some(version),
            "Cargo.lock entry of {name}"
        );
    }
    for path in ["tauri.conf.json", "../package.json"] {
        let json: serde_json::Value = serde_json::from_str(&read(path)).unwrap();
        assert_eq!(json["version"].as_str(), Some(version), "{path}");
    }
}
