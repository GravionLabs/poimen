mod backend;
#[cfg(test)]
mod backend_fake;
mod copilot;
mod git;
mod github;
mod github_branches;
mod github_issues;
mod github_milestones;
mod github_pulls;
mod gui_path;
mod harness;
mod herdr;
mod layout;
#[cfg(test)]
mod live;
mod poimen_backend;
mod poimen_server;
mod project;
mod schemes;
mod settings;
mod spaces;
mod stream;
mod task_start;
mod tasks;
mod terminal_colors;
mod transcripts;
mod trust;
mod updater;
mod watch;
mod worktree;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use backend::{Backend, Mode, SplitDirection, StreamRequest};

/// The backend the commands talk to (herdr or poimen-server, see the `backend` setting).
type Shared = std::sync::Arc<dyn Backend>;

/// Serializes `ensure_backend`.
#[derive(Default)]
struct StartLock(std::sync::Arc<Mutex<()>>);

/// The running subagent transcript watches by key: one per frontend follower (the output panel,
/// each visible flow tab), each with at most one watch.
#[derive(Default)]
struct SubagentWatch(Mutex<Watches<transcripts::WatchHandle>>);

/// Values by key, each with the generation of the latest command for that key.
///
/// Every command takes a generation when it starts; a result that finishes after a newer command
/// for the same key (another watch or an unwatch) began is stale and dropped instead of stored.
/// Generations are unique across keys, so a key removed and begun again never matches an older
/// in-flight command.
struct Watches<T> {
    next: u64,
    slots: HashMap<String, (u64, Option<T>)>,
}

impl<T> Default for Watches<T> {
    fn default() -> Self {
        Self {
            next: 0,
            slots: HashMap::new(),
        }
    }
}

impl<T> Watches<T> {
    /// A new command for `key` starts: older in-flight commands for it become stale.
    fn begin(&mut self, key: &str) -> u64 {
        self.next += 1;
        self.slots.entry(key.to_owned()).or_insert((0, None)).0 = self.next;
        self.next
    }

    /// Stores `value` for `key` unless a newer command began since `generation` or the key was
    /// removed; the previous value is dropped (stopped) when stored. `false` for a stale result,
    /// which is dropped.
    fn store(&mut self, key: &str, generation: u64, value: Option<T>) -> bool {
        match self.slots.get_mut(key) {
            Some(slot) if slot.0 == generation => {
                slot.1 = value;
                true
            }
            _ => false,
        }
    }

    /// Drops the value of `key` and makes every in-flight command for it stale.
    fn remove(&mut self, key: &str) {
        self.slots.remove(key);
    }
}

/// Runs `f` against the backend off the main thread; shared by the commands below.
async fn on_backend<T: Send + 'static>(
    backend: &Shared,
    f: impl FnOnce(&dyn Backend) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let backend = backend.clone();
    tauri::async_runtime::spawn_blocking(move || f(backend.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Workspaces, tabs and panes in one call; the frontend rebuilds its tree from it.
#[tauri::command]
async fn snapshot(backend: State<'_, Shared>) -> Result<backend::Snapshot, String> {
    on_backend(backend.inner(), |b| b.snapshot()).await
}

/// Splits a pane; returns the new pane id. The change also arrives as events.
#[tauri::command]
async fn pane_split(
    backend: State<'_, Shared>,
    target: String,
    direction: SplitDirection,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    on_backend(backend.inner(), move |b| {
        b.split(&target, direction, cwd.as_deref(), None)
    })
    .await
}

/// Closes a pane. The UI confirms first; the backend ends the pane's process.
#[tauri::command]
async fn pane_close(backend: State<'_, Shared>, pane_id: String) -> Result<(), String> {
    on_backend(backend.inner(), move |b| b.close_terminal(&pane_id)).await
}

/// Closes a space with all its panes; only on an explicit user action (the UI confirms first when
/// it has panes). Afterwards `focus`, the neighbouring space, is focused on a best-effort basis; a
/// failing close leaves the space untouched and returns the backend's error.
#[tauri::command]
async fn space_close(
    backend: State<'_, Shared>,
    workspace_id: String,
    focus: Option<String>,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        b.close_space(&workspace_id)?;
        if let Some(id) = focus.filter(|id| !id.is_empty()) {
            let _ = b.focus_space(&id);
        }
        Ok(())
    })
    .await
}

/// Starts a supported agent in an existing pane at a shell prompt; can take a while.
#[tauri::command]
async fn agent_start(
    backend: State<'_, Shared>,
    pane_id: String,
    kind: String,
    name: Option<String>,
    args: Vec<String>,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        b.agent_start(&pane_id, &kind, name.as_deref(), &args)
    })
    .await
}

/// Renames an agent; an empty name clears it.
#[tauri::command]
async fn agent_rename(
    backend: State<'_, Shared>,
    target: String,
    name: Option<String>,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        b.agent_rename(&target, name.as_deref())
    })
    .await
}

/// Sends a prompt to an agent without focusing its terminal; no controller needed. Stalled,
/// timed-out and blocked agents are results, not errors.
#[tauri::command]
async fn agent_prompt(
    backend: State<'_, Shared>,
    target: String,
    text: String,
) -> Result<herdr::PromptOutcome, String> {
    on_backend(backend.inner(), move |b| Ok(b.agent_prompt(&target, &text))).await
}

/// Renames a pane; an empty label clears it.
#[tauri::command]
async fn pane_rename(
    backend: State<'_, Shared>,
    pane_id: String,
    label: Option<String>,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        b.rename_terminal(&pane_id, label.as_deref())
    })
    .await
}

type GithubClient = std::sync::Arc<github::Client<github::SystemRunner>>;

/// Open pull requests of the GitHub repositories among `dirs` (space and explorer directories),
/// one GraphQL query per repository. A failing repository carries its own error. Read-only.
#[tauri::command]
async fn github_pulls(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<github_pulls::Pulls, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || github_pulls::load(&client, &dirs))
        .await
        .map_err(|e| e.to_string())
}

/// The pull request (open or merged) of the branch of each directory, one GraphQL query per
/// repository; directories without one are left out. Read-only; `gh` errors give an empty answer.
#[tauri::command]
async fn branch_pulls(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<HashMap<String, github_branches::BranchPull>, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || github_branches::load(&client, dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Issues of the GitHub repositories among `dirs` with parent links and linked pull requests (open
/// ones, or the closed ones with `closed`), up to 300 per repository. Read-only.
#[tauri::command]
async fn github_issues(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
    closed: Option<bool>,
) -> Result<Vec<github_issues::RepoIssues>, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    let closed = closed.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || github_issues::load(&client, &dirs, closed))
        .await
        .map_err(|e| e.to_string())
}

/// Open milestones of the GitHub repositories among `dirs` (REST with ETag), ordered by version
/// with the current one marked. Read-only.
#[tauri::command]
async fn github_milestones(
    client: State<'_, GithubClient>,
    dirs: Vec<String>,
) -> Result<Vec<github_milestones::RepoMilestones>, String> {
    let client = client.inner().clone();
    let home = home_dir();
    let dirs: Vec<String> = dirs
        .iter()
        .map(|d| github_pulls::expand_home(d, home.as_deref()))
        .collect();
    tauri::async_runtime::spawn_blocking(move || github_milestones::load(&client, &dirs))
        .await
        .map_err(|e| e.to_string())
}

/// `owner/name` of the GitHub `origin` of `dir`, `None` for a directory without one (read-only).
#[tauri::command]
async fn github_repo(dir: String) -> Option<github::Repo> {
    let dir = github_pulls::expand_home(&dir, home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || github::repo_of(std::path::Path::new(&dir)))
        .await
        .ok()
        .flatten()
}

/// Open issues of the GitHub repository of `dir` for the task picker (REST with ETag cache, pull
/// requests filtered out, up to 300). Read-only.
#[tauri::command]
async fn task_issues(
    client: State<'_, GithubClient>,
    dir: String,
) -> Result<Vec<github::OpenIssue>, github::GhError> {
    let client = client.inner().clone();
    let dir = github_pulls::expand_home(&dir, home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let repo = github::repo_of(std::path::Path::new(&dir)).ok_or(github::GhError::Other {
            message: "no GitHub repository".into(),
        })?;
        github::open_issues(&client, &repo)
    })
    .await
    .unwrap_or_else(|e| {
        Err(github::GhError::Other {
            message: e.to_string(),
        })
    })
}

/// Opens a new terminal in a workspace as a new tab; returns its pane id.
#[tauri::command]
async fn terminal_create(
    backend: State<'_, Shared>,
    workspace_id: String,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    on_backend(backend.inner(), move |b| {
        b.create_terminal(&workspace_id, cwd.as_deref())
    })
    .await
}

/// Creates a space for a directory and focuses it; when a space for the directory exists (a pane
/// inside it, else one labelled with its name) that one is focused.
#[tauri::command]
async fn create_space(
    backend: State<'_, Shared>,
    cwd: String,
    label: Option<String>,
    decision: Option<trust::Decision>,
    command: Option<String>,
) -> Result<spaces::SpaceOutcome, String> {
    on_backend(backend.inner(), move |b| {
        create_space_with(
            b,
            home_dir().as_deref(),
            &cwd,
            label.as_deref(),
            decision,
            command.as_deref(),
        )
    })
    .await
}

/// The body of `create_space`, against any backend. `root_command` replaces the command of the
/// layout's root pane in a new space (a task's command); an existing space runs nothing.
fn create_space_with(
    backend: &dyn Backend,
    home: Option<&std::path::Path>,
    cwd: &str,
    label: Option<&str>,
    decision: Option<trust::Decision>,
    root_command: Option<&str>,
) -> Result<spaces::SpaceOutcome, String> {
    let dir = spaces::resolve_dir(cwd, home)?;
    let snapshot = backend.snapshot()?;
    let list = |value: &Value| value.as_array().cloned().unwrap_or_default();
    let workspaces = list(&snapshot.workspaces);
    let panes = list(&snapshot.panes);
    if let Some(workspace_id) = spaces::find_space(&dir, &workspaces, &panes) {
        backend.focus_space(&workspace_id)?;
        return Ok(spaces::SpaceOutcome {
            workspace_id,
            created: false,
            warnings: Vec::new(),
            setup_started: false,
            trust: None,
        });
    }
    // The project files are read once; the trust check, the listed commands and the layout all
    // use this read.
    let root = git::toplevel(&dir).unwrap_or_else(|| dir.clone());
    let sources = trust::Sources::read(&root);
    let user = home.map(settings::load);
    let trusted = user
        .as_ref()
        .map(|s| s.trusted_projects.clone())
        .unwrap_or_default();
    let allow = match trust::gate(&sources, &trusted, decision.as_ref()) {
        trust::Gate::Ask(request) => {
            return Ok(spaces::SpaceOutcome {
                workspace_id: String::new(),
                created: false,
                warnings: Vec::new(),
                setup_started: false,
                trust: Some(request),
            });
        }
        trust::Gate::Strip => false,
        trust::Gate::Run { store } => {
            if let Some(hash) = store {
                let home = home.ok_or("the home directory is not known")?;
                settings::trust_project(home, &sources.root, &hash)?;
            }
            true
        }
    };
    let label = spaces::space_label(&dir, label.unwrap_or_default(), home, &workspaces);
    let created = backend.create_space(&dir, &label)?;
    let workspace_id = created.space_id.clone();
    backend.focus_space(&workspace_id)?;
    let user_layout = user.and_then(|s| s.spaces_layout);
    let mut warnings = build_layout(
        backend,
        &dir,
        created.root_terminal.as_deref(),
        &sources,
        user_layout.as_ref(),
        allow,
        root_command,
    );
    let (setup_started, warning) = run_setup(backend, &workspace_id, &dir, &sources, allow);
    warnings.extend(warning);
    Ok(spaces::SpaceOutcome {
        workspace_id,
        created: true,
        warnings,
        setup_started,
        trust: None,
    })
}

/// Builds the pane layout in a just-created space from the already read project `sources`:
/// `.poimen/config.json`, else the deprecated `.agents/herdr-layout.json`, else `spaces.layout` of
/// the config (`user_layout`), else the default. With `allow_project_commands` false the commands
/// of a layout that comes from a project file are not typed; the panes are still created and named.
/// Problems come back as warnings; the space then keeps the panes created so far (at least its
/// single root pane).
fn build_layout(
    backend: &dyn Backend,
    dir: &str,
    root_pane: Option<&str>,
    sources: &trust::Sources,
    user_layout: Option<&layout::Layout>,
    allow_project_commands: bool,
    root_command: Option<&str>,
) -> Vec<String> {
    let (project, legacy) = sources.refs();
    let (layout, mut warnings, from_project) =
        match layout::resolve_origin(project, legacy, user_layout) {
            Ok(r) => r,
            Err(warning) => return vec![warning],
        };
    let layout = if from_project && !allow_project_commands {
        layout.without_commands()
    } else {
        layout
    };
    let layout = match root_command {
        Some(command) => layout.with_root_command(command),
        None => layout,
    };
    let Some(root_pane) = root_pane else {
        return vec!["herdr did not report the root pane; no layout applied".into()];
    };
    if let Err(e) = layout::apply(backend, &layout, dir, root_pane) {
        warnings.push(format!("layout not fully applied: {e}"));
    }
    warnings
}

/// Starts the `setup` command of the project file in a `setup` tab of a just-created space. Only
/// when project commands are allowed (trusted); a failure is a warning, never a reason to close
/// the space. Returns whether it started and the warning, if any.
fn run_setup(
    backend: &dyn Backend,
    workspace_id: &str,
    dir: &str,
    sources: &trust::Sources,
    allow_project_commands: bool,
) -> (bool, Option<String>) {
    if !allow_project_commands {
        return (false, None);
    }
    let (project, _) = sources.refs();
    let Some(command) = project::setup_of(project) else {
        return (false, None);
    };
    match project::start_setup(backend, workspace_id, dir, &command) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("setup not started: {e}"))),
    }
}

/// Revokes the trust of the project file of `root`; the settings watcher emits the change.
#[tauri::command]
fn revoke_project(root: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::revoke_project(&home, &root)
}

/// The project file `.poimen/config.json` of the repository containing `dir`, with warnings.
#[tauri::command]
async fn project_config(dir: String) -> Result<project::Loaded, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = git::toplevel(&dir).unwrap_or(dir);
        project::load(&root)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Directories matching a typed path, for the completion of path fields.
#[tauri::command]
async fn list_dirs(input: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        spaces::list_dirs(&input, home_dir().as_deref(), 50)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The subdirectories of a directory for the explorer tree; unreadable ones have none.
#[tauri::command]
async fn list_child_dirs(path: String, hidden: bool) -> Result<Vec<spaces::DirEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        spaces::child_dirs(&path, home_dir().as_deref(), hidden)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The backend this run talks to (`herdr` or `poimen`), fixed at start from the `backend` setting.
struct BackendKind(&'static str);

#[tauri::command]
fn backend_kind(kind: State<'_, BackendKind>) -> &'static str {
    kind.0
}

/// Gives `poimen-server` the colors of the active scheme (`foreground`, `background`, `cursor`,
/// 16 `palette` entries as `#rrggbb`), so it answers color queries while no app is attached. Kept
/// for when the server starts; a no-op for the herdr backend, and an unreachable server is no error.
#[tauri::command]
async fn set_terminal_colors(
    colors: poimen_client::protocol::SetColors,
    store: State<'_, terminal_colors::TerminalColors>,
    kind: State<'_, BackendKind>,
) -> Result<(), String> {
    let poimen = kind.0 == "poimen";
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let client = poimen_client::Client::default();
        // The server may not be up yet: the colors are pushed after `ensure_backend`.
        let _ = store.set(
            colors,
            poimen.then_some(&client as &dyn terminal_colors::ColorSink),
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Version and protocol of the running backend; `None` when unreachable or too old.
#[tauri::command]
async fn herdr_info(backend: State<'_, Shared>) -> Result<Option<backend::BackendInfo>, String> {
    on_backend(backend.inner(), |b| Ok(b.info())).await
}

/// Starts the backend server when nothing answers on its socket (and `herdr.autoStart` allows it),
/// then waits until it answers. Runs at start and on Retry; one at a time so a retry during a
/// start never launches a second server.
#[tauri::command]
async fn ensure_backend(
    backend: State<'_, Shared>,
    lock: State<'_, StartLock>,
    colors: State<'_, terminal_colors::TerminalColors>,
    kind: State<'_, BackendKind>,
) -> Result<backend::BackendStatus, String> {
    let lock = lock.0.clone();
    let colors = colors.inner().clone();
    let poimen = kind.0 == "poimen";
    on_backend(backend.inner(), move |b| {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let auto_start = home_dir()
            .map(|h| settings::load(&h).herdr_auto_start)
            .unwrap_or(true);
        let status: backend::BackendStatus = b.ensure_running(auto_start).into();
        if poimen {
            // The server answers color queries with the scheme the frontend sent before it ran.
            let _ = colors.push(&poimen_client::Client::default());
        }
        Ok(status)
    })
    .await
}

/// Starts `poimen-server` when none answers (never stops it); the status is `already-running`,
/// `started` or `disabled`. The UI starts the selected backend through `ensure_backend`.
#[tauri::command]
async fn ensure_poimen_server(auto_start: bool) -> Result<&'static str, String> {
    tauri::async_runtime::spawn_blocking(move || {
        poimen_server::ensure_running(&poimen_server::SystemRunner::new(), auto_start)
            .map(|outcome| match outcome {
                backend::Outcome::AlreadyRunning => "already-running",
                backend::Outcome::Started => "started",
                backend::Outcome::Disabled => "disabled",
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The running poimen build's version (Tauri app version).
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The user's home directory: `HOME` on Unix, the profile (`%USERPROFILE%`) on Windows, where a
/// GUI app has no `HOME`.
fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|h| !h.as_os_str().is_empty())
}

/// The resolved user settings (`~/.config/poimen/config.json`); changes arrive as `settings` events.
#[tauri::command]
fn get_settings() -> settings::Settings {
    home_dir().map(|h| settings::load(&h)).unwrap_or_default()
}

/// The agent harnesses with availability (program found on the `PATH` of a child).
#[tauri::command]
async fn get_harnesses() -> Vec<harness::HarnessInfo> {
    let harnesses = get_settings().harnesses;
    tauri::async_runtime::spawn_blocking(move || harness::with_availability(harnesses))
        .await
        .unwrap_or_default()
}

/// Creates or reuses the git worktree and branch of `issue` for the task mode `mode` of the
/// repository of `repo_dir` (see `worktree::ensure`).
#[tauri::command]
async fn task_worktree(
    repo_dir: String,
    issue: worktree::Issue,
    mode: String,
) -> Result<worktree::Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = get_settings();
        let root = git::toplevel(&repo_dir).unwrap_or_else(|| repo_dir.clone());
        let sources = trust::Sources::read(&root);
        let resolved = tasks::for_project(&settings.tasks, &sources, &settings.trusted_projects);
        let found = resolved
            .modes
            .modes
            .iter()
            .find(|m| m.name == mode)
            .ok_or_else(|| format!("unknown task mode {mode:?}"))?;
        worktree::ensure(
            &repo_dir,
            &issue,
            &found.branch,
            &resolved.modes.worktree_dir,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Creates or reuses a worktree with the head of pull request `nr` checked out (see
/// `worktree::pr_worktree`).
#[tauri::command]
async fn pr_worktree(
    repo_dir: String,
    nr: u64,
    head_ref: Option<String>,
) -> Result<worktree::Worktree, String> {
    tauri::async_runtime::spawn_blocking(move || {
        worktree::pr_worktree(&repo_dir, nr, head_ref.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The worktrees of the repository of `dir` with their branches.
#[tauri::command]
async fn worktree_branches(dir: String) -> Result<Vec<worktree::Checkout>, String> {
    tauri::async_runtime::spawn_blocking(move || worktree::list_worktrees(&dir))
        .await
        .map_err(|e| e.to_string())?
}

/// The expanded command, space label and tab name of `issue` in the task mode `mode` of the
/// repository of `repo_dir` (see `task_start::plan`).
#[tauri::command]
async fn task_plan(
    repo_dir: String,
    issue: worktree::Issue,
    mode: String,
) -> Result<task_start::Plan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = get_settings();
        let root = git::toplevel(&repo_dir).unwrap_or_else(|| repo_dir.clone());
        let sources = trust::Sources::read(&root);
        let resolved = tasks::for_project(&settings.tasks, &sources, &settings.trusted_projects);
        let repo = std::path::Path::new(&root)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        task_start::plan(&resolved.modes, &repo, &issue, &mode)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes the worktrees, spaces and local branches of the repository of `dir` whose pull request
/// is merged (see `worktree::cleanup`). The first task mode with a `cleanup` command replaces the
/// built-in removal. `focused_space` is never touched. Destructive, so only on a user action.
#[tauri::command]
async fn task_cleanup(
    backend: State<'_, Shared>,
    dir: String,
    focused_space: Option<String>,
) -> Result<worktree::CleanupReport, String> {
    let backend = backend.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = worktree::main_worktree(std::path::Path::new(&dir))?;
        let repo = github::repo_of(&root)
            .ok_or_else(|| "the repository has no GitHub origin".to_owned())?;
        let merged = worktree::merged_prs(&github::SystemRunner, &repo)?;
        let settings = get_settings();
        let root_str = root.to_string_lossy().into_owned();
        let sources = trust::Sources::read(&root_str);
        let resolved = tasks::for_project(&settings.tasks, &sources, &settings.trusted_projects);
        let mode = resolved
            .modes
            .modes
            .iter()
            .find_map(|m| m.cleanup.clone())
            .map_or(
                worktree::CleanupMode::Builtin,
                worktree::CleanupMode::Command,
            );
        worktree::cleanup(
            &root,
            &merged,
            &mode,
            backend.as_ref(),
            focused_space.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens the tab `name` in a space and runs `command` in it (a task mode without a worktree).
/// The command comes from the webview (already expanded by `task_plan`) because the frontend is
/// the only caller; like the `command` of `create_space` it is not a surface for other clients.
#[tauri::command]
async fn task_tab(
    backend: State<'_, Shared>,
    workspace_id: String,
    dir: String,
    name: String,
    command: String,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        task_start::start_tab(b, &workspace_id, &dir, &name, &command)
    })
    .await
}

/// The task modes for the repository of `dir` (the user's `tasks` in config.json with the trusted
/// project file's `tasks` merged over them); without `dir` only the user's.
#[tauri::command]
async fn get_task_modes(dir: Option<String>) -> tasks::Resolved {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = get_settings();
        let Some(dir) = dir else {
            return tasks::Resolved {
                modes: settings.tasks,
                note: None,
            };
        };
        let root = git::toplevel(&dir).unwrap_or(dir);
        let sources = trust::Sources::read(&root);
        tasks::for_project(&settings.tasks, &sources, &settings.trusted_projects)
    })
    .await
    .unwrap_or_else(|_| tasks::Resolved {
        modes: tasks::TaskModes::default(),
        note: None,
    })
}

/// The user color schemes (`~/.config/poimen/themes/*.toml`); changes arrive as `color-schemes` events.
#[tauri::command]
fn get_color_schemes() -> schemes::Schemes {
    home_dir().map(|h| schemes::load(&h)).unwrap_or_default()
}

/// Writes only the `theme` setting (the toolbar button); the settings watcher emits the change.
#[tauri::command]
fn set_theme(theme: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_theme(&home, &theme)
}

/// Writes only the `colorSchema` setting (the settings view): a name or a `{light, dark}` pair.
#[tauri::command]
fn set_color_schema(schema: serde_json::Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_color_schema(&home, &schema)
}

/// Writes only `sidebar.layout` (`tree` or `split`); the settings watcher emits the change.
#[tauri::command]
fn set_sidebar_layout(layout: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_sidebar_layout(&home, &layout)
}

/// Writes only `explorer.roots` (paths under the home directory as `~/…`); the watcher emits it.
#[tauri::command]
fn set_explorer_roots(roots: Vec<String>) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_explorer_roots(&home, &roots)
}

/// Writes only `explorer.showHidden`; the settings watcher emits the change.
#[tauri::command]
fn set_explorer_show_hidden(show: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_explorer_show_hidden(&home, show)
}

/// Writes only `notifications.<kind>` (`blocked` or `done`); the settings watcher emits the change.
#[tauri::command]
fn set_notification(kind: String, on: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_notification(&home, &kind, on)
}

/// Writes only `herdr.autoStart`; the settings watcher emits the change.
#[tauri::command]
fn set_herdr_auto_start(on: bool) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_herdr_auto_start(&home, on)
}

/// Writes only `spaces.layout` after validating it; the settings watcher emits the change.
#[tauri::command]
fn set_spaces_layout(layout: serde_json::Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_spaces_layout(&home, &layout)
}

/// Removes `spaces.layout`, so new spaces use the built-in default again.
#[tauri::command]
fn remove_spaces_layout() -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::remove_spaces_layout(&home)
}

/// The built-in pane layout of a new space, shown by the settings view when none is configured.
#[tauri::command]
fn default_spaces_layout() -> layout::Layout {
    layout::default_layout()
}

/// Branch and worktree flag per directory (read-only `git rev-parse`).
#[tauri::command]
async fn repo_info(dirs: Vec<String>) -> Result<HashMap<String, git::RepoInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || git::infos(dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Starts following the subagent transcripts of the Claude Code or GitHub Copilot CLI agent in
/// `pane_id` (replacing the previous watch); updates arrive as `subagents` events. `false` when the
/// pane has no such session or its transcript is not found; nothing is watched then. Read-only.
#[tauri::command]
async fn subagents_watch(
    app: AppHandle,
    backend: State<'_, Shared>,
    watch: State<'_, SubagentWatch>,
    key: String,
    pane_id: String,
) -> Result<bool, String> {
    let generation = watch.0.lock().map_err(|e| e.to_string())?.begin(&key);
    let session = {
        let pane_id = pane_id.clone();
        on_backend(backend.inner(), move |b| b.agent_session(&pane_id)).await?
    };
    let found = session.and_then(|s| {
        let home = home_dir()?;
        let dir = match s.agent {
            herdr::TranscriptAgent::Claude => transcripts::locate_project_dir(
                &transcripts::projects_root(&home),
                &s.cwd,
                &s.session,
            )?,
            herdr::TranscriptAgent::Copilot => {
                copilot::locate_session_dir(&copilot::sessions_root(&home), &s.session)?
            }
        };
        Some((s.agent, dir, s.session))
    });
    let Some((agent, dir, session)) = found else {
        let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
        watches.store(&key, generation, None);
        return Ok(false);
    };
    let (id, follower) = (session.clone(), key.clone());
    let emit = move |update| {
        let event = json!({ "key": follower, "session": id, "update": update });
        let _ = app.emit("subagents", event);
    };
    let handle = match agent {
        herdr::TranscriptAgent::Claude => transcripts::watch(&dir, &session, emit)?,
        herdr::TranscriptAgent::Copilot => copilot::watch(&dir, emit)?,
    };
    let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
    // A stale handle is dropped here, which stops its watcher; so is one whose key was unwatched.
    Ok(watches.store(&key, generation, Some(handle)))
}

/// Stops the subagent watch of `key` and forgets the key.
#[tauri::command]
fn subagents_unwatch(watch: State<'_, SubagentWatch>, key: String) {
    if let Ok(mut watches) = watch.0.lock() {
        watches.remove(&key);
    }
}

/// What goes over the channel of a stream for one record. On an emulated stream the PTY bytes
/// (snapshot and output) are sent raw, which the webview receives as an `ArrayBuffer`; the other
/// records are JSON. A rendered stream (herdr frames) sends JSON only.
fn channel_messages(record: &backend::StreamRecord, emulated: bool) -> Vec<InvokeResponseBody> {
    use backend::StreamRecord;
    let json = |record: &StreamRecord| {
        InvokeResponseBody::Json(serde_json::to_string(record).unwrap_or_default())
    };
    let decode = |bytes: &str| {
        poimen_client::protocol::decode_bytes(bytes).unwrap_or_else(|err| {
            eprintln!("poimen: dropped terminal output that is not valid base64: {err}");
            Vec::new()
        })
    };
    match record {
        StreamRecord::Output { bytes } if emulated => {
            vec![InvokeResponseBody::Raw(decode(bytes))]
        }
        // The reset announces the size and clears the screen; the snapshot follows as raw bytes.
        StreamRecord::Reset { cols, rows, bytes } if emulated => {
            let mut messages = vec![json(&StreamRecord::Reset {
                cols: *cols,
                rows: *rows,
                bytes: String::new(),
            })];
            let snapshot = decode(bytes);
            if !snapshot.is_empty() {
                messages.push(InvokeResponseBody::Raw(snapshot));
            }
            messages
        }
        other => vec![json(other)],
    }
}

/// Opens a terminal stream; its records go through `on_record`, a channel of this stream (no
/// global event, so no record is lost or mixed up before the view has its id). The result says
/// whether the stream is emulated (raw PTY bytes) or rendered (finished frames).
#[tauri::command]
fn open_terminal(
    backend: State<'_, Shared>,
    mode: Mode,
    target: String,
    cols: u16,
    rows: u16,
    takeover: bool,
    on_record: Channel<InvokeResponseBody>,
) -> Result<backend::OpenedStream, String> {
    let request = StreamRequest {
        mode,
        target: &target,
        cols,
        rows,
        takeover,
    };
    let emulated = backend.emulated();
    backend.open_stream(
        request,
        Box::new(move |_, record| {
            for message in channel_messages(&record, emulated) {
                let _ = on_record.send(message);
            }
        }),
    )
}

#[tauri::command]
fn terminal_input(backend: State<'_, Shared>, id: u32, text: String) -> Result<(), String> {
    backend.stream_input(id, &text)
}

#[tauri::command]
fn terminal_resize(
    backend: State<'_, Shared>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    backend.stream_resize(id, cols, rows)
}

#[tauri::command]
fn terminal_scroll(
    backend: State<'_, Shared>,
    id: u32,
    up: bool,
    lines: u16,
) -> Result<(), String> {
    backend.stream_scroll(id, up, lines)
}

#[tauri::command]
fn terminal_mouse(
    backend: State<'_, Shared>,
    id: u32,
    action: String,
    button: String,
    column: u16,
    row: u16,
    modifiers: u8,
) -> Result<(), String> {
    backend.stream_mouse(id, &action, &button, column, row, modifiers)
}

#[tauri::command]
fn close_terminal(backend: State<'_, Shared>, id: u32) {
    backend.close_stream(id);
}

/// The accelerator the native menu carries. Only Cmd bindings on macOS: elsewhere (and for Ctrl
/// or Alt bindings) the webview matches the keys itself, so a terminal that has focus keeps them.
fn native_accelerator(accelerator: &str) -> Option<String> {
    let cmd = accelerator
        .split('+')
        .any(|t| matches!(t, "Cmd" | "CmdOrCtrl"));
    if !cfg!(target_os = "macos") || !cmd {
        return None;
    }
    Some(
        accelerator
            .split('+')
            .map(|t| match t {
                "Left" => "ArrowLeft",
                "Right" => "ArrowRight",
                "Up" => "ArrowUp",
                "Down" => "ArrowDown",
                other => other,
            })
            .collect::<Vec<_>>()
            .join("+"),
    )
}

fn action_item(
    app: &AppHandle,
    shortcuts: &BTreeMap<String, String>,
    id: &str,
    text: &str,
    enabled: bool,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    let accelerator = shortcuts.get(id).and_then(|a| native_accelerator(a));
    // An accelerator muda cannot parse leaves the item without one instead of failing the menu.
    MenuItem::with_id(app, id, text, enabled, accelerator.as_deref())
        .or_else(|_| MenuItem::with_id(app, id, text, enabled, None::<&str>))
}

fn build_menu(
    app: &AppHandle,
    shortcuts: &BTreeMap<String, String>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str| action_item(app, shortcuts, id, text, true);
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::new(app)?;
    if cfg!(target_os = "macos") {
        menu.append(&Submenu::with_items(
            app,
            "poimen",
            true,
            &[
                &PredefinedMenuItem::about(app, None, None)?,
                &sep()?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &sep()?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?)?;
    }
    let new_terminal = item("newTerminal", "New Terminal")?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &new_terminal,
            &sep()?,
            &item("closeTab", "Close Tab")?,
            &item("closeAll", "Close All Tabs")?,
        ],
    )?;
    if !cfg!(target_os = "macos") {
        file.append(&sep()?)?;
        file.append(&PredefinedMenuItem::quit(app, None)?)?;
    }
    menu.append(&file)?;
    menu.append(&Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "View",
        true,
        &[
            &item("toggleSidebar", "Toggle Sidebar")?,
            &item("toggleOutput", "Toggle Output Panel")?,
            &sep()?,
            &item("splitRight", "Split Right")?,
            &item("splitDown", "Split Down")?,
            &sep()?,
            &item("zoomIn", "Zoom In")?,
            &item("zoomOut", "Zoom Out")?,
            &item("zoomReset", "Actual Size")?,
            &sep()?,
            &item("refresh", "Refresh")?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "Go",
        true,
        &[
            &item("commandPalette", "Command Palette...")?,
            &sep()?,
            &item("nextWaiting", "Next Waiting Agent")?,
            &sep()?,
            &item("nextTab", "Next Tab")?,
            &item("previousTab", "Previous Tab")?,
            &sep()?,
            &item("showShortcuts", "Keyboard Shortcuts")?,
        ],
    )?)?;
    menu.append(&Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?)?;
    Ok(menu)
}

/// The backend named by the `backend` setting (anything but `poimen` is herdr) and its name.
fn make_backend(kind: &str) -> (Shared, &'static str) {
    if kind == "poimen" {
        let backend = poimen_backend::PoimenBackend::new(poimen_client::Client::default());
        return (std::sync::Arc::new(backend), "poimen");
    }
    let client = herdr::Client::from_env();
    let backend = backend::HerdrBackend::new(
        backend::SystemRunner::new(client.clone()),
        client,
        stream::resolve_bin_from_env(),
    );
    (std::sync::Arc::new(backend), "herdr")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let kind = home_dir()
        .map(|h| settings::load(&h).backend)
        .unwrap_or_default();
    let (backend, kind) = make_backend(&kind);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::PendingUpdate::default())
        .manage(backend.clone())
        .manage(BackendKind(kind))
        .manage::<GithubClient>(std::sync::Arc::new(github::Client::system(
            home_dir()
                .map(|h| {
                    h.join(".config")
                        .join("poimen")
                        .join("cache")
                        .join("github")
                })
                .unwrap_or_else(|| std::env::temp_dir().join("poimen-github")),
        )))
        .manage(SubagentWatch::default())
        .manage(StartLock::default())
        .manage(terminal_colors::TerminalColors::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            let shortcuts = home_dir()
                .map(|h| settings::load(&h).shortcuts)
                .unwrap_or_else(settings::default_shortcuts);
            app.set_menu(build_menu(&handle, &shortcuts)?)?;
            app.on_menu_event(|app, event| {
                let _ = app.emit("menu", event.id().0.clone());
            });
            if let Some(home) = home_dir() {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    schemes::watch(&home, std::time::Duration::from_secs(1), |s| {
                        let _ = handle.emit("color-schemes", s);
                    });
                });
            }
            if let Some(home) = home_dir() {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    settings::watch(&home, std::time::Duration::from_secs(1), |s| {
                        let _ = handle.emit("settings", s);
                        // Menu items carry the accelerators; rebuild them on the main thread.
                        let (app, shortcuts) = (handle.clone(), s.shortcuts.clone());
                        let _ = handle.run_on_main_thread(move || {
                            if let Ok(menu) = build_menu(&app, &shortcuts) {
                                let _ = app.set_menu(menu);
                            }
                        });
                    });
                });
            }
            std::thread::spawn(move || {
                backend.watch(&|event| {
                    let _ = handle.emit("herdr", event);
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            herdr_info,
            ensure_backend,
            backend_kind,
            set_terminal_colors,
            ensure_poimen_server,
            set_herdr_auto_start,
            set_notification,
            pane_split,
            pane_close,
            space_close,
            agent_start,
            agent_rename,
            agent_prompt,
            pane_rename,
            terminal_create,
            github_pulls,
            branch_pulls,
            github_issues,
            github_milestones,
            task_issues,
            github_repo,
            create_space,
            list_dirs,
            list_child_dirs,
            app_version,
            updater::update_check,
            updater::update_install,
            get_settings,
            get_harnesses,
            get_task_modes,
            task_worktree,
            pr_worktree,
            worktree_branches,
            task_plan,
            task_tab,
            task_cleanup,
            get_color_schemes,
            set_theme,
            set_color_schema,
            set_sidebar_layout,
            set_explorer_roots,
            set_explorer_show_hidden,
            set_spaces_layout,
            revoke_project,
            remove_spaces_layout,
            default_spaces_layout,
            project_config,
            repo_info,
            subagents_watch,
            subagents_unwatch,
            open_terminal,
            terminal_input,
            terminal_resize,
            terminal_scroll,
            terminal_mouse,
            close_terminal
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Release controllers and stop the CLI children; herdr panes keep running.
                app.state::<Shared>().close_streams();
            }
        });
}

#[cfg(test)]
mod subagent_watches_tests {
    use super::Watches;

    #[test]
    fn a_newer_command_makes_an_older_result_stale() {
        let mut watches = Watches::<&str>::default();
        let older = watches.begin("panel");
        let newer = watches.begin("panel");
        assert!(watches.store("panel", newer, Some("new")));
        assert!(!watches.store("panel", older, Some("old")));
        assert_eq!(watches.slots["panel"].1, Some("new"));
    }

    #[test]
    fn a_remove_before_a_watch_finishes_is_not_undone() {
        let mut watches = Watches::<&str>::default();
        let generation = watches.begin("panel");
        watches.remove("panel");
        assert!(!watches.store("panel", generation, Some("late")));
        assert!(!watches.slots.contains_key("panel"));
    }

    #[test]
    fn a_key_begun_again_after_a_remove_does_not_take_an_older_result() {
        let mut watches = Watches::<&str>::default();
        let older = watches.begin("flow");
        watches.remove("flow");
        let newer = watches.begin("flow");
        assert!(!watches.store("flow", older, Some("old")));
        assert!(watches.store("flow", newer, Some("new")));
    }

    #[test]
    fn keys_do_not_make_each_other_stale() {
        let mut watches = Watches::<&str>::default();
        let panel = watches.begin("panel");
        let flow = watches.begin("flow");
        assert!(watches.store("panel", panel, Some("p")));
        assert!(watches.store("flow", flow, Some("f")));
        watches.remove("flow");
        assert_eq!(watches.slots["panel"].1, Some("p"));
    }

    #[test]
    fn a_current_result_replaces_the_value_and_none_clears_it() {
        let mut watches = Watches::<&str>::default();
        let g = watches.begin("panel");
        assert!(watches.store("panel", g, Some("a")));
        let g = watches.begin("panel");
        assert!(watches.store("panel", g, None));
        assert_eq!(watches.slots["panel"].1, None);
    }
}

#[cfg(test)]
mod build_layout_tests {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use backend::{Snapshot, StreamRecord};
    use serde_json::json;

    const PROJECT_TEXT: &str = r#"{"layout":{"panes":[{"name":"agent","command":"evil"},
        {"name":"sh","command":"worse","split":{"direction":"down"}}]}}"#;

    fn sources(text: &str) -> trust::Sources {
        trust::Sources {
            root: "/work/app".into(),
            project: Some(("/work/app/.poimen/config.json".into(), text.into())),
            legacy: None,
        }
    }

    fn run(sources: &trust::Sources, user: Option<&layout::Layout>, allow: bool) -> Vec<String> {
        let fake = FakeBackend::default();
        let warnings = build_layout(
            &fake,
            "/work/app",
            Some("w1:root"),
            sources,
            user,
            allow,
            None,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        fake.calls()
    }

    fn typed(calls: &[String]) -> Vec<&String> {
        calls.iter().filter(|c| c.starts_with("input ")).collect()
    }

    #[test]
    fn untrusted_project_layout_builds_panes_but_types_nothing() {
        let calls = run(&sources(PROJECT_TEXT), None, false);
        assert_eq!(
            calls,
            [
                "rename w1:root agent",
                "split w1:root down /work/app 0.5",
                "rename w1:p11 sh"
            ]
        );
    }

    #[test]
    fn trusted_project_layout_types_its_commands() {
        let calls = run(&sources(PROJECT_TEXT), None, true);
        assert_eq!(typed(&calls).len(), 2);
    }

    #[test]
    fn a_missing_root_pane_is_a_warning_and_nothing_is_called() {
        let fake = FakeBackend::default();
        let warnings = build_layout(
            &fake,
            "/work/app",
            None,
            &sources(PROJECT_TEXT),
            None,
            true,
            None,
        );
        assert_eq!(warnings.len(), 1);
        assert!(fake.calls().is_empty());
    }

    type SetupRun = ((bool, Option<String>), Vec<String>);

    fn setup_calls(sources: &trust::Sources, allow: bool, fail_on: Option<&str>) -> SetupRun {
        let fake = FakeBackend::default();
        if let Some(prefix) = fail_on {
            fake.fail_on(prefix);
        }
        let out = run_setup(&fake, "w1", "/work/app", sources, allow);
        (out, fake.calls())
    }

    #[test]
    fn a_trusted_setup_runs_in_a_setup_tab_of_the_new_space() {
        let ((started, warning), calls) =
            setup_calls(&sources(r#"{"setup":"pnpm install"}"#), true, None);
        assert!(started && warning.is_none());
        assert_eq!(
            calls,
            [
                "create_terminal w1 /work/app",
                "rename w1:t0 setup",
                "input w1:t0 pnpm install"
            ]
        );
    }

    #[test]
    fn an_untrusted_project_runs_no_setup() {
        let ((started, warning), calls) =
            setup_calls(&sources(r#"{"setup":"pnpm install"}"#), false, None);
        assert!(!started && warning.is_none());
        assert!(calls.is_empty());
    }

    #[test]
    fn no_setup_in_the_file_means_no_tab() {
        let ((started, _), calls) = setup_calls(&sources(PROJECT_TEXT), true, None);
        assert!(!started);
        assert!(calls.is_empty());
        let ((started, _), calls) = setup_calls(&trust::Sources::default(), true, None);
        assert!(!started && calls.is_empty());
    }

    #[test]
    fn a_failing_setup_start_is_a_warning() {
        let ((started, warning), _) =
            setup_calls(&sources(r#"{"setup":"make"}"#), true, Some("input"));
        assert!(!started);
        assert_eq!(warning.as_deref(), Some("setup not started: boom"));
    }

    #[test]
    fn the_user_layout_needs_no_trust() {
        let user = layout::default_layout();
        // No project layout: the user's `claude` is typed although project commands are blocked.
        let calls = run(&sources("{\"setup\":\"make\"}"), Some(&user), false);
        assert_eq!(typed(&calls).len(), 1);
        let calls = run(&trust::Sources::default(), Some(&user), false);
        assert_eq!(typed(&calls).len(), 1);
    }

    // The command layer through a fake backend.

    fn tmp_dir() -> String {
        std::env::temp_dir()
            .to_string_lossy()
            .trim_end_matches('/')
            .to_owned()
    }

    #[test]
    fn create_space_creates_focuses_and_lays_out_a_new_space() {
        let fake = FakeBackend::default();
        let dir = tmp_dir();
        let outcome = create_space_with(&fake, None, &dir, Some("demo"), None, None).unwrap();
        assert!(outcome.created && outcome.trust.is_none());
        assert_eq!(outcome.workspace_id, "w9");
        let calls = fake.calls();
        assert_eq!(calls[0], "snapshot");
        assert_eq!(calls[1], format!("create_space {dir} demo"));
        assert_eq!(calls[2], "focus w9");
        // The default layout: agent, tests, shell.
        assert!(calls.contains(&"rename w9:root agent".to_owned()));
        assert_eq!(calls.iter().filter(|c| c.starts_with("split ")).count(), 2);
    }

    #[test]
    fn create_space_focuses_an_existing_space_for_the_directory() {
        let fake = FakeBackend::default();
        let dir = tmp_dir();
        *fake.snapshot.lock().unwrap() = Snapshot {
            workspaces: json!([{ "workspace_id": "w2", "label": "demo", "number": 1 }]),
            tabs: json!([]),
            panes: json!([{ "pane_id": "w2:p1", "workspace_id": "w2", "cwd": dir }]),
        };
        let outcome = create_space_with(&fake, None, &dir, None, None, Some("claude 'x'")).unwrap();
        assert!(!outcome.created);
        assert_eq!(outcome.workspace_id, "w2");
        // Focused only: no layout, no command.
        assert_eq!(fake.calls(), ["snapshot", "focus w2"]);
    }

    #[test]
    fn create_space_runs_the_task_command_in_the_root_pane_of_the_layout() {
        let fake = FakeBackend::default();
        let dir = tmp_dir();
        let label = "#12 Add picker";
        create_space_with(&fake, None, &dir, Some(label), None, Some("claude 'x'")).unwrap();
        let calls = fake.calls();
        assert_eq!(calls[1], format!("create_space {dir} {label}"));
        assert!(calls.contains(&"rename w9:root agent".to_owned()));
        assert!(calls.contains(&"input w9:root claude 'x'".to_owned()));
        assert!(!calls.iter().any(|c| c == "input w9:root claude"));
    }

    #[test]
    fn create_space_with_a_failing_create_runs_no_command() {
        let fake = FakeBackend::default();
        fake.fail_on("create_space");
        let err = create_space_with(&fake, None, &tmp_dir(), None, None, Some("c")).unwrap_err();
        assert_eq!(err, "boom");
        assert!(!fake.calls().iter().any(|c| c.starts_with("input")));
    }

    #[test]
    fn create_space_reports_a_backend_error() {
        let fake = FakeBackend::default();
        fake.fail_on("create_space");
        let err = create_space_with(&fake, None, &tmp_dir(), None, None, None).unwrap_err();
        assert_eq!(err, "boom");
    }

    #[test]
    fn create_space_rejects_a_missing_directory_before_calling_the_backend() {
        let fake = FakeBackend::default();
        assert!(create_space_with(&fake, None, "/no/such/dir/at/all", None, None, None).is_err());
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_fake_stream_delivers_neutral_records_and_says_if_it_is_emulated() {
        let mut fake = FakeBackend::default();
        fake.emulated = true;
        fake.records.lock().unwrap().push(StreamRecord::Output {
            bytes: "AA==".into(),
        });
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let backend: &dyn Backend = &fake;
        let opened = backend
            .open_stream(
                StreamRequest {
                    mode: Mode::Observe,
                    target: "w1:p1",
                    cols: 80,
                    rows: 24,
                    takeover: false,
                },
                Box::new(move |_, record| sink.lock().unwrap().push(record)),
            )
            .unwrap();
        assert!(opened.emulated);
        assert_eq!(
            *seen.lock().unwrap(),
            [StreamRecord::Output {
                bytes: "AA==".into()
            }]
        );
    }

    fn raw(body: &InvokeResponseBody) -> Option<&[u8]> {
        match body {
            InvokeResponseBody::Raw(bytes) => Some(bytes),
            InvokeResponseBody::Json(_) => None,
        }
    }

    fn text(body: &InvokeResponseBody) -> Option<&str> {
        match body {
            InvokeResponseBody::Json(text) => Some(text),
            InvokeResponseBody::Raw(_) => None,
        }
    }

    #[test]
    fn emulated_output_goes_over_the_channel_as_raw_bytes() {
        let messages = channel_messages(
            &StreamRecord::Output {
                bytes: "aGk=".into(),
            },
            true,
        );
        assert_eq!(messages.len(), 1);
        assert_eq!(raw(&messages[0]), Some(&b"hi"[..]));
    }

    #[test]
    fn an_emulated_snapshot_is_a_json_reset_without_bytes_then_raw_bytes() {
        let record = StreamRecord::Reset {
            cols: 90,
            rows: 30,
            bytes: "aGk=".into(),
        };
        let messages = channel_messages(&record, true);
        assert_eq!(messages.len(), 2);
        let head: serde_json::Value = serde_json::from_str(text(&messages[0]).unwrap()).unwrap();
        assert_eq!(
            head,
            json!({"type": "reset", "cols": 90, "rows": 30, "bytes": ""})
        );
        assert_eq!(raw(&messages[1]), Some(&b"hi"[..]));
    }

    #[test]
    fn an_empty_snapshot_sends_only_the_reset() {
        let record = StreamRecord::Reset {
            cols: 80,
            rows: 24,
            bytes: String::new(),
        };
        assert_eq!(channel_messages(&record, true).len(), 1);
    }

    #[test]
    fn other_records_and_rendered_streams_are_json() {
        let closed = StreamRecord::Closed {
            reason: "x".into(),
            exit_code: None,
            code: Some(backend::CloseCode::Exited),
        };
        let messages = channel_messages(&closed, true);
        let value: serde_json::Value = serde_json::from_str(text(&messages[0]).unwrap()).unwrap();
        assert_eq!(value["code"], "exited");
        let frame = StreamRecord::Output {
            bytes: "aGk=".into(),
        };
        let messages = channel_messages(&frame, false);
        let value: serde_json::Value = serde_json::from_str(text(&messages[0]).unwrap()).unwrap();
        assert_eq!(value, json!({"type": "output", "bytes": "aGk="}));
    }

    #[test]
    fn the_backend_setting_picks_the_backend_and_unknown_means_herdr() {
        assert_eq!(make_backend("poimen").1, "poimen");
        assert_eq!(make_backend("herdr").1, "herdr");
        assert_eq!(make_backend("").1, "herdr");
        assert!(make_backend("poimen").0.emulated());
        assert!(!make_backend("herdr").0.emulated());
    }
}
