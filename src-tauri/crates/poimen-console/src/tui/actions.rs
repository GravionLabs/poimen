//! What the palette does against the server and the disk, without any screen: creating a space
//! (with the trust handshake), running a project entry behind `trust::gate`, and starting a task
//! from an issue (`task_start::plan`, `worktree::ensure`, then a space on the worktree whose root
//! pane runs the mode's command). The TUI mirrors the app's `TaskStarter` and `project_run`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use poimen_client::protocol::{
    SpaceCreate, SpaceCreateResult, SpaceCreated, SplitDirection, TabCreate, TabSplit, TrustAnswer,
    TrustRequired,
};
use poimen_client::{Client, ClientError};
use poimen_core::github::{self, GhError, OpenIssue};
use poimen_core::harness::{self, Harness};
use poimen_core::layout::SplitDirection as CoreDirection;
use poimen_core::project::{self, Entries, Entry};
use poimen_core::settings;
use poimen_core::task_start;
use poimen_core::tasks::{self, TaskModes};
use poimen_core::trust::{self, Decision, Gate, Sources};
use poimen_core::worktree::{self, Issue};
use serde_json::Value;

use super::palette::{TrustChoice, TrustPrompt};

/// Reads the open issues of the GitHub repository of a directory (the task picker's source).
/// A function so tests bring a `gh` of their own.
pub type IssueSource = Arc<dyn Fn(&str) -> Result<Vec<OpenIssue>, String> + Send + Sync>;

fn text(e: ClientError) -> String {
    e.to_string()
}

/// A message for a failed `gh` call.
pub fn gh_text(e: &GhError) -> String {
    match e {
        GhError::GhMissing => "the GitHub CLI (gh) is not installed".into(),
        GhError::NotLoggedIn => "gh is not logged in (run `gh auth login`)".into(),
        GhError::Offline => "GitHub cannot be reached".into(),
        GhError::RateLimited { .. } => "GitHub rate limit reached".into(),
        GhError::NotFound => "GitHub repository not found".into(),
        GhError::Other { message } => message.clone(),
    }
}

/// The issue source over the real `gh`, caching under `cache_dir`.
pub fn system_issues(cache_dir: std::path::PathBuf) -> IssueSource {
    Arc::new(move |dir| open_issues(&github::Client::system(cache_dir.clone()), dir))
}

/// The open issues of the `origin` repository of `dir` through `client`.
pub fn open_issues<R: github::Runner>(
    client: &github::Client<R>,
    dir: &str,
) -> Result<Vec<OpenIssue>, String> {
    let root = poimen_core::git::toplevel(dir).unwrap_or_else(|| dir.to_owned());
    let repo = github::repo_of(Path::new(&root))
        .ok_or_else(|| format!("{root} has no GitHub repository as origin"))?;
    github::open_issues(client, &repo).map_err(|e| gh_text(&e))
}

pub fn prompt_of(request: &TrustRequired) -> TrustPrompt {
    TrustPrompt {
        root: request.root.clone(),
        hash: request.hash.clone(),
        commands: request
            .commands
            .iter()
            .map(|c| format!("{}: {}", c.what, c.command))
            .collect(),
    }
}

fn core_prompt(request: &trust::TrustRequest) -> TrustPrompt {
    TrustPrompt {
        root: request.root.clone(),
        hash: request.hash.clone(),
        commands: request
            .commands
            .iter()
            .map(|c| format!("{}: {}", c.what, c.command))
            .collect(),
    }
}

/// What a space creation came to.
#[derive(Debug)]
pub enum Created {
    Space(Box<SpaceCreated>),
    /// The project file has commands nobody trusted yet; nothing was created.
    Trust(TrustRequired),
}

/// `space.create`, once. On `Trust`, answer and send the same params again.
pub fn create_space(client: &Client, params: &SpaceCreate) -> Result<Created, String> {
    match client.create_space_with(params).map_err(text)? {
        SpaceCreateResult::Created(c) => Ok(Created::Space(Box::new(c))),
        SpaceCreateResult::TrustRequired { trust_required } => Ok(Created::Trust(trust_required)),
    }
}

/// `params` with the user's answer to the trust question.
pub fn answered(params: &SpaceCreate, hash: &str, choice: TrustChoice) -> SpaceCreate {
    SpaceCreate {
        trust: Some(match choice {
            TrustChoice::Trust => TrustAnswer::Trust {
                hash: hash.to_owned(),
            },
            TrustChoice::Without => TrustAnswer::Without,
        }),
        ..params.clone()
    }
}

fn config_json(home: Option<&Path>) -> Value {
    home.and_then(|h| std::fs::read_to_string(settings::config_path(h)).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Whether two directory strings name the same place: git prints `/`, the server may store `\`
/// (Windows), and a canonical path carries the `\\?\` prefix.
fn same_dir(a: &str, b: &str) -> bool {
    fn norm(p: &str) -> String {
        let p = p.strip_prefix(r"\\?\").unwrap_or(p).replace('\\', "/");
        p.trim_end_matches('/').to_owned()
    }
    norm(a) == norm(b)
}

/// The repository root of `dir`, else `dir`.
fn root_of(dir: &str) -> String {
    poimen_core::git::toplevel(dir).unwrap_or_else(|| dir.to_owned())
}

/// The names in the project file of the repository of `dir`, and whether it still needs trust.
pub fn project_entries(dir: &str, home: Option<&Path>) -> Entries {
    let sources = Sources::read(&root_of(dir));
    let trusted = home.map(settings::trusted_projects).unwrap_or_default();
    project::entries(&sources, &trusted)
}

/// What running a project entry came to.
#[derive(Debug, PartialEq)]
pub enum EntryResult {
    /// A tab was created; `terminal` is the one to show.
    Done { terminal: Option<String> },
    /// An `agent` command: type it into the agent of the space.
    Agent(String),
    /// Not trusted yet; nothing ran.
    Trust(TrustPrompt),
    /// The user chose to run without project commands: nothing ran.
    Declined,
    /// The file no longer lists the entry.
    Missing,
}

fn direction(d: CoreDirection) -> SplitDirection {
    match d {
        CoreDirection::Right => SplitDirection::Right,
        CoreDirection::Down => SplitDirection::Down,
    }
}

/// Opens a tab named `name` in the space and types `command` into its terminal.
fn tab_with_command(
    client: &Client,
    space_id: &str,
    name: &str,
    command: &str,
) -> Result<String, String> {
    let (_, terminal) = client
        .create_tab(&TabCreate {
            space_id: space_id.to_owned(),
            name: Some(name.to_owned()),
            terminal_id: None,
        })
        .map_err(text)?;
    client
        .input(&terminal.id, &format!("{command}\r"))
        .map_err(text)?;
    Ok(terminal.id)
}

/// Runs a palette entry of the project file of `dir` behind `trust::gate` with the one read of
/// the file. A fresh `Trust` decision is stored in the user config under `home`.
pub fn run_entry(
    client: &Client,
    home: Option<&Path>,
    space_id: &str,
    dir: &str,
    entry: &Entry,
    decision: Option<&Decision>,
) -> Result<EntryResult, String> {
    let sources = Sources::read(&root_of(dir));
    let Some((path, file)) = &sources.project else {
        return Ok(EntryResult::Missing);
    };
    let config = project::parse(path, file, project::platform()).config;
    let command = match entry {
        Entry::Command { name } => config.commands.iter().find(|c| &c.name == name),
        Entry::Preset { .. } => None,
    };
    let preset = match entry {
        Entry::Preset { name } => config.presets.iter().find(|p| &p.name == name),
        Entry::Command { .. } => None,
    };
    if command.is_none() && preset.is_none() {
        return Ok(EntryResult::Missing);
    }
    let trusted = home.map(settings::trusted_projects).unwrap_or_default();
    match trust::gate(&sources, &trusted, decision) {
        Gate::Ask(request) => return Ok(EntryResult::Trust(core_prompt(&request))),
        Gate::Strip => return Ok(EntryResult::Declined),
        Gate::Run { store } => {
            if let Some(hash) = store {
                let home =
                    home.ok_or("the home directory is not known; the trust cannot be stored")?;
                settings::trust_project(home, &sources.root, &hash)?;
            }
        }
    }
    if let Some(c) = command {
        if c.target == project::Target::Agent {
            return Ok(EntryResult::Agent(c.command.clone()));
        }
        let terminal = tab_with_command(client, space_id, &c.name, &c.command)?;
        return Ok(EntryResult::Done {
            terminal: Some(terminal),
        });
    }
    let preset = preset.ok_or("the preset is gone")?;
    let (_, root) = client
        .create_tab(&TabCreate {
            space_id: space_id.to_owned(),
            name: Some(preset.name.clone()),
            terminal_id: None,
        })
        .map_err(text)?;
    let mut ids: BTreeMap<&str, String> = BTreeMap::new();
    for pane in &preset.layout.panes {
        let id = match &pane.split {
            None => {
                client.rename_terminal(&root.id, &pane.name).map_err(text)?;
                root.id.clone()
            }
            Some(s) => {
                let from = ids.get(s.from.as_str()).ok_or_else(|| {
                    format!("pane {:?} splits unknown pane {:?}", pane.name, s.from)
                })?;
                let (_, t) = client
                    .split(&TabSplit {
                        terminal_id: from.clone(),
                        direction: direction(s.direction),
                        ratio: Some(s.ratio),
                        cwd: None,
                        name: Some(pane.name.clone()),
                        command: None,
                    })
                    .map_err(text)?;
                t.id
            }
        };
        if let Some(command) = &pane.command {
            client.input(&id, &format!("{command}\r")).map_err(text)?;
        }
        ids.insert(pane.name.as_str(), id);
    }
    Ok(EntryResult::Done {
        terminal: Some(root.id),
    })
}

/// The task modes of the user config with the project's merged over them when trusted, and the
/// harnesses.
pub fn task_setup(dir: &str, home: Option<&Path>) -> (tasks::Resolved, Vec<Harness>) {
    let root = config_json(home);
    let mut warnings = Vec::new();
    let user = root
        .get("tasks")
        .map(|v| settings::parse_tasks(v, "using the default agent mode", &mut warnings))
        .map(|over| tasks::merge(&TaskModes::default(), &over))
        .unwrap_or_default();
    let harnesses = root
        .get("harnesses")
        .map(|v| harness::resolve(v, &mut warnings))
        .unwrap_or_else(harness::default_harnesses);
    let sources = Sources::read(&root_of(dir));
    let trusted = home.map(settings::trusted_projects).unwrap_or_default();
    (tasks::for_project(&user, &sources, &trusted), harnesses)
}

/// A tab to open after the space exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabPlan {
    pub name: String,
    pub command: String,
}

/// A task ready to start: the worktree exists, nothing is on the server yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    /// The space to create (with the resolved layout and the mode's command in its root pane).
    pub create: SpaceCreate,
    /// A space that already shows the repository, used instead of creating one (modes without a
    /// worktree).
    pub reuse_space: Option<String>,
    /// The tab to open (modes without a worktree).
    pub tab: Option<TabPlan>,
}

/// Plans the task of `issue` in `mode` for the repository of `dir` and creates its worktree
/// (`task_worktree`). `existing` are the spaces `(id, cwd)` of the server.
pub fn prepare_task(
    dir: &str,
    home: Option<&Path>,
    issue: &Issue,
    mode: &str,
    existing: &[(String, String)],
) -> Result<Prepared, String> {
    let (resolved, harnesses) = task_setup(dir, home);
    let root = root_of(dir);
    let repo = Path::new(&root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let plan = task_start::plan(&resolved.modes, &harnesses, &repo, issue, mode)
        .map_err(|e| format!("task mode: {e}"))?;
    if !plan.worktree {
        return Ok(Prepared {
            create: SpaceCreate::with_layout(super::palette::space_name_of(&root), root.clone()),
            reuse_space: existing
                .iter()
                .find(|(_, cwd)| same_dir(cwd, &root))
                .map(|(id, _)| id.clone()),
            tab: Some(TabPlan {
                name: plan.tab,
                command: plan.command,
            }),
        });
    }
    let found = resolved
        .modes
        .modes
        .iter()
        .find(|m| m.name == mode)
        .ok_or_else(|| format!("unknown task mode {mode:?}"))?;
    let wt = worktree::ensure(dir, issue, &found.branch, &resolved.modes.worktree_dir)
        .map_err(|e| format!("git worktree: {e}"))?;
    let mut create = SpaceCreate::with_layout(plan.label, wt.path);
    create.command = Some(plan.command);
    Ok(Prepared {
        create,
        reuse_space: None,
        tab: None,
    })
}

/// What starting a task came to.
#[derive(Debug)]
pub enum Started {
    Done {
        space_id: String,
        /// The terminal to show.
        terminal: Option<String>,
        warnings: Vec<String>,
    },
    Trust(TrustRequired),
}

/// Carries a prepared task out on the server: creates the space (or reuses one) and opens the
/// tab. Send again with the trust answer in `prepared.create` after a `Trust` result.
pub fn start_prepared(client: &Client, prepared: &Prepared) -> Result<Started, String> {
    let (space_id, mut terminal, warnings) = match &prepared.reuse_space {
        Some(id) => (id.clone(), None, Vec::new()),
        None => match create_space(client, &prepared.create)? {
            Created::Trust(t) => return Ok(Started::Trust(t)),
            Created::Space(c) => {
                let first = c
                    .tabs
                    .first()
                    .map(|t| t.active_terminal.clone())
                    .filter(|id| !id.is_empty());
                (c.space.id, first, c.warnings)
            }
        },
    };
    if let Some(tab) = &prepared.tab {
        terminal = Some(
            tab_with_command(client, &space_id, &tab.name, &tab.command)
                .map_err(|e| format!("terminal tab: {e}"))?,
        );
    }
    Ok(Started::Done {
        space_id,
        terminal,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_core::github::{Output, Runner};
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;
    use std::time::Duration;

    /// A temporary directory removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let d =
                std::env::temp_dir().join(format!("poimen-actions-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            let c = d.canonicalize().unwrap();
            // Windows: no `\\?\` prefix, git and the code under test print plain paths.
            let plain = c.display().to_string();
            Self(PathBuf::from(plain.strip_prefix(r"\\?\").unwrap_or(&plain)))
        }
        fn s(&self) -> String {
            self.0.display().to_string()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = poimen_core::gui_path::command("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    /// A clone `<t>/app` of a bare `<t>/origin.git` with one commit on `main` pushed.
    fn repo_with_origin(t: &Temp) -> String {
        git(&t.0, &["init", "--bare", "-b", "main", "origin.git"]);
        git(&t.0, &["clone", "origin.git", "app"]);
        let app = t.0.join("app");
        std::fs::write(app.join("README.md"), "x").unwrap();
        git(&app, &["checkout", "-b", "main"]);
        git(&app, &["add", "."]);
        git(&app, &["commit", "-m", "init"]);
        git(&app, &["push", "origin", "main"]);
        git(&app, &["remote", "set-head", "origin", "main"]);
        app.display().to_string()
    }

    /// `home` with a user config.
    fn home_with(t: &Temp, config: &str) -> PathBuf {
        let home = t.0.join("home");
        let path = settings::config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, config).unwrap();
        home
    }

    const ISSUE_12: &str = r#"[
        {"number": 12, "title": "Add picker", "html_url": "https://github.com/o/r/issues/12",
         "labels": [{"name": "pbi"}], "assignees": []},
        {"number": 13, "title": "A pull request", "html_url": "x", "pull_request": {}}
    ]"#;

    struct FakeGh {
        calls: RefCell<Vec<Vec<String>>>,
        answer: io::Result<String>,
    }

    impl Runner for &FakeGh {
        fn gh(&self, args: &[String]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args.to_vec());
            match &self.answer {
                Ok(body) => Ok(Output {
                    code: Some(0),
                    stdout: format!("HTTP/2.0 200 OK\r\n\r\n{body}"),
                    stderr: String::new(),
                }),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
        fn now(&self) -> Duration {
            Duration::from_secs(1_000_000)
        }
    }

    fn fake_gh(answer: io::Result<String>) -> FakeGh {
        FakeGh {
            calls: RefCell::new(Vec::new()),
            answer,
        }
    }

    #[test]
    fn open_issues_asks_gh_for_the_origin_repository_and_drops_pull_requests() {
        let t = Temp::new("issues");
        let dir = t.s();
        git(&t.0, &["init", "-b", "main", "."]);
        git(
            &t.0,
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        let gh = fake_gh(Ok(ISSUE_12.into()));
        let cache = Temp::new("issues-cache");
        let client = github::Client::new(&gh, cache.0.clone());
        let issues = open_issues(&client, &dir).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(
            (
                issues[0].number,
                issues[0].title.as_str(),
                issues[0].level.as_deref()
            ),
            (12, "Add picker", Some("pbi"))
        );
        let calls = gh.calls.borrow();
        assert!(
            calls[0]
                .iter()
                .any(|a| a.starts_with("repos/o/r/issues?state=open"))
        );
    }

    #[test]
    fn open_issues_fail_with_a_reason_without_a_github_origin_or_gh() {
        let t = Temp::new("issues-none");
        git(&t.0, &["init", "-b", "main", "."]);
        let gh = fake_gh(Ok("[]".into()));
        let cache = Temp::new("issues-none-cache");
        let client = github::Client::new(&gh, cache.0.clone());
        let err = open_issues(&client, &t.s()).unwrap_err();
        assert!(err.contains("no GitHub repository"), "{err}");
        assert!(gh.calls.borrow().is_empty());

        git(
            &t.0,
            &["remote", "add", "origin", "https://github.com/o/r.git"],
        );
        let missing = fake_gh(Err(io::Error::new(io::ErrorKind::NotFound, "gh")));
        let client = github::Client::new(&missing, cache.0.clone());
        let err = open_issues(&client, &t.s()).unwrap_err();
        assert!(err.contains("gh"), "{err}");
    }

    fn issue() -> Issue {
        Issue {
            nr: 12,
            title: "feat: Add picker".into(),
            url: "https://github.com/o/r/issues/12".into(),
        }
    }

    #[test]
    fn a_worktree_task_creates_the_worktree_and_a_space_whose_root_pane_runs_the_command() {
        let t = Temp::new("task-wt");
        let app = repo_with_origin(&t);
        let home = home_with(
            &t,
            r#"{"tasks": {"modes": [{"name": "echo", "worktree": true, "command": ["echo", "task {nr}"]}]}}"#,
        );
        let p = prepare_task(&app, Some(&home), &issue(), "echo", &[]).unwrap();
        let path = t.0.join("app-12");
        assert_eq!(p.create.cwd, path.display().to_string());
        assert_eq!(p.create.name, "#12 feat: Add picker");
        assert!(p.create.layout);
        assert_eq!(p.create.command.as_deref(), Some("echo 'task 12'"));
        assert_eq!((p.reuse_space, p.tab), (None, None));
        assert!(
            path.join("README.md").exists(),
            "the worktree is checked out"
        );
        // A second start reuses the worktree of the branch.
        let again = prepare_task(&app, Some(&home), &issue(), "echo", &[]).unwrap();
        assert_eq!(again.create.cwd, p.create.cwd);
    }

    #[test]
    fn a_mode_without_a_worktree_opens_a_tab_in_the_existing_space_of_the_repository() {
        let t = Temp::new("task-tab");
        let app = repo_with_origin(&t);
        let home = home_with(
            &t,
            r#"{"tasks": {"modes": [{"name": "here", "worktree": false, "command": ["echo", "hi"]}]}}"#,
        );
        let existing = vec![("s1".to_owned(), format!("{app}/"))];
        let p = prepare_task(&app, Some(&home), &issue(), "here", &existing).unwrap();
        assert_eq!(p.reuse_space.as_deref(), Some("s1"));
        assert_eq!(
            p.tab,
            Some(TabPlan {
                name: "#12 here".into(),
                command: "echo hi".into()
            })
        );
        assert!(!t.0.join("app-12").exists(), "no worktree for this mode");
        let none = prepare_task(&app, Some(&home), &issue(), "here", &[]).unwrap();
        assert_eq!(none.reuse_space, None);
        assert_eq!(none.create.cwd, app);
    }

    #[test]
    fn an_unknown_mode_or_a_failing_git_names_the_step_and_creates_nothing() {
        let t = Temp::new("task-err");
        let app = repo_with_origin(&t);
        let err = prepare_task(&app, None, &issue(), "nope", &[]).unwrap_err();
        assert!(
            err.starts_with("task mode:") && err.contains("nope"),
            "{err}"
        );
        // Not a repository: the worktree step fails.
        let plain = Temp::new("task-plain");
        let err = prepare_task(&plain.s(), None, &issue(), "agent", &[]).unwrap_err();
        assert!(err.starts_with("git worktree:"), "{err}");
        assert!(!plain.0.join("..").join("task-plain-12").exists());
    }

    #[test]
    fn modes_of_an_untrusted_project_file_are_not_used() {
        let t = Temp::new("task-untrusted");
        let app = repo_with_origin(&t);
        std::fs::create_dir_all(Path::new(&app).join(".poimen")).unwrap();
        std::fs::write(
            Path::new(&app).join(".poimen/config.json"),
            r#"{"tasks": {"modes": [{"name": "evil", "worktree": false, "command": "rm -rf /"}]},
                "setup": "make"}"#,
        )
        .unwrap();
        let (resolved, _) = task_setup(&app, None);
        let names: Vec<_> = resolved
            .modes
            .modes
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(names, ["agent"]);
        assert!(resolved.note.is_some());
    }

    #[test]
    fn an_answer_is_added_to_the_held_request_and_nothing_else_changes() {
        let base = SpaceCreate::with_layout("a", "/x");
        let trusted = answered(&base, "ab12", TrustChoice::Trust);
        assert_eq!(
            trusted.trust,
            Some(TrustAnswer::Trust {
                hash: "ab12".into()
            })
        );
        assert_eq!((trusted.name.as_str(), trusted.cwd.as_str()), ("a", "/x"));
        assert_eq!(
            answered(&base, "ab12", TrustChoice::Without).trust,
            Some(TrustAnswer::Without)
        );
    }

    #[test]
    fn the_prompt_lists_hash_and_commands() {
        let p = prompt_of(&TrustRequired {
            root: "/r".into(),
            files: vec![],
            hash: "h".into(),
            commands: vec![poimen_client::protocol::TrustCommand {
                what: "setup".into(),
                command: "make".into(),
            }],
            layout: None,
        });
        assert_eq!((p.root.as_str(), p.hash.as_str()), ("/r", "h"));
        assert_eq!(p.commands, ["setup: make"]);
    }

    #[test]
    fn project_entries_lock_until_the_hash_is_trusted() {
        let t = Temp::new("entries");
        let dir = t.s();
        std::fs::create_dir_all(t.0.join(".poimen")).unwrap();
        let file = r#"{"commands": [{"name": "test", "command": "make test", "target": "terminal"}],
                       "presets": [{"name": "duo", "panes": [{"name": "a"}]}]}"#;
        std::fs::write(t.0.join(".poimen/config.json"), file).unwrap();
        let e = project_entries(&dir, None);
        assert_eq!(e.commands.len(), 1);
        assert_eq!(e.presets, ["duo"]);
        assert!(e.locked);
        let home = home_with(&t, "{}");
        settings::trust_project(&home, &dir, &trust::sha256_hex(file)).unwrap();
        assert!(!project_entries(&dir, Some(&home)).locked);
    }
}
