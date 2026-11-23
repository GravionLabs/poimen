//! The environment of the project's commands and `setupPolicy` (#992): `POIMEN_ROOT_PATH`,
//! `POIMEN_WORKTREE_PATH` and `POIMEN_SPACE` (plus a task's `POIMEN_ISSUE` and `POIMEN_BRANCH`,
//! added by the caller that knows the task).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::git;
use crate::project::{parse, platform};

/// What the root pane of a new space does while `setup` runs (`setupPolicy`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupPolicy {
    /// The root pane's command starts at once, in parallel with `setup` (the default).
    #[default]
    Start,
    /// The root pane's command is typed only after `setup` exited 0.
    Wait,
}

/// The `setupPolicy` of the project file; `Start` without one or when it is invalid.
pub fn setup_policy_of(project: Option<(&str, &str)>) -> SetupPolicy {
    project
        .map(|(path, text)| parse(path, text, platform()).config.setup_policy)
        .unwrap_or_default()
}

/// The variables the `setup`, `teardown`, `commands[]` and task commands run with: the main
/// checkout (`POIMEN_ROOT_PATH`), the worktree (`POIMEN_WORKTREE_PATH`) and the space label
/// (`POIMEN_SPACE`); `POIMEN_ISSUE` and `POIMEN_BRANCH` are added by whoever knows the task.
pub fn workspace_env(root: &str, worktree: &str, space: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("POIMEN_ROOT_PATH".to_owned(), root.to_owned()),
        ("POIMEN_WORKTREE_PATH".to_owned(), worktree.to_owned()),
        ("POIMEN_SPACE".to_owned(), space.to_owned()),
    ])
}

/// [`workspace_env`] of the directory `dir` (its worktree top level, the main checkout of its
/// repository; outside a repository `dir` for both) with `space` as the label.
pub fn dir_env(dir: &str, space: &str) -> BTreeMap<String, String> {
    let worktree = git::toplevel(dir).unwrap_or_else(|| dir.to_owned());
    let root = crate::worktree::main_worktree(Path::new(dir))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| worktree.clone());
    workspace_env(&root, &worktree, space)
}

/// Reads `setupPolicy`; `Start` when absent, an error text for any other value.
pub(crate) fn parse_policy(value: &Value) -> Result<SetupPolicy, String> {
    match value.as_str() {
        _ if value.is_null() => Ok(SetupPolicy::Start),
        Some("start") => Ok(SetupPolicy::Start),
        Some("wait") => Ok(SetupPolicy::Wait),
        _ => Err("expected \"start\" or \"wait\"; using \"start\"".into()),
    }
}

/// The environment of a teardown in `worktree`: the workspace variables (the directory name is
/// the space label) and its branch.
pub(crate) fn teardown_env(worktree: &Path) -> BTreeMap<String, String> {
    let dir = worktree.to_string_lossy();
    let label = worktree
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut env = dir_env(&dir, &label);
    if let Some(branch) = git::info(&dir).and_then(|i| i.branch) {
        env.insert("POIMEN_BRANCH".to_owned(), branch);
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_policy_is_start_unless_the_file_says_wait() {
        let policy = |text: &str| setup_policy_of(Some(("/r/.poimen/config.json", text)));
        assert_eq!(policy("{}"), SetupPolicy::Start);
        assert_eq!(policy(r#"{"setupPolicy": "start"}"#), SetupPolicy::Start);
        assert_eq!(policy(r#"{"setupPolicy": "wait"}"#), SetupPolicy::Wait);
        assert_eq!(setup_policy_of(None), SetupPolicy::Start);
        let bad = parse("f", r#"{"setupPolicy": "later"}"#, "linux");
        assert_eq!(bad.config.setup_policy, SetupPolicy::Start);
        assert_eq!(bad.warnings.len(), 1);
        assert!(
            bad.warnings[0].contains("setupPolicy"),
            "{:?}",
            bad.warnings
        );
    }

    #[test]
    fn workspace_env_names_root_worktree_and_space() {
        let env = workspace_env("/work/app", "/work/app-12", "#12 Add");
        assert_eq!(env["POIMEN_ROOT_PATH"], "/work/app");
        assert_eq!(env["POIMEN_WORKTREE_PATH"], "/work/app-12");
        assert_eq!(env["POIMEN_SPACE"], "#12 Add");
        // Outside a repository the directory is both.
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let env = dir_env(&dir, "s");
        assert_eq!(env["POIMEN_ROOT_PATH"], env["POIMEN_WORKTREE_PATH"]);
    }
}
