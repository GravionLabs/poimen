//! The pure parts of "start a task": which command, label and tab name an issue and a mode give,
//! and the name of the tab of a mode without a worktree. The steps themselves (worktree, space) are separate
//! commands so the frontend can name the failing one.

use crate::harness::Harness;
use crate::tasks::{TaskModes, TaskVars, expand_mode};
use crate::worktree::Issue;
use serde::Serialize;

/// The longest space label of a task, including `#<nr> `.
const LABEL_MAX: usize = 40;

/// What starting `issue` in a mode comes to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The mode wants a worktree of its own (and so a space for it).
    pub worktree: bool,
    /// The expanded command of the mode as one line for a shell (argv elements quoted).
    pub command: String,
    /// The expanded argv of the mode; empty for a deprecated string command.
    pub argv: Vec<String>,
    /// The label of the space: `#<nr> <title>`, shortened.
    pub label: String,
    /// The name of the tab of a mode without a worktree: `#<nr> <mode>`.
    pub tab: String,
}

/// `#<nr> <title>` cut to `LABEL_MAX` characters with an ellipsis; just `#<nr>` without a title.
pub fn space_label(nr: u64, title: &str) -> String {
    let title = title.trim();
    let full = if title.is_empty() {
        format!("#{nr}")
    } else {
        format!("#{nr} {title}")
    };
    if full.chars().count() <= LABEL_MAX {
        return full;
    }
    let cut: String = full.chars().take(LABEL_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The plan of `issue` for the mode named `mode`; `repo` is the repository directory name.
pub fn plan(
    modes: &TaskModes,
    harnesses: &[Harness],
    repo: &str,
    issue: &Issue,
    mode: &str,
) -> Result<Plan, String> {
    let found = modes
        .modes
        .iter()
        .find(|m| m.name == mode)
        .ok_or_else(|| format!("unknown task mode {mode:?}"))?;
    let vars = TaskVars::new(issue.nr, &issue.title, &issue.url, repo);
    let expanded = expand_mode(found, &modes.worktree_dir, vars, harnesses)?;
    Ok(Plan {
        worktree: found.worktree,
        command: expanded.command.line(),
        argv: expanded.command.argv(),
        label: space_label(issue.nr, &issue.title),
        tab: format!("#{} {}", issue.nr, found.name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::default_harnesses;

    fn issue(title: &str) -> Issue {
        Issue {
            nr: 12,
            title: title.into(),
            url: "https://github.com/o/r/issues/12".into(),
        }
    }

    #[test]
    fn the_default_agent_mode_runs_claude_with_the_quoted_prompt_in_a_worktree() {
        let plan = plan(
            &TaskModes::default(),
            &default_harnesses(),
            "app",
            &issue("feat: Add picker"),
            "agent",
        )
        .unwrap();
        assert!(plan.worktree);
        assert_eq!(
            plan.argv,
            [
                "claude",
                "Work on GitHub issue #12: feat: Add picker (https://github.com/o/r/issues/12)"
            ]
        );
        assert!(plan.command.starts_with("claude '"), "{}", plan.command);
        assert_eq!(plan.label, "#12 feat: Add picker");
        assert_eq!(plan.tab, "#12 agent");
    }

    #[test]
    fn an_unknown_mode_is_an_error() {
        let err = plan(
            &TaskModes::default(),
            &default_harnesses(),
            "app",
            &issue("x"),
            "nope",
        )
        .unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn long_titles_are_shortened_and_a_missing_title_gives_the_number() {
        let label = space_label(12, &"word ".repeat(30));
        assert_eq!(label, "#12 word word word word word word word…");
        assert!(label.starts_with("#12 word") && label.ends_with('…'));
        assert_eq!(space_label(7, ""), "#7");
    }
}
