//! Trust in a repository's project file. The commands of `.poimen/config.json` (or the deprecated
//! `.agents/herdr-layout.json`) run only while the SHA-256 of the file content equals the hash the
//! user trusted for that repository root (`trustedProjects` in the user `config.json`).
//!
//! The files are read once into [`Sources`]; the hash, the listed commands and the layout all come
//! from that one read, so what the user approved is exactly what runs. Nothing here runs a command.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::layout::{self, REPO_LAYOUT_FILE};
use crate::project::{self, PROJECT_FILE};

/// Lower-case hex SHA-256 of `text`.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A project file as `(path, text)`.
pub type FileRef<'a> = Option<(&'a str, &'a str)>;

/// The project files of one repository root as read once: `(path, text)` each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sources {
    pub root: String,
    pub project: Option<(String, String)>,
    pub legacy: Option<(String, String)>,
}

impl Sources {
    pub fn read(root: &str) -> Self {
        let read = |rel: &str| {
            let path = std::path::Path::new(root).join(rel);
            std::fs::read_to_string(&path)
                .ok()
                .map(|text| (path.to_string_lossy().into_owned(), text))
        };
        Self {
            root: root.to_owned(),
            project: read(PROJECT_FILE),
            legacy: read(REPO_LAYOUT_FILE),
        }
    }

    /// The hash of the file content: of the project file, else of the deprecated file. When both
    /// exist (the deprecated one can still supply the layout) the hash covers both, so a change to
    /// either revokes the trust. `None` without any file.
    pub fn hash(&self) -> Option<String> {
        match (&self.project, &self.legacy) {
            (None, None) => None,
            (Some((_, p)), None) => Some(sha256_hex(p)),
            (None, Some((_, l))) => Some(sha256_hex(l)),
            (Some((_, p)), Some((_, l))) => Some(sha256_hex(&format!("{p}\0{l}"))),
        }
    }

    /// The files as `(path, text)` pairs for `layout::resolve`.
    pub fn refs(&self) -> (FileRef<'_>, FileRef<'_>) {
        fn r(o: &Option<(String, String)>) -> FileRef<'_> {
            o.as_ref().map(|(p, t)| (p.as_str(), t.as_str()))
        }
        (r(&self.project), r(&self.legacy))
    }

    /// Every command the files would run, in file order.
    pub fn commands(&self) -> Vec<ListedCommand> {
        let mut out = Vec::new();
        let add = |out: &mut Vec<ListedCommand>, what: String, command: &str| {
            out.push(ListedCommand {
                what,
                command: command.to_owned(),
            });
        };
        let panes = |out: &mut Vec<ListedCommand>, prefix: &str, l: &layout::Layout| {
            for p in &l.panes {
                if let Some(c) = &p.command {
                    add(out, format!("{prefix}pane \"{}\"", p.name), c);
                }
            }
        };
        if let Some((path, text)) = &self.project {
            let c = project::parse(path, text, project::platform()).config;
            if let Some(l) = &c.layout {
                panes(&mut out, "layout: ", l);
            }
            if let Some(s) = &c.setup {
                add(&mut out, "setup".into(), s);
            }
            if let Some(s) = &c.teardown {
                add(&mut out, "teardown".into(), s);
            }
            for cmd in &c.commands {
                add(&mut out, format!("command \"{}\"", cmd.name), &cmd.command);
            }
            if let Some(t) = &c.tasks {
                if let Some(dir) = &t.worktree_dir {
                    add(&mut out, "task worktreeDir".into(), dir);
                }
                for m in t.modes.iter().flatten() {
                    add(&mut out, format!("task mode \"{}\"", m.name), &m.describe());
                    if let Some(cleanup) = &m.cleanup {
                        add(
                            &mut out,
                            format!("task mode \"{}\" cleanup", m.name),
                            &cleanup.display(),
                        );
                    }
                }
            }
            for preset in &c.presets {
                panes(
                    &mut out,
                    &format!("preset \"{}\": ", preset.name),
                    &preset.layout,
                );
            }
        }
        if let Some((_, text)) = &self.legacy
            && let Ok(l) = layout::parse_layout(text)
        {
            panes(&mut out, "deprecated layout file: ", &l);
        }
        out
    }
}

/// One command of a project file, for the trust dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListedCommand {
    /// Where it comes from (`setup`, `layout: pane "agent"`, ...).
    pub what: String,
    pub command: String,
}

/// What the dialog shows: the files, their hash and every command in them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustRequest {
    pub root: String,
    /// The files read, for display.
    pub files: Vec<String>,
    pub hash: String,
    pub commands: Vec<ListedCommand>,
}

/// The user's answer to a [`TrustRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Decision {
    /// "Trust and run": valid only for this exact content hash.
    Trust { hash: String },
    /// "Run without project commands".
    Without,
}

/// What may happen with the project commands of one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Commands may run; `store` is the hash to persist as trusted (a fresh "Trust" answer).
    Run { store: Option<String> },
    /// The panes are built, but no project command runs.
    Strip,
    /// Ask first; nothing is created or run.
    Ask(TrustRequest),
}

/// Decides for `sources` against the stored `trusted` map (root to hash) and the answer, if any.
/// No files or no commands in them: nothing to gate, so `Run` without storing.
pub fn gate(
    sources: &Sources,
    trusted: &BTreeMap<String, String>,
    answer: Option<&Decision>,
) -> Gate {
    let commands = sources.commands();
    let Some(hash) = sources.hash().filter(|_| !commands.is_empty()) else {
        return Gate::Run { store: None };
    };
    if trusted.get(&sources.root) == Some(&hash) {
        return Gate::Run { store: None };
    }
    match answer {
        Some(Decision::Trust { hash: approved }) if *approved == hash => {
            Gate::Run { store: Some(hash) }
        }
        Some(Decision::Without) => Gate::Strip,
        _ => Gate::Ask(TrustRequest {
            root: sources.root.clone(),
            files: [&sources.project, &sources.legacy]
                .into_iter()
                .flatten()
                .map(|(p, _)| p.clone())
                .collect(),
            hash,
            commands,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/work/app";
    const FILE: &str = r#"{
        "layout": {"panes": [{"name": "agent", "command": "claude"},
                             {"name": "sh", "split": {"direction": "down"}}]},
        "setup": "pnpm install", "teardown": "echo bye",
        "commands": [{"name": "test", "command": "pnpm test", "target": "terminal"}],
        "presets": [{"name": "duo", "panes": [{"name": "a", "command": "vim"}]}]
    }"#;

    fn sources(project: Option<&str>, legacy: Option<&str>) -> Sources {
        let f = |p: &str, t: &str| Some((format!("{ROOT}/{p}"), t.to_owned()));
        Sources {
            root: ROOT.into(),
            project: project.and_then(|t| f(PROJECT_FILE, t)),
            legacy: legacy.and_then(|t| f(REPO_LAYOUT_FILE, t)),
        }
    }

    fn trusted(hash: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(ROOT.to_owned(), hash.to_owned())])
    }

    #[test]
    fn sha256_matches_the_known_vectors() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn lists_every_command_of_the_project_file() {
        let what: Vec<_> = sources(Some(FILE), None)
            .commands()
            .into_iter()
            .map(|c| (c.what, c.command))
            .collect();
        assert_eq!(
            what,
            [
                ("layout: pane \"agent\"", "claude"),
                ("setup", "pnpm install"),
                ("teardown", "echo bye"),
                ("command \"test\"", "pnpm test"),
                ("preset \"duo\": pane \"a\"", "vim"),
            ]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
        );
    }

    #[test]
    fn lists_the_commands_of_the_deprecated_file() {
        let s = sources(None, Some(r#"{"panes":[{"name":"a","command":"nvim"}]}"#));
        assert_eq!(s.commands().len(), 1);
        assert_eq!(s.commands()[0].command, "nvim");
    }

    #[test]
    fn an_unknown_root_asks_and_lists_the_commands() {
        let s = sources(Some(FILE), None);
        match gate(&s, &BTreeMap::new(), None) {
            Gate::Ask(req) => {
                assert_eq!(req.root, ROOT);
                assert_eq!(req.hash, sha256_hex(FILE));
                assert_eq!(req.commands.len(), 5);
                assert_eq!(req.files, [format!("{ROOT}/{PROJECT_FILE}")]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_changed_file_revokes_the_trust() {
        let before = sources(Some(FILE), None);
        let hash = before.hash().unwrap();
        assert_eq!(
            gate(&before, &trusted(&hash), None),
            Gate::Run { store: None }
        );
        let after = sources(Some(&FILE.replace("pnpm test", "rm -rf /")), None);
        assert!(matches!(gate(&after, &trusted(&hash), None), Gate::Ask(_)));
    }

    #[test]
    fn trust_answer_is_valid_only_for_the_hash_it_was_given_for() {
        let s = sources(Some(FILE), None);
        let ok = Decision::Trust {
            hash: s.hash().unwrap(),
        };
        assert_eq!(
            gate(&s, &BTreeMap::new(), Some(&ok)),
            Gate::Run { store: s.hash() }
        );
        // The file changed between the dialog and the answer: ask again, store nothing.
        let stale = Decision::Trust {
            hash: sha256_hex("old content"),
        };
        assert!(matches!(
            gate(&s, &BTreeMap::new(), Some(&stale)),
            Gate::Ask(_)
        ));
    }

    #[test]
    fn without_strips_and_never_stores() {
        let s = sources(Some(FILE), None);
        assert_eq!(
            gate(&s, &BTreeMap::new(), Some(&Decision::Without)),
            Gate::Strip
        );
    }

    #[test]
    fn a_hash_stored_for_another_root_does_not_trust_this_one() {
        let s = sources(Some(FILE), None);
        let other = BTreeMap::from([("/work/other".to_owned(), s.hash().unwrap())]);
        assert!(matches!(gate(&s, &other, None), Gate::Ask(_)));
    }

    #[test]
    fn no_file_or_no_commands_needs_no_trust() {
        let none = Gate::Run { store: None };
        assert_eq!(gate(&sources(None, None), &BTreeMap::new(), None), none);
        let plain = r#"{"layout":{"panes":[{"name":"a"}]}}"#;
        assert_eq!(
            gate(&sources(Some(plain), None), &BTreeMap::new(), None),
            none
        );
    }

    #[test]
    fn the_hash_covers_both_files_when_both_exist() {
        let legacy = r#"{"panes":[{"name":"a","command":"nvim"}]}"#;
        let both = sources(Some("{}"), Some(legacy));
        let hash = both.hash().unwrap();
        assert_ne!(hash, sha256_hex("{}"));
        let changed = sources(Some("{}"), Some(&legacy.replace("nvim", "evil")));
        assert!(matches!(
            gate(&changed, &trusted(&hash), None),
            Gate::Ask(_)
        ));
    }

    #[test]
    fn read_loads_both_files() {
        let dir = std::env::temp_dir().join(format!("poimen-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".poimen")).unwrap();
        std::fs::write(dir.join(PROJECT_FILE), r#"{"setup":"make"}"#).unwrap();
        let s = Sources::read(&dir.to_string_lossy());
        assert_eq!(s.project.unwrap().1, r#"{"setup":"make"}"#);
        assert!(s.legacy.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
