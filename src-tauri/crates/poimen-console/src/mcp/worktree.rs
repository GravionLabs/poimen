//! The `worktree` argument of `start_agent` (#981).

use std::path::Path;

use poimen_client::Client;
use serde_json::Value;

use super::{Env, Fail, server_error};

/// The `worktree` argument of `start_agent`: a task worktree of an issue, or of a branch.
pub(super) enum WorktreeSpec {
    Issue {
        nr: u64,
        title: String,
        mode: String,
    },
    Branch(String),
}

impl WorktreeSpec {
    pub(super) fn parse(value: Option<&Value>) -> Result<Option<Self>, Fail> {
        let Some(value) = value.filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let bad = |m: &str| Fail::Tool(format!("bad argument `worktree`: {m}"));
        let obj = value
            .as_object()
            .ok_or_else(|| bad("expected {issue} or {branch}"))?;
        match (obj.get("issue"), obj.get("branch")) {
            (Some(n), None) => {
                let nr = n
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| bad("`issue` is an issue number"))?;
                let str_of = |k: &str| obj.get(k).and_then(Value::as_str).map(str::trim);
                Ok(Some(Self::Issue {
                    nr,
                    title: str_of("title").unwrap_or_default().to_owned(),
                    mode: str_of("mode")
                        .filter(|m| !m.is_empty())
                        .unwrap_or("agent")
                        .to_owned(),
                }))
            }
            (None, Some(b)) => {
                let branch = b.as_str().map(str::trim).filter(|b| !b.is_empty());
                let branch = branch.ok_or_else(|| bad("`branch` is a branch name"))?;
                if branch.contains(['{', '}']) {
                    return Err(bad("`branch` has no placeholders"));
                }
                Ok(Some(Self::Branch(branch.to_owned())))
            }
            _ => Err(bad("give exactly one of `issue` and `branch`")),
        }
    }

    pub(super) fn describe(&self) -> String {
        match self {
            Self::Issue { nr, .. } => format!("issue #{nr}"),
            Self::Branch(b) => format!("branch {b}"),
        }
    }
}

/// A worktree and the space made on it.
pub(super) struct WorktreeSpace {
    pub(super) space: poimen_client::protocol::Space,
    pub(super) path: String,
    pub(super) branch: String,
    pub(super) created: bool,
}

/// Creates or reuses the worktree of `spec` for the repository of `repo_dir` (the rules of
/// `task_worktree`; an existing worktree of the branch is returned as is) and a space on it.
pub(super) fn make_worktree_space(
    client: &Client,
    env: &Env,
    repo_dir: &str,
    spec: &WorktreeSpec,
) -> Result<WorktreeSpace, Fail> {
    let (path, branch, created) = match spec {
        WorktreeSpec::Issue { nr, title, mode } => {
            let issue = poimen_client::protocol::TaskIssue {
                nr: *nr,
                title: title.clone(),
                url: String::new(),
            };
            let wt = client
                .create_worktree(repo_dir, issue, mode)
                .map_err(server_error)?;
            (wt.path, wt.branch, wt.created)
        }
        WorktreeSpec::Branch(branch) => {
            let dir = poimen_core::tasks::for_dir(env.home.as_deref(), repo_dir)
                .modes
                .worktree_dir;
            // `{nr}` has no meaning for a bare branch: name the directory after the branch.
            let dir = dir.replace("{nr}", "{title}");
            let issue = poimen_core::worktree::Issue {
                nr: 0,
                title: branch.replace('/', "-"),
                url: String::new(),
            };
            let wt = poimen_core::worktree::ensure(repo_dir, &issue, branch, &dir)
                .map_err(|e| Fail::Tool(format!("git worktree: {e}")))?;
            (wt.path, wt.branch, wt.created)
        }
    };
    let name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| branch.clone());
    let space = client.create_space(&name, &path).map_err(server_error)?;
    Ok(WorktreeSpace {
        space,
        path,
        branch,
        created,
    })
}
