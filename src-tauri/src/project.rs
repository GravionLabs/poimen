//! The project file `.poimen/config.json`: parsing, entries and teardown live in
//! `poimen_core::project`; what needs a `Backend` (palette entries) is here.

pub use poimen_core::detect::{DetectedEntry, detect_all};
pub use poimen_core::project::*;

/// Opens a tab named `name` in a space, in `cwd`, and types `command` into it.
pub fn start_tab(
    backend: &dyn crate::backend::Backend,
    space_id: &str,
    cwd: &str,
    name: &str,
    command: &str,
) -> Result<(), String> {
    let pane = backend
        .create_terminal(space_id, Some(cwd))?
        .ok_or("the backend did not report the new tab")?;
    backend.rename_terminal(&pane, Some(name))?;
    backend.send_input(&pane, command)?;
    Ok(())
}

/// The repository root of `dir` (the key of the hidden detected entries, #1218), or `dir` itself.
#[tauri::command]
pub async fn repo_root(dir: String) -> String {
    tauri::async_runtime::spawn_blocking(move || poimen_core::git::toplevel(&dir).unwrap_or(dir))
        .await
        .unwrap_or_default()
}

/// Runs the detected entry `name` of `source` (`poimen_core::detect`) in a tab named after it in
/// `dir`. The entry is detected again at `root`, so the frontend never supplies a command; there
/// is no hash trust, a call is an explicit pick. Returns `false` when the file no longer lists it.
pub fn run_detected(
    backend: &dyn crate::backend::Backend,
    root: &str,
    space_id: &str,
    dir: &str,
    source: &str,
    name: &str,
) -> Result<bool, String> {
    let found = poimen_core::detect::detect_all(std::path::Path::new(root))
        .into_iter()
        .find(|e| e.source == source && e.name == name && !e.argv.is_empty());
    let Some(entry) = found else {
        return Ok(false);
    };
    let line = poimen_core::detect::command_line(&entry.argv);
    start_tab(backend, space_id, dir, &entry.name, &line)?;
    Ok(true)
}

/// Runs a palette entry of the project file in `sources` through `trust::gate`. A `terminal`
/// command opens a tab named after it in `dir` and types the command; a preset opens a tab built
/// by the backend (`Backend::apply_layout`, which then gates and stores the trust itself); an
/// `agent` command is handed back. `persist(hash)` stores a fresh trust.
#[allow(clippy::too_many_arguments)]
pub fn run_entry(
    backend: &dyn crate::backend::Backend,
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
    decision: Option<&crate::trust::Decision>,
    persist: &dyn Fn(&str) -> Result<(), String>,
    space_id: &str,
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
    let gate = crate::trust::gate(sources, trusted, decision);
    // poimen-server gates the preset, stores the trust and builds the tab; a declined trust runs
    // nothing.
    if let (Found::Preset(p), false) = (&found, matches!(gate, crate::trust::Gate::Strip)) {
        let request = crate::backend::ApplyRequest {
            space_id,
            cwd: dir,
            preset: Some(&p.name),
            decision,
        };
        let result = backend
            .apply_layout(request)
            .ok_or("this backend cannot build a layout")??;
        return Ok(match result {
            crate::backend::LaidOut::Trust(request) => EntryOutcome::Ask { request },
            crate::backend::LaidOut::Created { .. } => EntryOutcome::Done,
        });
    }
    match gate {
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
            start_tab(backend, space_id, dir, &c.name, &c.command)?;
            Ok(EntryOutcome::Done)
        }
        // A preset is built by the backend above, or declined with the gate.
        Found::Preset(_) => Ok(EntryOutcome::Declined),
    }
}

/// The `commands[]` and `presets[]` of the project file of the repository of `dir`, for the
/// palette (names only), and whether the file still needs the user's trust.
#[tauri::command]
pub async fn project_entries(dir: String) -> Entries {
    tauri::async_runtime::spawn_blocking(move || {
        let root = poimen_core::git::toplevel(&dir).unwrap_or(dir);
        let sources = crate::trust::Sources::read(&root);
        entries(&sources, &crate::get_settings().trusted_projects)
    })
    .await
    .unwrap_or_default()
}

/// The commands detected in the repository of `dir` (`package.json` scripts), for the Scripts tab
/// of the palette: no project file and no trust involved, the entries only list. Local disk only.
#[tauri::command]
pub async fn detected_entries(dir: String) -> Vec<DetectedEntry> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = poimen_core::git::toplevel(&dir).unwrap_or(dir);
        detect_all(std::path::Path::new(&root))
    })
    .await
    .unwrap_or_default()
}

/// Runs the detected entry `name` of `source` in a tab of the space `space_id` (an explicit pick,
/// no hash trust). The argv is built here from the repository, never taken from the frontend;
/// `false` when the entry is gone.
#[tauri::command]
pub async fn detected_run(
    backend: tauri::State<'_, crate::Shared>,
    space_id: String,
    dir: String,
    source: String,
    name: String,
) -> Result<bool, String> {
    crate::on_backend(backend.inner(), move |b| {
        let root = poimen_core::git::toplevel(&dir).unwrap_or_else(|| dir.clone());
        run_detected(b, &root, &space_id, &dir, &source, &name)
    })
    .await
}

/// Runs a palette entry of the project file in the space `space_id` (a user action). Goes
/// through `crate::trust::gate` with the one read of the file; an untrusted file answers `ask`.
#[tauri::command]
pub async fn project_run(
    backend: tauri::State<'_, crate::Shared>,
    space_id: String,
    dir: String,
    entry: Entry,
    decision: Option<crate::trust::Decision>,
) -> Result<EntryOutcome, String> {
    crate::on_backend(backend.inner(), move |b| {
        let root = poimen_core::git::toplevel(&dir).unwrap_or_else(|| dir.clone());
        let sources = crate::trust::Sources::read(&root);
        let home = crate::home_dir();
        let trusted = crate::get_settings().trusted_projects;
        let persist = |hash: &str| -> Result<(), String> {
            let home = home.as_deref().ok_or("the home directory is not known")?;
            crate::settings::trust_project(home, &sources.root, hash)
        };
        run_entry(
            b,
            &sources,
            &trusted,
            decision.as_ref(),
            &persist,
            &space_id,
            &dir,
            &entry,
        )
    })
    .await
}

#[cfg(test)]
mod detected {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use crate::test_dir::TestDir;

    fn repo(pkg: &str) -> (TestDir, String) {
        let dir = TestDir::new("poimen-run-detected-");
        std::fs::write(dir.join("package.json"), pkg).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        let path = dir.to_string_lossy().into_owned();
        (dir, path)
    }

    #[test]
    fn a_pick_opens_a_named_tab_and_types_the_argv() {
        let (_dir, root) = repo(r#"{"scripts": {"build": "tsc", "a b": "x"}}"#);
        let fake = FakeBackend::default();
        assert_eq!(
            run_detected(&fake, &root, "w1", &root, "npm", "build"),
            Ok(true)
        );
        let calls = fake.calls();
        assert!(calls[0].starts_with("create_terminal w1 "), "{calls:?}");
        assert!(
            calls
                .iter()
                .any(|c| c.contains("build") && c.starts_with("rename"))
        );
        assert!(
            calls
                .iter()
                .any(|c| c.ends_with("pnpm run build") && c.starts_with("input"))
        );
        assert_eq!(
            run_detected(&fake, &root, "w1", &root, "npm", "a b"),
            Ok(true)
        );
        assert!(fake.calls().iter().any(|c| c.ends_with("pnpm run 'a b'")));
    }

    #[test]
    fn an_unknown_name_runs_nothing() {
        let (_dir, root) = repo(r#"{"scripts": {"build": "tsc"}}"#);
        let fake = FakeBackend::default();
        assert_eq!(
            run_detected(&fake, &root, "w1", &root, "npm", "rm -rf"),
            Ok(false)
        );
        assert_eq!(
            run_detected(&fake, &root, "w1", &root, "cargo", "build"),
            Ok(false)
        );
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_skill_is_not_a_command() {
        let (_dir, root) = repo(r#"{"scripts": {}}"#);
        let skill = std::path::Path::new(&root).join(".agents/skills/deliver");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: deliver\n---\n").unwrap();
        assert_eq!(detect_all(std::path::Path::new(&root))[0].source, "skills");
        let fake = FakeBackend::default();
        assert_eq!(
            run_detected(&fake, &root, "w1", &root, "skills", "deliver"),
            Ok(false)
        );
        assert!(fake.calls().is_empty());
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
    fn a_preset_on_a_backend_that_cannot_build_layouts_is_an_error() {
        let fake = FakeBackend::default();
        let preset = Entry::Preset { name: "duo".into() };
        assert_eq!(
            run(&fake, &trusted(), None, &preset),
            Err("this backend cannot build a layout".into())
        );
        assert!(fake.calls().is_empty());
    }

    // Presets on a backend that builds layouts itself (poimen-server, #645).

    fn layout_fake(answer: crate::backend::LaidOut) -> FakeBackend {
        let fake = FakeBackend::default();
        *fake.laid_out.lock().unwrap() = Some(answer);
        fake
    }

    fn applied() -> crate::backend::LaidOut {
        crate::backend::LaidOut::Created {
            space_id: "w1".into(),
            warnings: vec![],
            setup_started: false,
        }
    }

    /// `run_entry` of the preset `duo` that fails the test when the app stores a trust.
    fn run_preset(
        fake: &FakeBackend,
        trusted: &BTreeMap<String, String>,
        decision: Option<&Decision>,
    ) -> Result<EntryOutcome, String> {
        run_entry(
            fake,
            &sources(Some(FILE)),
            trusted,
            decision,
            &|_| panic!("the app must not store the trust"),
            "w1",
            ROOT,
            &Entry::Preset { name: "duo".into() },
        )
    }

    #[test]
    fn a_layout_backend_builds_the_preset_itself() {
        let fake = layout_fake(applied());
        assert_eq!(run_preset(&fake, &trusted(), None), Ok(EntryOutcome::Done));
        // No terminal, split, rename or input from the app: the server builds the layout.
        assert_eq!(fake.calls(), ["apply_layout w1 /work/app duo -"]);
    }

    #[test]
    fn a_layout_backend_asks_through_the_server_and_gets_the_answer() {
        let question = crate::trust::TrustRequest {
            root: ROOT.into(),
            files: vec![],
            hash: sha256_hex(FILE),
            commands: vec![],
        };
        let fake = layout_fake(crate::backend::LaidOut::Trust(question.clone()));
        assert_eq!(
            run_preset(&fake, &BTreeMap::new(), None),
            Ok(EntryOutcome::Ask { request: question })
        );
        let fake = layout_fake(applied());
        let d = Decision::Trust {
            hash: sha256_hex(FILE),
        };
        assert_eq!(
            run_preset(&fake, &BTreeMap::new(), Some(&d)),
            Ok(EntryOutcome::Done)
        );
        assert_eq!(
            fake.calls(),
            [format!(
                "apply_layout w1 /work/app duo trust:{}",
                sha256_hex(FILE)
            )]
        );
    }

    #[test]
    fn a_declined_preset_runs_nothing_on_a_layout_backend() {
        let fake = layout_fake(applied());
        assert_eq!(
            run_preset(&fake, &BTreeMap::new(), Some(&Decision::Without)),
            Ok(EntryOutcome::Declined)
        );
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_failing_apply_layout_is_an_error() {
        let fake = layout_fake(applied());
        fake.fail_on("apply_layout");
        assert_eq!(run_preset(&fake, &trusted(), None), Err("boom".into()));
    }

    #[test]
    fn a_layout_backend_still_runs_terminal_commands_through_the_app() {
        let fake = layout_fake(applied());
        assert_eq!(
            run(&fake, &trusted(), None, &command("test")),
            Ok(EntryOutcome::Done)
        );
        assert_eq!(fake.calls()[0], "create_terminal w1 /work/app");
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
