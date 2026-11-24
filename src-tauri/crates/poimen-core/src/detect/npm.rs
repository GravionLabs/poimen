//! The scripts of the `package.json` at the repository root.

use std::path::Path;

use super::{DetectedEntry, Detector};

pub struct Npm;

/// The package manager of the repository: the `packageManager` field (`pnpm@9.1.0`), else the
/// lock file, else npm.
pub fn package_manager(root: &Path, manifest: &serde_json::Value) -> &'static str {
    const KNOWN: [&str; 4] = ["pnpm", "yarn", "bun", "npm"];
    if let Some(field) = manifest.get("packageManager").and_then(|v| v.as_str()) {
        let name = field.split('@').next().unwrap_or_default();
        if let Some(pm) = KNOWN.iter().find(|k| **k == name) {
            return pm;
        }
    }
    let has = |f: &str| root.join(f).is_file();
    if has("pnpm-lock.yaml") {
        "pnpm"
    } else if has("package-lock.json") {
        "npm"
    } else if has("yarn.lock") {
        "yarn"
    } else if has("bun.lock") || has("bun.lockb") {
        "bun"
    } else {
        "npm"
    }
}

impl Detector for Npm {
    fn detect(&self, root: &Path) -> Vec<DetectedEntry> {
        let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
            return Vec::new();
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Vec::new();
        };
        let Some(scripts) = manifest.get("scripts").and_then(|s| s.as_object()) else {
            return Vec::new();
        };
        let pm = package_manager(root, &manifest);
        scripts
            .iter()
            .filter_map(|(name, line)| {
                let line = line.as_str()?;
                (!name.is_empty() && !name.chars().any(char::is_control)).then(|| DetectedEntry {
                    source: "npm".into(),
                    name: name.clone(),
                    description: line.to_owned(),
                    argv: vec![pm.into(), "run".into(), name.clone()],
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;

    fn repo(files: &[(&str, &str)]) -> TestDir {
        let dir = TestDir::new("poimen-detect-npm-");
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    const PKG: &str = r#"{"scripts": {"build": "tsc -p .", "test": "vitest", "bad": 3}}"#;

    fn argvs(root: &Path) -> Vec<Vec<String>> {
        Npm.detect(root).into_iter().map(|e| e.argv).collect()
    }

    #[test]
    fn lists_scripts_with_their_command_line() {
        let root = repo(&[("package.json", PKG)]);
        let found = Npm.detect(&root);
        let names: Vec<_> = found.iter().map(|e| (&*e.name, &*e.description)).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&("build", "tsc -p .")));
        assert!(names.contains(&("test", "vitest")));
        let build = found.iter().find(|e| e.name == "build").unwrap();
        assert_eq!(build.argv, ["npm", "run", "build"]);
    }

    #[test]
    fn package_manager_from_each_lock_file() {
        for (lock, pm) in [
            ("pnpm-lock.yaml", "pnpm"),
            ("package-lock.json", "npm"),
            ("yarn.lock", "yarn"),
            ("bun.lock", "bun"),
            ("bun.lockb", "bun"),
        ] {
            let root = repo(&[("package.json", PKG), (lock, "")]);
            assert_eq!(argvs(&root)[0][0], pm, "{lock}");
        }
    }

    #[test]
    fn package_manager_field_wins_over_the_lock_file() {
        let pkg = r#"{"packageManager": "yarn@4.1.0", "scripts": {"a": "x"}}"#;
        let root = repo(&[("package.json", pkg), ("pnpm-lock.yaml", "")]);
        assert_eq!(argvs(&root), [["yarn", "run", "a"]]);
        let pkg = r#"{"packageManager": "unknown@1", "scripts": {"a": "x"}}"#;
        let root = repo(&[("package.json", pkg), ("pnpm-lock.yaml", "")]);
        assert_eq!(argvs(&root)[0][0], "pnpm");
    }

    #[test]
    fn a_name_with_a_control_character_is_not_listed() {
        let root = repo(&[(
            "package.json",
            r#"{"scripts": {"a\u0003b": "x", "line\nbreak": "x", "ok": "x"}}"#,
        )]);
        assert_eq!(argvs(&root), vec![vec!["npm", "run", "ok"]]);
    }

    #[test]
    fn missing_or_invalid_file_yields_nothing() {
        assert!(Npm.detect(&repo(&[])).is_empty());
        assert!(Npm.detect(&repo(&[("package.json", "{nope")])).is_empty());
        assert!(Npm.detect(&repo(&[("package.json", "{}")])).is_empty());
        assert!(
            Npm.detect(&repo(&[("package.json", r#"{"scripts": []}"#)]))
                .is_empty()
        );
    }

    #[test]
    fn a_workspace_root_lists_its_own_scripts_only() {
        let root = repo(&[(
            "package.json",
            r#"{"workspaces": ["packages/*"], "scripts": {"all": "pnpm -r build"}}"#,
        )]);
        std::fs::create_dir_all(root.join("packages/a")).unwrap();
        std::fs::write(
            root.join("packages/a/package.json"),
            r#"{"scripts": {"inner": "x"}}"#,
        )
        .unwrap();
        assert_eq!(argvs(&root), [["npm", "run", "all"]]);
    }
}
