//! Space templates (`spaceTemplates[]` in the user config): a directory prefix mapped to an agent
//! (a harness id, optional model) and an optional layout for a new space. Matching is pure; the
//! agent line goes through the harness registry. A template matches by directory, by the remote of
//! `origin` (GitHub owner or Azure DevOps project, see `remote.rs`) or by both.

use crate::harness::{self, Harness};
use crate::layout::Layout;
use crate::remote::Remote;
use crate::tasks::Cmd;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub name: String,
    /// `match.dir` as written: absolute, or relative to the home directory with a leading `~`.
    pub dir: Option<String>,
    /// `match.remote`: the GitHub owner or Azure DevOps organization and project of `origin`.
    pub remote: Option<Remote>,
    /// A harness id from `harnesses`.
    pub agent: String,
    pub model: Option<String>,
    pub layout: Option<Layout>,
}

/// Whether `dir` (as written in the config) is absolute or starts with `~`; also accepts Windows
/// forms (`C:\x`, `\\server`) on any platform so a shared config parses everywhere.
pub fn is_valid_dir(dir: &str) -> bool {
    let b = dir.as_bytes();
    dir == "~"
        || dir.starts_with("~/")
        || dir.starts_with("~\\")
        || dir.starts_with(['/', '\\'])
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

/// Separators unified to `/`, no trailing separator, `~` expanded against `home`, optionally
/// lowercased.
fn normalize(dir: &str, home: Option<&std::path::Path>, ci: bool) -> String {
    let mut s = dir.replace('\\', "/");
    if let Some(rest) = s.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with('/'))
        && let Some(h) = home
    {
        s = format!("{}{rest}", h.to_string_lossy().replace('\\', "/"));
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if ci { s.to_lowercase() } else { s }
}

fn contains(prefix: &str, dir: &str) -> bool {
    dir == prefix
        || prefix == "/"
        || dir
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The best template for `dir` and the parsed `origin` of its repository (`None` without a
/// repository or an `origin`; such a directory matches by `dir` only). A template with both fields
/// needs both to match. Precedence: both > `remote` only > `dir` only, then the longest `dir`
/// (path-segment prefix). A template without `dir` and `remote` is the default: it matches any
/// directory, below every other kind. Case-insensitive paths on Windows and macOS.
pub fn resolve<'a>(
    dir: &str,
    remote: Option<&Remote>,
    templates: &'a [Template],
    home: Option<&std::path::Path>,
) -> Option<&'a Template> {
    resolve_with(
        dir,
        remote,
        templates,
        home,
        cfg!(any(windows, target_os = "macos")),
    )
}

fn resolve_with<'a>(
    dir: &str,
    remote: Option<&Remote>,
    templates: &'a [Template],
    home: Option<&std::path::Path>,
    case_insensitive: bool,
) -> Option<&'a Template> {
    let dir = normalize(dir, home, case_insensitive);
    templates
        .iter()
        .filter_map(|t| {
            let dir_len = match &t.dir {
                Some(d) => {
                    let p = normalize(d, home, case_insensitive);
                    contains(&p, &dir).then_some(p.len())?
                }
                None => 0,
            };
            if let Some(want) = &t.remote
                && !remote.is_some_and(|r| want.same(r))
            {
                return None;
            }
            // both (3) > remote only (2) > dir only (1) > default, no match (0)
            let tier = u8::from(t.dir.is_some()) + 2 * u8::from(t.remote.is_some());
            Some(((tier, dir_len), t))
        })
        .max_by_key(|(k, _)| *k)
        .map(|(_, t)| t)
}

/// The line that starts the template's agent. An unknown harness or one whose program is not
/// available is an `Err` naming the template.
pub fn agent_command(
    t: &Template,
    harnesses: &[Harness],
    available: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let h = harnesses.iter().find(|h| h.id == t.agent).ok_or_else(|| {
        format!(
            "space template {:?}: unknown harness {:?}; the space has no agent",
            t.name, t.agent
        )
    })?;
    if !available(&h.command[0]) {
        return Err(format!(
            "space template {:?}: {} was not found on the PATH; the space has no agent",
            t.name, h.command[0]
        ));
    }
    Ok(Cmd::Argv(harness::build_argv(h, t.model.as_deref(), None)).line())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::default_harnesses;
    use std::path::Path;

    fn t(name: &str, dir: &str) -> Template {
        Template {
            name: name.into(),
            dir: Some(dir.into()),
            remote: None,
            agent: "claude".into(),
            model: None,
            layout: None,
        }
    }

    fn r(name: &str, dir: Option<&str>, remote: Remote) -> Template {
        Template {
            dir: dir.map(Into::into),
            remote: Some(remote),
            ..t(name, "/unused")
        }
    }

    fn gh(owner: &str) -> Remote {
        Remote::GitHub {
            owner: owner.into(),
        }
    }

    fn pick_remote<'a>(dir: &str, remote: Option<Remote>, ts: &'a [Template]) -> Option<&'a str> {
        resolve_with(dir, remote.as_ref(), ts, Some(Path::new("/home/jk")), false)
            .map(|t| t.name.as_str())
    }

    fn pick<'a>(dir: &str, ts: &'a [Template], ci: bool) -> Option<&'a str> {
        resolve_with(dir, None, ts, Some(Path::new("/home/jk")), ci).map(|t| t.name.as_str())
    }

    #[test]
    fn the_longest_prefix_wins() {
        let ts = [
            t("all", "~/src"),
            t("a", "~/src/a"),
            t("deep", "/home/jk/src/a/b"),
        ];
        assert_eq!(pick("/home/jk/src/x", &ts, false), Some("all"));
        assert_eq!(pick("/home/jk/src/a", &ts, false), Some("a"));
        assert_eq!(pick("/home/jk/src/a/b/c", &ts, false), Some("deep"));
        assert_eq!(pick("/elsewhere", &ts, false), None);
    }

    #[test]
    fn a_prefix_must_end_at_a_segment_boundary() {
        let ts = [t("a", "~/src/a/")];
        assert_eq!(pick("/home/jk/src/ab", &ts, false), None);
        assert_eq!(pick("/home/jk/src/a", &ts, false), Some("a"));
        assert_eq!(pick("/home/jk/src/a/x", &ts, false), Some("a"));
    }

    #[test]
    fn case_matters_only_where_the_platform_ignores_it() {
        let ts = [t("a", "~/Src")];
        assert_eq!(pick("/home/jk/src/x", &ts, false), None);
        assert_eq!(pick("/home/jk/src/x", &ts, true), Some("a"));
    }

    #[test]
    fn windows_paths_match_across_separators_and_case() {
        let ts = [t("w", "C:\\Src\\a"), t("root", "C:\\")];
        assert_eq!(pick("c:/src/a/x", &ts, true), Some("w"));
        assert_eq!(pick("C:\\Src\\ab", &ts, true), Some("root"));
        assert_eq!(pick("D:\\Src\\a", &ts, true), None);
    }

    #[test]
    fn a_remote_only_template_matches_any_directory_of_that_remote() {
        let ts = [r("work", None, gh("GravionLabs"))];
        assert_eq!(
            pick_remote("/x", Some(gh("gravionlabs")), &ts),
            Some("work")
        );
        assert_eq!(pick_remote("/x", Some(gh("other")), &ts), None);
    }

    fn default_template(name: &str) -> Template {
        Template {
            dir: None,
            ..t(name, "/unused")
        }
    }

    #[test]
    fn the_default_template_applies_only_when_nothing_else_matches() {
        let ts = [
            default_template("default"),
            t("src", "~/src"),
            r("work", None, gh("a")),
        ];
        assert_eq!(pick("/elsewhere", &ts, false), Some("default"));
        assert_eq!(pick("/home/jk/src/x", &ts, false), Some("src"));
        assert_eq!(pick_remote("/elsewhere", Some(gh("a")), &ts), Some("work"));
        assert_eq!(
            pick_remote("/elsewhere", Some(gh("b")), &ts),
            Some("default")
        );
        // A template for a parent directory beats the default, however deep the directory is.
        assert_eq!(pick("/home/jk/src/a/b/c", &ts, false), Some("src"));
    }

    #[test]
    fn without_a_default_template_nothing_matches() {
        assert_eq!(pick("/elsewhere", &[t("src", "~/src")], false), None);
    }

    #[test]
    fn a_directory_without_origin_matches_only_by_dir() {
        let ts = [
            r("remote", None, gh("a")),
            r("both", Some("~/src"), gh("a")),
            t("dir", "~/src"),
        ];
        assert_eq!(pick_remote("/home/jk/src/x", None, &ts), Some("dir"));
        assert_eq!(pick_remote("/elsewhere", None, &ts), None);
    }

    #[test]
    fn with_both_fields_both_must_match() {
        let ts = [r("both", Some("~/src"), gh("a"))];
        assert_eq!(
            pick_remote("/home/jk/src/x", Some(gh("a")), &ts),
            Some("both")
        );
        assert_eq!(pick_remote("/home/jk/src/x", Some(gh("b")), &ts), None);
        assert_eq!(pick_remote("/home/jk/other", Some(gh("a")), &ts), None);
    }

    #[test]
    fn precedence_is_both_then_remote_only_then_dir_only() {
        let ts = [
            t("dir", "~/src/deep/er"),
            r("remote", None, gh("a")),
            r("both", Some("~/src"), gh("a")),
        ];
        // a shorter dir with a matching remote beats a longer dir alone
        assert_eq!(
            pick_remote("/home/jk/src/deep/er/x", Some(gh("a")), &ts),
            Some("both")
        );
        // outside the dir of `both`, the remote-only template applies
        assert_eq!(pick_remote("/tmp/x", Some(gh("a")), &ts), Some("remote"));
        // remote only beats a dir only, however long
        let ts = [t("dir", "~/src/deep"), r("remote", None, gh("a"))];
        assert_eq!(
            pick_remote("/home/jk/src/deep/x", Some(gh("a")), &ts),
            Some("remote")
        );
        assert_eq!(
            pick_remote("/home/jk/src/deep/x", Some(gh("z")), &ts),
            Some("dir")
        );
    }

    #[test]
    fn among_templates_with_both_fields_the_longest_dir_wins() {
        let ts = [
            r("short", Some("~/src"), gh("a")),
            r("long", Some("~/src/app"), gh("a")),
        ];
        assert_eq!(
            pick_remote("/home/jk/src/app/x", Some(gh("a")), &ts),
            Some("long")
        );
    }

    #[test]
    fn dir_validation_accepts_absolute_home_and_drive_paths() {
        for ok in ["/a", "~", "~/a", "C:\\a", "c:/a", "\\\\srv\\x"] {
            assert!(is_valid_dir(ok), "{ok}");
        }
        for bad in ["a/b", "./a", "~user/a", "C:a", ""] {
            assert!(!is_valid_dir(bad), "{bad}");
        }
    }

    #[test]
    fn agent_command_uses_the_harness_and_model() {
        let mut tpl = t("x", "/a");
        tpl.model = Some("opus".into());
        let line = agent_command(&tpl, &default_harnesses(), |_| true).unwrap();
        assert_eq!(line, "claude --model opus");
        tpl.model = None;
        assert_eq!(
            agent_command(&tpl, &default_harnesses(), |_| true).unwrap(),
            "claude"
        );
    }

    #[test]
    fn agent_command_errors_name_the_template() {
        let mut tpl = t("work", "/a");
        tpl.agent = "nope".into();
        let e = agent_command(&tpl, &default_harnesses(), |_| true).unwrap_err();
        assert!(
            e.contains("\"work\"") && e.contains("unknown harness"),
            "{e}"
        );
        tpl.agent = "claude".into();
        let e = agent_command(&tpl, &default_harnesses(), |_| false).unwrap_err();
        assert!(e.contains("\"work\"") && e.contains("PATH"), "{e}");
    }
}
