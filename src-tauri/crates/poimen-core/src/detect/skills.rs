//! The skills of the repository: `.agents/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md` at
//! the root. An entry is not a command: it has no argv, the app sends `/<name>` to an agent.

use std::path::Path;

use super::{DetectedEntry, Detector};

pub struct Skills;

/// The directories read, in order; a skill of the same name in a later one is not listed again.
const DIRS: [&str; 2] = [".agents/skills", ".claude/skills"];

/// The value of a front matter key: a plain or quoted scalar, or a block scalar (`>`, `|`) or a
/// plain scalar continued on indented lines.
fn field(lines: &[&str], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let at = lines.iter().position(|l| l.starts_with(&prefix))?;
    let head = lines[at][prefix.len()..].trim();
    let rest: Vec<&str> = lines[at + 1..]
        .iter()
        .take_while(|l| l.starts_with([' ', '\t']) || l.trim().is_empty())
        .map(|l| l.trim())
        .collect();
    let folded = |first: &str| {
        std::iter::once(first)
            .chain(rest.iter().copied())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let value = if head.is_empty() || head.starts_with(['>', '|']) {
        folded("")
    } else if let Some(q) = head.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let inner = head[1..].strip_suffix(q).unwrap_or(&head[1..]);
        inner.to_owned()
    } else {
        folded(head)
    };
    Some(value.trim().to_owned())
}

/// `(name, description)` of a `SKILL.md`; `None` without front matter or a usable name. The name
/// is typed into an agent, so it is limited to characters a slash command can carry.
fn parse(text: &str) -> Option<(String, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let front: Vec<&str> = lines
        .by_ref()
        .take_while(|l| l.trim_end() != "---")
        .collect();
    // `take_while` also ends at the end of the text: a front matter must be closed.
    if !text.lines().skip(1).any(|l| l.trim_end() == "---") {
        return None;
    }
    let name = field(&front, "name")?;
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c));
    ok.then(|| (name, field(&front, "description").unwrap_or_default()))
}

impl Detector for Skills {
    fn detect(&self, root: &Path) -> Vec<DetectedEntry> {
        let mut found: Vec<DetectedEntry> = Vec::new();
        for dir in DIRS {
            let Ok(read) = std::fs::read_dir(root.join(dir)) else {
                continue;
            };
            let mut paths: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                let Ok(text) = std::fs::read_to_string(path.join("SKILL.md")) else {
                    continue;
                };
                let Some((name, description)) = parse(&text) else {
                    continue;
                };
                if found.iter().any(|e| e.name == name) {
                    continue;
                }
                found.push(DetectedEntry {
                    source: "skills".into(),
                    name,
                    description,
                    argv: Vec::new(),
                });
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;

    fn repo(files: &[(&str, &str)]) -> TestDir {
        let dir = TestDir::new("poimen-detect-skills-");
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn skill(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\nBody\n")
    }

    fn listed(root: &Path) -> Vec<(String, String)> {
        Skills
            .detect(root)
            .into_iter()
            .map(|e| (e.name, e.description))
            .collect()
    }

    #[test]
    fn reads_both_directories_with_name_and_description() {
        let root = repo(&[
            (
                ".agents/skills/deliver/SKILL.md",
                &skill("deliver", "Ship an issue"),
            ),
            (
                ".claude/skills/review/SKILL.md",
                &skill("review", "Review a diff"),
            ),
        ]);
        let found = Skills.detect(&root);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].source, "skills");
        assert!(found[0].argv.is_empty());
        assert_eq!(
            listed(&root),
            [
                ("deliver".to_owned(), "Ship an issue".to_owned()),
                ("review".to_owned(), "Review a diff".to_owned())
            ]
        );
    }

    #[test]
    fn a_skill_in_both_directories_is_listed_once() {
        let root = repo(&[
            (".agents/skills/x/SKILL.md", &skill("x", "from agents")),
            (".claude/skills/x/SKILL.md", &skill("x", "from claude")),
        ]);
        assert_eq!(listed(&root), [("x".to_owned(), "from agents".to_owned())]);
    }

    #[test]
    fn a_file_without_valid_front_matter_is_skipped() {
        let root = repo(&[
            (".agents/skills/none/SKILL.md", "# Just a title\n"),
            (
                ".agents/skills/open/SKILL.md",
                "---\nname: open\ndescription: x\n",
            ),
            (
                ".agents/skills/noname/SKILL.md",
                "---\ndescription: x\n---\n",
            ),
            (".agents/skills/spaced/SKILL.md", "---\nname: a b\n---\n"),
            (".agents/skills/slash/SKILL.md", "---\nname: ../x\n---\n"),
            (
                ".agents/skills/ctrl/SKILL.md",
                "---\nname: \"a\\u0003\"\n---\n",
            ),
            (".agents/skills/empty/SKILL.md", ""),
            (".agents/skills/ok/SKILL.md", &skill("ok", "fine")),
            (".agents/skills/nofile/README.md", "x"),
            (".agents/skills/loose.md", "x"),
        ]);
        assert_eq!(listed(&root), [("ok".to_owned(), "fine".to_owned())]);
    }

    #[test]
    fn quoted_folded_and_continued_descriptions() {
        let root = repo(&[
            (
                ".agents/skills/a/SKILL.md",
                "---\nname: 'a'\ndescription: \"Quoted: text\"\n---\n",
            ),
            (
                ".agents/skills/b/SKILL.md",
                "---\nname: b\ndescription: >\n  first line\n  second line\nother: 1\n---\n",
            ),
            (
                ".agents/skills/c/SKILL.md",
                "---\r\nname: c\r\ndescription: one\r\n  two\r\n---\r\n",
            ),
        ]);
        assert_eq!(
            listed(&root),
            [
                ("a".to_owned(), "Quoted: text".to_owned()),
                ("b".to_owned(), "first line second line".to_owned()),
                ("c".to_owned(), "one two".to_owned()),
            ]
        );
    }

    #[test]
    fn a_missing_directory_yields_nothing() {
        assert!(Skills.detect(&repo(&[])).is_empty());
    }
}
