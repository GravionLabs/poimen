//! Creating a space (herdr workspace) for a directory, and listing directories for path completion.
//! The matching mirrors the herdr-plugins `repo-open`: focus the existing space, else create one.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

/// What `create_space` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceOutcome {
    pub workspace_id: String,
    /// False when an existing space for the directory was focused instead.
    pub created: bool,
    /// Why the layout of a new space was not applied (or only partly); empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Whether the `setup` command of the project file was started in a `setup` tab. A setup that
    /// fails to start is a warning; the space stays.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub setup_started: bool,
    /// Set instead of creating anything when the project file of the repository is not trusted
    /// yet: the commands to show. `workspace_id` is empty then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<crate::trust::TrustRequest>,
}

/// Whether two directories are the same, ignoring trailing slashes (`/` stays `/`).
fn same_dir(a: &str, b: &str) -> bool {
    fn trim(s: &str) -> &str {
        match s.trim_end_matches('/') {
            "" if s.starts_with('/') => "/",
            t => t,
        }
    }
    trim(a) == trim(b)
}

fn pane_dir(pane: &Value) -> Option<&str> {
    ["foreground_cwd", "cwd"]
        .iter()
        .filter_map(|k| pane[k].as_str())
        .find(|s| !s.is_empty())
}

fn panes_of<'a>(workspace: &Value, panes: &'a [Value]) -> Vec<&'a Value> {
    let id = workspace["workspace_id"].as_str();
    panes
        .iter()
        .filter(|p| id.is_some() && p["workspace_id"].as_str() == id)
        .collect()
}

/// The space of `dir`: the first with a pane exactly in the directory (a pane in an ancestor or in
/// a subdirectory does not count), else the first whose own `cwd` (poimen backend) is exactly the
/// directory, else one labelled with the directory name whose own `cwd` (if any) and panes report
/// no directory other than `dir`. `None` when there is none.
pub fn find_space(dir: &str, workspaces: &[Value], panes: &[Value]) -> Option<String> {
    let id = |w: &Value| w["workspace_id"].as_str().map(str::to_owned);
    let by_pane = workspaces.iter().find(|w| {
        panes_of(w, panes)
            .iter()
            .any(|p| pane_dir(p).is_some_and(|d| same_dir(dir, d)))
    });
    if let Some(w) = by_pane {
        return id(w);
    }
    // A space of the poimen backend knows its directory even without terminals.
    let by_own_dir = workspaces.iter().find(|w| {
        w["cwd"]
            .as_str()
            .is_some_and(|d| !d.is_empty() && same_dir(dir, d))
    });
    if let Some(w) = by_own_dir {
        return id(w);
    }
    let name = Path::new(dir).file_name()?.to_str()?;
    workspaces
        .iter()
        .find(|w| {
            w["label"].as_str() == Some(name)
                && w["cwd"]
                    .as_str()
                    .is_none_or(|d| d.is_empty() || same_dir(dir, d))
                && panes_of(w, panes)
                    .iter()
                    .all(|p| pane_dir(p).is_none_or(|d| same_dir(dir, d)))
        })
        .and_then(id)
}

/// The label of a new space: the wanted one (default: the directory name), or the path relative to
/// `home` (else the full path) when another space already uses it.
pub fn space_label(dir: &str, wanted: &str, home: Option<&Path>, workspaces: &[Value]) -> String {
    let wanted = wanted.trim();
    let wanted = if wanted.is_empty() {
        Path::new(dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(dir)
    } else {
        wanted
    };
    if !workspaces
        .iter()
        .any(|w| w["label"].as_str() == Some(wanted))
    {
        return wanted.to_owned();
    }
    home.and_then(|h| Path::new(dir).strip_prefix(h).ok())
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
        .unwrap_or(dir)
        .to_owned()
}

/// Params of `workspace.create`.
pub fn create_params(cwd: &str, label: &str) -> Value {
    json!({ "cwd": cwd, "label": label })
}

/// Params of `workspace.focus`.
pub fn focus_params(workspace_id: &str) -> Value {
    json!({ "workspace_id": workspace_id })
}

/// The workspace id of a `workspace.create` result.
pub fn created_workspace_id(result: &Value) -> Option<String> {
    result["workspace"]["workspace_id"]
        .as_str()
        .map(str::to_owned)
}

fn expand_home(input: &str, home: Option<&Path>) -> PathBuf {
    match (input.strip_prefix('~'), home) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            h.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(input),
    }
}

/// Expands a leading `~` and checks that the path is an existing directory.
pub fn resolve_dir(input: &str, home: Option<&Path>) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a directory.".into());
    }
    let path = expand_home(input, home);
    if !path.is_absolute() {
        return Err(format!("{input} is not an absolute path."));
    }
    if !path.is_dir() {
        return Err(format!("{input} is not a directory."));
    }
    let s = path.to_string_lossy().into_owned();
    // Keep a root (`/`, `C:\`) as it is; any other path loses its trailing separators.
    Ok(if path.file_name().is_some() {
        s.trim_end_matches(is_separator).to_owned()
    } else {
        s
    })
}

/// A path separator: `/` everywhere, `\` too on Windows (an ordinary character elsewhere).
fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Names of the subdirectories of `dir` (symlinks to directories included); empty when `dir` is
/// unreadable. Unsorted.
fn subdir_names(dir: impl AsRef<Path>) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// A directory of the explorer tree.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

/// The subdirectories of `path` (a leading `~` expands), sorted by name; hidden ones (leading dot)
/// only with `hidden`. A relative, missing or unreadable `path` has none: no error.
pub fn child_dirs(path: &str, home: Option<&Path>, hidden: bool) -> Vec<DirEntry> {
    let dir = expand_home(path.trim(), home);
    if !dir.is_absolute() {
        return Vec::new();
    }
    let mut names: Vec<String> = subdir_names(&dir)
        .into_iter()
        .filter(|n| hidden || !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
        .into_iter()
        .map(|name| DirEntry {
            path: dir.join(&name).to_string_lossy().into_owned(),
            name,
        })
        .collect()
}

/// Directories matching a typed path: the subdirectories of the typed directory whose name starts
/// with the typed rest (`/a/b` lists `/a` entries starting with `b`; `/a/` lists all of `/a`).
/// Hidden ones only show when the typed rest starts with a dot. Sorted, at most `limit`.
pub fn list_dirs(input: &str, home: Option<&Path>, limit: usize) -> Vec<String> {
    let expanded = expand_home(input, home).to_string_lossy().into_owned();
    // `~/` keeps its trailing separator through the join only when typed; restore it.
    let expanded = if input.ends_with(is_separator) && !expanded.ends_with(is_separator) {
        format!("{expanded}/")
    } else {
        expanded
    };
    if !Path::new(&expanded).is_absolute() {
        return Vec::new();
    }
    let Some(slash) = expanded.rfind(is_separator) else {
        return Vec::new();
    };
    // `parent` keeps its separator, so a root (`/`, `C:\`) works like any directory.
    let (parent, prefix) = (&expanded[..=slash], &expanded[slash + 1..]);
    let mut dirs: Vec<String> = subdir_names(parent)
        .into_iter()
        .filter(|n| n.starts_with(prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
        .map(|n| format!("{parent}{n}"))
        .collect();
    dirs.sort();
    dirs.truncate(limit);
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str, label: &str) -> Value {
        json!({ "workspace_id": id, "label": label })
    }

    fn pane(ws: &str, cwd: &str) -> Value {
        json!({ "workspace_id": ws, "cwd": cwd })
    }

    #[test]
    fn a_space_with_a_pane_in_an_ancestor_directory_does_not_match() {
        let workspaces = [ws("w1", "other"), ws("w2", "app")];
        let panes = [pane("w1", "/work"), pane("w2", "/elsewhere")];
        assert_eq!(find_space("/work/app", &workspaces, &panes), None);
        // Not even through the label: the pane is in another directory.
        let labelled = [ws("w1", "app")];
        assert_eq!(find_space("/work/app", &labelled, &panes[..1]), None);
    }

    #[test]
    fn an_equal_pane_wins_over_an_ancestor_pane_in_an_earlier_space() {
        let workspaces = [ws("w1", "gravionlabs"), ws("w2", "x")];
        let panes = [
            pane("w1", "/src/gravionlabs"),
            pane("w2", "/src/gravionlabs/app"),
        ];
        assert_eq!(
            find_space("/src/gravionlabs/app", &workspaces, &panes),
            Some("w2".into())
        );
    }

    #[test]
    fn matches_the_space_with_a_pane_in_the_same_directory() {
        let workspaces = [ws("w1", "other")];
        let panes = [pane("w1", "/work/app/")];
        assert_eq!(
            find_space("/work/app", &workspaces, &panes),
            Some("w1".into())
        );
    }

    #[test]
    fn a_space_with_panes_only_in_a_subdirectory_does_not_match() {
        let workspaces = [ws("w1", "other")];
        let panes = [pane("w1", "/work/app/src")];
        assert_eq!(find_space("/work/app", &workspaces, &panes), None);
        // Not even through the label.
        let labelled = [ws("w1", "app")];
        assert_eq!(find_space("/work/app", &labelled, &panes), None);
    }

    #[test]
    fn prefers_the_foreground_directory_of_a_pane() {
        let workspaces = [ws("w1", "x")];
        let panes = [json!({ "workspace_id": "w1", "cwd": "/a", "foreground_cwd": "/work/app" })];
        assert_eq!(
            find_space("/work/app", &workspaces, &panes),
            Some("w1".into())
        );
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_not_inside() {
        let workspaces = [ws("w1", "x")];
        let panes = [pane("w1", "/work/app-two")];
        assert_eq!(find_space("/work/app", &workspaces, &panes), None);
    }

    #[test]
    fn the_label_fallback_matches_panes_without_a_directory_or_in_dir() {
        let workspaces = [ws("w1", "app")];
        let mixed = [pane("w1", "/work/app"), json!({ "workspace_id": "w1" })];
        assert_eq!(
            find_space("/work/app/", &workspaces, &mixed),
            Some("w1".into())
        );
    }

    fn ws_in(id: &str, label: &str, cwd: &str) -> Value {
        json!({ "workspace_id": id, "label": label, "cwd": cwd })
    }

    #[test]
    fn matches_a_space_without_panes_by_its_own_directory() {
        // A task space on the poimen backend whose terminals were all closed.
        let workspaces = [ws_in("w1", "#7 Add thing", "/work/app-7/")];
        assert_eq!(
            find_space("/work/app-7", &workspaces, &[]),
            Some("w1".into())
        );
    }

    #[test]
    fn the_space_directory_matches_the_exact_directory_only() {
        let workspaces = [
            ws_in("w1", "a", "/work"),
            ws_in("w2", "b", "/work/app-7/src"),
            ws_in("w3", "c", "/work/app-70"),
        ];
        assert_eq!(find_space("/work/app-7", &workspaces, &[]), None);
    }

    #[test]
    fn a_pane_in_the_directory_wins_over_an_earlier_space_directory() {
        let workspaces = [ws_in("w1", "a", "/work/app"), ws("w2", "b")];
        let panes = [pane("w1", "/elsewhere"), pane("w2", "/work/app")];
        assert_eq!(
            find_space("/work/app", &workspaces, &panes),
            Some("w2".into())
        );
    }

    #[test]
    fn the_label_fallback_skips_a_space_in_another_directory() {
        let workspaces = [ws_in("w1", "app-7", "/other/app-7")];
        assert_eq!(find_space("/work/app-7", &workspaces, &[]), None);
    }

    #[test]
    fn the_label_fallback_accepts_a_space_with_an_empty_directory() {
        let workspaces = [ws_in("w1", "app-7", "")];
        assert_eq!(
            find_space("/work/app-7", &workspaces, &[]),
            Some("w1".into())
        );
    }

    #[test]
    fn a_space_directory_wins_over_the_label_fallback() {
        let workspaces = [ws("w1", "app"), ws_in("w2", "#7 x", "/work/app")];
        assert_eq!(find_space("/work/app", &workspaces, &[]), Some("w2".into()));
    }

    #[test]
    fn falls_back_to_the_label_unless_panes_are_elsewhere() {
        let workspaces = [ws("w1", "app")];
        let elsewhere = [pane("w1", "/other/app")];
        assert_eq!(find_space("/work/app", &workspaces, &elsewhere), None);
        assert_eq!(find_space("/work/app", &workspaces, &[]), Some("w1".into()));
    }

    #[test]
    fn label_defaults_to_the_directory_name_and_falls_back_when_taken() {
        let workspaces = [ws("w1", "app")];
        let home = Path::new("/home/u");
        assert_eq!(space_label("/work/lib", "", Some(home), &workspaces), "lib");
        assert_eq!(
            space_label("/home/u/work/app", "app", Some(home), &workspaces),
            "work/app"
        );
        assert_eq!(
            space_label("/opt/app", "app", Some(home), &workspaces),
            "/opt/app"
        );
        assert_eq!(
            space_label("/work/app", " custom ", Some(home), &workspaces),
            "custom"
        );
    }

    #[test]
    fn request_params_carry_cwd_and_label() {
        assert_eq!(
            create_params("/work/app", "app"),
            json!({ "cwd": "/work/app", "label": "app" })
        );
        assert_eq!(focus_params("w1"), json!({ "workspace_id": "w1" }));
        let result = json!({ "type": "workspace_created", "workspace": { "workspace_id": "w3" } });
        assert_eq!(created_workspace_id(&result), Some("w3".into()));
        assert_eq!(created_workspace_id(&json!({})), None);
    }

    #[test]
    fn resolve_dir_checks_the_directory() {
        let dir = std::env::temp_dir()
            .to_string_lossy()
            .trim_end_matches('/')
            .to_owned();
        assert_eq!(resolve_dir(&format!("{dir}/"), None), Ok(dir.clone()));
        assert!(resolve_dir("", None).is_err());
        assert!(resolve_dir("relative/x", None).is_err());
        assert!(
            resolve_dir("/definitely/not/here", None)
                .unwrap_err()
                .contains("not a directory")
        );
    }

    #[test]
    fn child_dirs_lists_sorted_directories_only_and_hides_dotted_ones() {
        let dir = std::env::temp_dir().join(format!("poimen-explorer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["beta", "Alpha", ".hidden"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        let base = dir.to_string_lossy().into_owned();
        let names = |hidden| -> Vec<String> {
            child_dirs(&base, None, hidden)
                .into_iter()
                .map(|e| e.name)
                .collect()
        };
        assert_eq!(names(false), ["Alpha", "beta"]);
        assert_eq!(names(true), [".hidden", "Alpha", "beta"]);
        assert_eq!(
            child_dirs(&base, None, false)[0].path,
            format!("{base}/Alpha")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn child_dirs_expands_home_and_tolerates_bad_paths() {
        let dir = std::env::temp_dir().join(format!("poimen-explorer-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        let listed = child_dirs("~", Some(&dir), false);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "proj");
        assert!(child_dirs("/nonexistent/poimen", None, false).is_empty());
        assert!(child_dirs("relative", None, false).is_empty());
        assert!(child_dirs("", None, false).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dirs_filters_by_prefix_and_hides_dotted_ones() {
        let root = std::env::temp_dir().join(format!("poimen-dirs-{}", std::process::id()));
        for d in ["alpha", "alps", "beta", ".hidden"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("alfile"), "").unwrap();
        let base = root.to_string_lossy().into_owned();
        let names = |input: &str| -> Vec<String> {
            list_dirs(input, None, 10)
                .iter()
                .map(|p| {
                    Path::new(p)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        };
        assert_eq!(names(&format!("{base}/al")), ["alpha", "alps"]);
        assert_eq!(names(&format!("{base}/")), ["alpha", "alps", "beta"]);
        assert_eq!(names(&format!("{base}/.")), [".hidden"]);
        assert_eq!(list_dirs(&format!("{base}/"), None, 2).len(), 2);
        assert!(list_dirs("relative", None, 10).is_empty());
        // A root lists like any directory, and a trailing separator is not kept by `resolve_dir`.
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            resolve_dir(&format!("{base}{sep}"), None).as_deref(),
            Ok(base.as_str())
        );
        let root_dir = Path::new(&base).ancestors().last().unwrap();
        assert_eq!(
            resolve_dir(&root_dir.to_string_lossy(), None),
            Ok(root_dir.to_string_lossy().into_owned())
        );
        assert!(!list_dirs(&root_dir.to_string_lossy(), None, 5).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
