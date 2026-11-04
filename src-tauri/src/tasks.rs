//! Task modes (`tasks` in `config.json`): how an issue is turned into work. This module holds the
//! types and the template expansion; `settings.rs` resolves and validates the config.

// The expansion is consumed by the task start (a later PBI); until then only tests call it.
#![allow(dead_code)]

use serde::Serialize;

pub const DEFAULT_BRANCH: &str = "{type}/{nr}-{slug}";
pub const DEFAULT_WORKTREE_DIR: &str = "../{repo}-{nr}";
pub const DEFAULT_PROMPT: &str = "Work on GitHub issue #{nr}: {title} ({url})";

/// Placeholders allowed in `branch` and `worktreeDir`.
pub const PATH_PLACEHOLDERS: &[&str] = &["nr", "title", "url", "repo", "slug", "type"];
/// Placeholders allowed in `command` and `cleanup`.
pub const COMMAND_PLACEHOLDERS: &[&str] = &[
    "nr", "title", "url", "branch", "repo", "slug", "type", "prompt",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMode {
    pub name: String,
    pub worktree: bool,
    pub command: String,
    pub branch: String,
    pub cleanup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModes {
    pub modes: Vec<TaskMode>,
    pub worktree_dir: String,
}

impl Default for TaskModes {
    fn default() -> Self {
        Self {
            modes: vec![TaskMode {
                name: "agent".into(),
                worktree: true,
                command: "claude {prompt}".into(),
                branch: DEFAULT_BRANCH.into(),
                cleanup: None,
            }],
            worktree_dir: DEFAULT_WORKTREE_DIR.into(),
        }
    }
}

/// The names of the `{placeholder}`s in `template`, in order.
pub fn placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

/// The first placeholder of `template` that is not in `allowed`.
pub fn unknown_placeholder(template: &str, allowed: &[&str]) -> Option<String> {
    placeholders(template)
        .into_iter()
        .find(|p| !allowed.contains(&p.as_str()))
}

/// Single-quotes `value` for a POSIX shell (embedded `'` becomes `'\''`).
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn strip_level(title: &str) -> &str {
    let t = title.trim_start();
    if t.starts_with('[')
        && let Some(i) = t.find(']')
    {
        return t[i + 1..].trim_start();
    }
    t
}

/// The conventional commit type of an issue title (`feat(tasks): x`, `[Level] fix!: x`), else `issue`.
pub fn title_type(title: &str) -> &'static str {
    const TYPES: &[&str] = &[
        "feat", "fix", "refactor", "docs", "build", "chore", "test", "perf", "ci",
    ];
    let rest = strip_level(title);
    let head: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let after = &rest[head.len()..];
    let after = match after.strip_prefix('(') {
        Some(a) => match a.find(')') {
            Some(i) => &a[i + 1..],
            None => return "issue",
        },
        None => after,
    };
    let after = after.strip_prefix('!').unwrap_or(after);
    if after.starts_with(':')
        && let Some(t) = TYPES.iter().find(|t| **t == head.to_lowercase())
    {
        return t;
    }
    "issue"
}

/// Title without the `[Level] ` prefix and the conventional type/scope, lowercase, runs of
/// non-alphanumerics as one `-`, at most 40 characters, no leading or trailing `-`.
pub fn slug(title: &str) -> String {
    let mut rest = strip_level(title);
    if title_type(title) != "issue"
        && let Some(i) = rest.find(':')
    {
        rest = &rest[i + 1..];
    }
    let mut out = String::new();
    for c in rest.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.truncate(40);
    out.trim_end_matches('-').to_string()
}

/// The values behind the placeholders of one issue.
#[derive(Debug, Clone, Default)]
pub struct TaskVars {
    pub nr: String,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub branch: String,
    pub prompt: String,
}

impl TaskVars {
    /// Variables for an issue; `branch` and `prompt` are filled by `expand_mode`.
    pub fn new(nr: u64, title: &str, url: &str, repo: &str) -> Self {
        Self {
            nr: nr.to_string(),
            title: title.into(),
            url: url.into(),
            repo: repo.into(),
            ..Self::default()
        }
    }

    fn value(&self, name: &str) -> Option<String> {
        Some(match name {
            "nr" => self.nr.clone(),
            "title" => self.title.clone(),
            "url" => self.url.clone(),
            "repo" => self.repo.clone(),
            "branch" => self.branch.clone(),
            "prompt" => self.prompt.clone(),
            "slug" => slug(&self.title),
            "type" => title_type(&self.title).into(),
            _ => return None,
        })
    }
}

/// Expands `template`; values are shell-quoted when `quote` is set. Unknown placeholders stay.
pub fn expand(template: &str, vars: &TaskVars, quote: bool) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        match vars.value(&after[..end]) {
            Some(v) if quote => out.push_str(&shell_quote(&v)),
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A mode expanded for one issue.
#[derive(Debug, Clone, PartialEq)]
pub struct Expanded {
    pub branch: String,
    pub worktree_dir: String,
    pub command: String,
    pub cleanup: Option<String>,
}

/// Expands branch, worktree directory, command and cleanup of `mode` for the issue in `vars`.
pub fn expand_mode(mode: &TaskMode, worktree_dir: &str, mut vars: TaskVars) -> Expanded {
    vars.branch = expand(&mode.branch, &vars, false);
    vars.prompt = expand(DEFAULT_PROMPT, &vars, false);
    Expanded {
        branch: vars.branch.clone(),
        worktree_dir: expand(worktree_dir, &vars, false),
        command: expand(&mode.command, &vars, true),
        cleanup: mode.cleanup.as_ref().map(|c| expand(c, &vars, true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_comes_from_the_conventional_title() {
        assert_eq!(title_type("[PBI] feat(tasks): task modes"), "feat");
        assert_eq!(title_type("fix!: boom"), "fix");
        assert_eq!(title_type("ci: pin"), "ci");
        assert_eq!(title_type("Add a thing"), "issue");
        assert_eq!(title_type("style: nope"), "issue");
        assert_eq!(title_type("feat(open: x"), "issue");
    }

    #[test]
    fn slug_drops_prefix_type_and_scope() {
        assert_eq!(
            slug("[PBI] feat(tasks): Task modes in the settings"),
            "task-modes-in-the-settings"
        );
        assert_eq!(slug("Fix the  thing!!"), "fix-the-thing");
        assert_eq!(
            slug("[Epic] Plain title: with colon"),
            "plain-title-with-colon"
        );
        assert_eq!(slug("???"), "");
    }

    #[test]
    fn slug_is_at_most_40_chars_without_trailing_dash() {
        let s = slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbb");
        assert_eq!(s, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let t = slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbb");
        assert_eq!(t.len(), 40);
        assert_eq!(slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa b"), s);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn command_values_are_quoted_and_paths_are_not() {
        let mode = TaskMode {
            name: "agent".into(),
            worktree: true,
            command: "claude {prompt}".into(),
            branch: "{type}/{nr}-{slug}".into(),
            cleanup: Some("git branch -D {branch}".into()),
        };
        let vars = TaskVars::new(
            7,
            "fix: it's $(rm -rf ~); echo `x` a b",
            "https://x/7",
            "poimen",
        );
        let e = expand_mode(&mode, "../{repo}-{nr}", vars);
        assert_eq!(e.branch, "fix/7-it-s-rm-rf-echo-x-a-b");
        assert_eq!(e.worktree_dir, "../poimen-7");
        assert_eq!(
            e.command,
            "claude 'Work on GitHub issue #7: fix: it'\\''s $(rm -rf ~); echo `x` a b (https://x/7)'"
        );
        assert_eq!(
            e.cleanup.as_deref(),
            Some("git branch -D 'fix/7-it-s-rm-rf-echo-x-a-b'")
        );
    }

    #[test]
    fn each_placeholder_is_quoted_separately_in_a_command() {
        let mut mode = TaskModes::default().modes.remove(0);
        mode.command = "run {nr} {title} {url} {repo} {slug} {type}".into();
        let e = expand_mode(&mode, "{slug}", TaskVars::new(3, "Hello; world", "u", "r"));
        assert_eq!(
            e.command,
            "run '3' 'Hello; world' 'u' 'r' 'hello-world' 'issue'"
        );
        assert_eq!(e.worktree_dir, "hello-world");
    }

    #[test]
    fn unknown_placeholders_are_found_and_kept() {
        assert_eq!(
            unknown_placeholder("a {nr} {x}", COMMAND_PLACEHOLDERS),
            Some("x".into())
        );
        assert_eq!(
            unknown_placeholder("{prompt}", PATH_PLACEHOLDERS),
            Some("prompt".into())
        );
        assert_eq!(unknown_placeholder("no braces {", PATH_PLACEHOLDERS), None);
        assert_eq!(
            expand("{x}-{nr}", &TaskVars::new(1, "t", "u", "r"), false),
            "{x}-1"
        );
    }
}
