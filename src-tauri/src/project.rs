//! The project file `.poimen/config.json`: parsing, entries and teardown live in
//! `poimen_core::project`; what needs a `Backend` (setup tab, palette entries) is here.

pub use poimen_core::project::*;

/// Runs `command` visibly in a new tab of the workspace, labelled `setup`, in `cwd`: the tab is
/// created, named, and the command typed with Enter. The caller decides whether it may run (trust).
pub fn start_setup(
    backend: &dyn crate::backend::Backend,
    workspace_id: &str,
    cwd: &str,
    command: &str,
) -> Result<(), String> {
    start_tab(backend, workspace_id, cwd, SETUP_LABEL, command)
}

/// Opens a tab named `name` in a space, in `cwd`, and types `command` into it.
pub fn start_tab(
    backend: &dyn crate::backend::Backend,
    workspace_id: &str,
    cwd: &str,
    name: &str,
    command: &str,
) -> Result<(), String> {
    let pane = backend
        .create_terminal(workspace_id, Some(cwd))?
        .ok_or("herdr did not report the new tab")?;
    backend.rename_terminal(&pane, Some(name))?;
    backend.send_input(&pane, command)?;
    Ok(())
}

/// Runs a palette entry of the project file in `sources` through `trust::gate`. A `terminal`
/// command opens a tab named after it in `dir` and types the command; a preset opens a tab built
/// with `layout::apply`; an `agent` command is handed back. `persist(hash)` stores a fresh trust.
#[allow(clippy::too_many_arguments)]
pub fn run_entry(
    backend: &dyn crate::backend::Backend,
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
    decision: Option<&crate::trust::Decision>,
    persist: &dyn Fn(&str) -> Result<(), String>,
    workspace_id: &str,
    dir: &str,
    entry: &Entry,
) -> Result<EntryOutcome, String> {
    let Some((path, text)) = &sources.project else {
        return Ok(EntryOutcome::Missing);
    };
    let config = parse(path, text, platform()).config;
    enum Found<'a> {
        Command(&'a ProjectCommand),
        Preset(&'a Preset),
    }
    let found = match entry {
        Entry::Command { name } => config
            .commands
            .iter()
            .find(|c| &c.name == name)
            .map(Found::Command),
        Entry::Preset { name } => config
            .presets
            .iter()
            .find(|p| &p.name == name)
            .map(Found::Preset),
    };
    let Some(found) = found else {
        return Ok(EntryOutcome::Missing);
    };
    match crate::trust::gate(sources, trusted, decision) {
        crate::trust::Gate::Ask(request) => return Ok(EntryOutcome::Ask { request }),
        crate::trust::Gate::Strip => return Ok(EntryOutcome::Declined),
        crate::trust::Gate::Run { store } => {
            if let Some(hash) = store {
                persist(&hash)?;
            }
        }
    }
    match found {
        Found::Command(c) if c.target == Target::Agent => Ok(EntryOutcome::Agent {
            command: c.command.clone(),
        }),
        Found::Command(c) => {
            start_tab(backend, workspace_id, dir, &c.name, &c.command)?;
            Ok(EntryOutcome::Done)
        }
        Found::Preset(p) => {
            let root = backend
                .create_terminal(workspace_id, Some(dir))?
                .ok_or("herdr did not report the new tab")?;
            crate::layout::apply(backend, &p.layout, dir, &root)?;
            Ok(EntryOutcome::Done)
        }
    }
}

#[cfg(test)]
mod palette {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use crate::trust::{Decision, Sources, sha256_hex};
    use std::collections::BTreeMap;

    const ROOT: &str = "/work/app";
    const FILE: &str = r#"{
            "commands": [
                {"name": "test", "command": "pnpm test", "target": "terminal"},
                {"name": "review", "command": "/review", "target": "agent"}],
            "presets": [{"name": "duo", "panes": [
                {"name": "a", "command": "vim"},
                {"name": "b", "split": {"direction": "right"}}]}]
        }"#;

    fn sources(text: Option<&str>) -> Sources {
        Sources {
            root: ROOT.into(),
            project: text.map(|t| (format!("{ROOT}/{PROJECT_FILE}"), t.to_owned())),
            legacy: None,
        }
    }

    fn trusted() -> BTreeMap<String, String> {
        BTreeMap::from([(ROOT.to_owned(), sha256_hex(FILE))])
    }

    fn run(
        fake: &FakeBackend,
        trusted: &BTreeMap<String, String>,
        decision: Option<&Decision>,
        entry: &Entry,
    ) -> Result<EntryOutcome, String> {
        let stored = std::cell::RefCell::new(Vec::new());
        let out = run_entry(
            fake,
            &sources(Some(FILE)),
            trusted,
            decision,
            &|h| {
                stored.borrow_mut().push(h.to_owned());
                Ok(())
            },
            "w1",
            ROOT,
            entry,
        );
        if decision.is_some() {
            assert_eq!(stored.borrow().len(), 1);
        }
        out
    }

    fn command(name: &str) -> Entry {
        Entry::Command { name: name.into() }
    }

    #[test]
    fn entries_list_names_and_the_lock() {
        let e = entries(&sources(Some(FILE)), &BTreeMap::new());
        assert_eq!(
            e.commands
                .iter()
                .map(|c| (c.name.as_str(), c.target))
                .collect::<Vec<_>>(),
            [("test", Target::Terminal), ("review", Target::Agent)]
        );
        assert_eq!(e.presets, ["duo"]);
        assert!(e.locked);
        assert!(!entries(&sources(Some(FILE)), &trusted()).locked);
        assert_eq!(entries(&sources(None), &trusted()), Entries::default());
    }

    #[test]
    fn a_changed_file_locks_again() {
        let stale = BTreeMap::from([(ROOT.to_owned(), sha256_hex("old"))]);
        assert!(entries(&sources(Some(FILE)), &stale).locked);
    }

    #[test]
    fn untrusted_asks_and_runs_nothing() {
        let fake = FakeBackend::default();
        let out = run(&fake, &BTreeMap::new(), None, &command("test")).unwrap();
        let EntryOutcome::Ask { request } = out else {
            panic!("{out:?}")
        };
        assert_eq!(request.hash, sha256_hex(FILE));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_terminal_command_opens_a_named_tab_and_types_it() {
        let fake = FakeBackend::default();
        assert_eq!(
            run(&fake, &trusted(), None, &command("test")),
            Ok(EntryOutcome::Done)
        );
        let calls = fake.calls();
        assert_eq!(calls[0], "create_terminal w1 /work/app");
        assert!(calls[1].ends_with(" test") && calls[1].starts_with("rename "));
        assert!(calls[2].ends_with(" pnpm test") && calls[2].starts_with("input "));
    }

    #[test]
    fn an_agent_command_is_handed_back_not_run() {
        let fake = FakeBackend::default();
        assert_eq!(
            run(&fake, &trusted(), None, &command("review")),
            Ok(EntryOutcome::Agent {
                command: "/review".into()
            })
        );
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_preset_builds_a_new_tab_from_its_panes() {
        let fake = FakeBackend::default();
        let preset = Entry::Preset { name: "duo".into() };
        assert_eq!(
            run(&fake, &trusted(), None, &preset),
            Ok(EntryOutcome::Done)
        );
        let calls = fake.calls();
        assert_eq!(calls[0], "create_terminal w1 /work/app");
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with("split ") && c.contains("right"))
        );
        assert!(calls.iter().any(|c| c.ends_with(" vim")));
        assert_eq!(calls.iter().filter(|c| c.starts_with("rename ")).count(), 2);
    }

    #[test]
    fn trusting_stores_the_hash_and_runs() {
        let fake = FakeBackend::default();
        let d = Decision::Trust {
            hash: sha256_hex(FILE),
        };
        assert_eq!(
            run(&fake, &BTreeMap::new(), Some(&d), &command("test")),
            Ok(EntryOutcome::Done)
        );
        assert!(!fake.calls().is_empty());
    }

    #[test]
    fn without_declines_and_a_stale_hash_asks_again() {
        let fake = FakeBackend::default();
        let stored = |_: &str| -> Result<(), String> { panic!("must not store") };
        let none = BTreeMap::new();
        let go = |d: &Decision| {
            run_entry(
                &fake,
                &sources(Some(FILE)),
                &none,
                Some(d),
                &stored,
                "w1",
                ROOT,
                &command("test"),
            )
        };
        assert_eq!(go(&Decision::Without), Ok(EntryOutcome::Declined));
        let stale = Decision::Trust { hash: "old".into() };
        assert!(matches!(go(&stale), Ok(EntryOutcome::Ask { .. })));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn an_unknown_entry_or_file_is_missing() {
        let fake = FakeBackend::default();
        assert_eq!(
            run(&fake, &trusted(), None, &command("nope")),
            Ok(EntryOutcome::Missing)
        );
        let out = run_entry(
            &fake,
            &sources(None),
            &trusted(),
            None,
            &|_| Ok(()),
            "w1",
            ROOT,
            &command("test"),
        );
        assert_eq!(out, Ok(EntryOutcome::Missing));
    }

    #[test]
    fn a_failing_backend_call_is_an_error() {
        let fake = FakeBackend::default();
        fake.fail_on("create_terminal");
        assert!(run(&fake, &trusted(), None, &command("test")).is_err());
    }
}
