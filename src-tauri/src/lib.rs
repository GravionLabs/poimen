use poimen_core::agent_actions;
use poimen_core::{copilot, transcripts};
mod backend;
#[cfg(test)]
mod backend_fake;
mod changes;
mod changes_git;
mod deliver;
use poimen_core::git;
mod git_sync;
use poimen_core::github;
mod github_branches;
mod github_issues;
mod github_milestones;
mod github_pulls;
use poimen_core::gui_path;
use poimen_core::harness;
mod herdr;
mod hooks;
mod layout;
#[cfg(test)]
mod live;
mod machines;
mod poimen_backend;
mod poimen_server;
mod project;
mod project_edit;
mod remote_runs;
mod remote_transcripts;
mod schemes;
mod settings;
mod spaces;
mod stream;
mod task_start;
use poimen_core::tasks;
use poimen_core::templates;
mod terminal_colors;
use poimen_core::trust;
mod updater;
mod watch;
mod worktree;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
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
struct SubagentWatch(Mutex<Watches<FollowHandle>>);

/// A running transcript follow of either kind (a local file watch, or the poll of a machine's
/// server); dropping it stops it.
type FollowHandle = Box<dyn std::any::Any + Send>;

/// The running watches of deliver loop status files by repository root.
#[derive(Default)]
struct DeliverWatch(Mutex<deliver::Watches>);

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

/// Runs `f` on the backend `id` addresses (#693): the machine of a qualified id (`<machine>::<id>`),
/// else the local one, with the id on that backend. An offline machine fails; the local backend is
/// never the fallback for a machine's id.
async fn on_routed<T: Send + 'static>(
    local: &Shared,
    machines: &SharedMachines,
    id: &str,
    f: impl FnOnce(&dyn Backend, &str) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let routed = machines.route(local, id)?;
    on_backend(&routed.backend, move |b| f(b, &routed.id)).await
}

/// `new`, an id returned by a backend, in the form of `origin`, the id the request addressed.
fn qualify_like(origin: &str, new: String) -> String {
    match machines::split_id(origin).0 {
        Some(machine) => format!("{machine}::{new}"),
        None => new,
    }
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
    machines: State<'_, SharedMachines>,
    target: String,
    direction: SplitDirection,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    let origin = target.clone();
    let created = on_routed(backend.inner(), machines.inner(), &target, move |b, id| {
        b.split(id, direction, cwd.as_deref(), None)
    })
    .await?;
    Ok(created.map(|id| qualify_like(&origin, id)))
}

/// Closes a pane. The UI confirms first; the backend ends the pane's process.
#[tauri::command]
async fn pane_close(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    terminal_id: String,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &terminal_id, |b, id| {
        b.close_terminal(id)
    })
    .await
}

/// Restarts a terminal on the bundled PTY host: a new terminal with the same directory, program
/// and place; the old process ends. The UI confirms first. Returns the new pane id.
#[tauri::command]
async fn pane_restart(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    terminal_id: String,
) -> Result<Option<String>, String> {
    let created = on_routed(backend.inner(), machines.inner(), &terminal_id, |b, id| {
        b.restart_terminal(id)
    })
    .await?;
    Ok(created.map(|id| qualify_like(&terminal_id, id)))
}

/// Closes a tab with all its terminals (`tab.close` on the poimen backend). The UI confirms first.
#[tauri::command]
async fn tab_close(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    tab_id: String,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &tab_id, |b, id| {
        b.close_tab(id)
    })
    .await
}

/// Renames a tab (`tab.rename` on the poimen backend); a blank name is rejected.
#[tauri::command]
async fn tab_rename(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    tab_id: String,
    name: String,
) -> Result<(), String> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err("The name of a tab cannot be empty.".into());
    }
    on_routed(backend.inner(), machines.inner(), &tab_id, move |b, id| {
        b.rename_tab(id, &name)
    })
    .await
}

/// Sets the ratio of a split of a tab (`tab.resize_split` on the poimen backend).
#[tauri::command]
async fn tab_resize_split(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    tab_id: String,
    split_id: String,
    ratio: f64,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &tab_id, move |b, id| {
        b.resize_split(id, &split_id, ratio)
    })
    .await
}

/// Focuses a pane of a tab for every client (`tab.focus` on the poimen backend).
#[tauri::command]
async fn tab_focus(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    tab_id: String,
    terminal_id: String,
) -> Result<(), String> {
    // The terminal is of the tab's machine: its own id is what the machine's server knows.
    let terminal = machines::split_id(&terminal_id).1.to_owned();
    on_routed(backend.inner(), machines.inner(), &tab_id, move |b, id| {
        b.focus_tab(id, &terminal)
    })
    .await
}

/// Closes a space with all its panes; only on an explicit user action (the UI confirms first when
/// it has panes). Afterwards `focus`, the neighbouring space, is focused on a best-effort basis; a
/// failing close leaves the space untouched and returns the backend's error.
#[tauri::command]
async fn space_close(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    space_id: String,
    focus: Option<String>,
) -> Result<(), String> {
    // The neighbour to focus afterwards counts only on the same backend.
    let machine = machines::split_id(&space_id).0.map(str::to_owned);
    let focus =
        focus.filter(|id| !id.is_empty() && machines::split_id(id).0.map(str::to_owned) == machine);
    on_routed(
        backend.inner(),
        machines.inner(),
        &space_id,
        move |b, id| {
            b.close_space(id)?;
            if let Some(next) = focus {
                let _ = b.focus_space(machines::split_id(&next).1);
            }
            Ok(())
        },
    )
    .await
}

/// Renames a space. A blank label is rejected here too; the UI checks first.
#[tauri::command]
async fn space_rename(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    space_id: String,
    label: String,
) -> Result<(), String> {
    let label = label.trim().to_owned();
    if label.is_empty() {
        return Err("The name of a space cannot be empty.".into());
    }
    on_routed(
        backend.inner(),
        machines.inner(),
        &space_id,
        move |b, id| b.rename_space(id, &label),
    )
    .await
}

/// Starts a supported agent in an existing pane at a shell prompt; can take a while.
#[tauri::command]
async fn agent_start(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    terminal_id: String,
    kind: String,
    name: Option<String>,
    args: Vec<String>,
) -> Result<(), String> {
    on_routed(
        backend.inner(),
        machines.inner(),
        &terminal_id,
        move |b, id| b.agent_start(id, &kind, name.as_deref(), &args),
    )
    .await
}

/// Renames an agent; an empty name clears it.
#[tauri::command]
async fn agent_rename(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    target: String,
    name: Option<String>,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &target, move |b, id| {
        b.agent_rename(id, name.as_deref())
    })
    .await
}

/// Sends a prompt to an agent without focusing its terminal; no controller needed. Stalled,
/// timed-out and blocked agents are results, not errors.
#[tauri::command]
async fn agent_prompt(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    target: String,
    text: String,
) -> Result<herdr::PromptOutcome, String> {
    on_routed(backend.inner(), machines.inner(), &target, move |b, id| {
        Ok(b.agent_prompt(id, &text))
    })
    .await
}

/// Renames a pane; an empty label clears it.
#[tauri::command]
async fn pane_rename(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    terminal_id: String,
    label: Option<String>,
) -> Result<(), String> {
    on_routed(
        backend.inner(),
        machines.inner(),
        &terminal_id,
        move |b, id| b.rename_terminal(id, label.as_deref()),
    )
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

/// One pull request in detail (body, checks, reviews, linked issues) in the repository of `root`.
/// Read-only; the GraphQL cache of the client holds it for 30 s.
#[tauri::command]
async fn github_pull_detail(
    client: State<'_, GithubClient>,
    root: String,
    number: u64,
) -> Result<github_pulls::PullDetail, github::GhError> {
    let client = client.inner().clone();
    let root = github_pulls::expand_home(&root, home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || github_pulls::load_detail(&client, &root, number))
        .await
        .map_err(|e| github::GhError::Other {
            message: e.to_string(),
        })?
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

/// One issue in detail (body, labels, sub-issues, closing pull requests) in the repository of
/// `root`. Read-only; the GraphQL cache of the client holds it for 30 s.
#[tauri::command]
async fn github_issue_detail(
    client: State<'_, GithubClient>,
    root: String,
    number: u64,
) -> Result<github_issues::IssueDetail, github::GhError> {
    let client = client.inner().clone();
    let root = github_pulls::expand_home(&root, home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || github_issues::load_detail(&client, &root, number))
        .await
        .map_err(|e| github::GhError::Other {
            message: e.to_string(),
        })?
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
    machines: State<'_, SharedMachines>,
    space_id: String,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    let created = on_routed(
        backend.inner(),
        machines.inner(),
        &space_id,
        move |b, id| b.create_terminal(id, cwd.as_deref()),
    )
    .await?;
    Ok(created.map(|id| qualify_like(&space_id, id)))
}

/// The space template that applies to `dir`; `origin` is read only when a template matches by
/// remote.
fn template_for<'a>(
    dir: &str,
    user: &'a settings::Settings,
    home: Option<&std::path::Path>,
) -> Option<&'a templates::Template> {
    poimen_core::space_layout::template_for(dir, &user.space_templates, home)
}

/// The template a new space for a directory would use, for the new-space dialog.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
struct TemplateHint {
    name: String,
    agent: String,
    /// The default template (no `match`), which the dialog says.
    default: bool,
}

fn resolve_space_template_with(home: Option<&std::path::Path>, cwd: &str) -> Option<TemplateHint> {
    let dir = spaces::resolve_dir(cwd, home).ok()?;
    let user = settings::load(home?);
    template_for(&dir, &user, home).map(|t| TemplateHint {
        name: t.name.clone(),
        agent: t.agent.clone(),
        default: t.dir.is_none() && t.remote.is_none(),
    })
}

/// The space template that applies to `cwd` (read-only; the rules of `create_space`), or `None`
/// for the default.
#[tauri::command]
async fn resolve_space_template(cwd: String) -> Option<TemplateHint> {
    tauri::async_runtime::spawn_blocking(move || {
        resolve_space_template_with(home_dir().as_deref(), &cwd)
    })
    .await
    .ok()
    .flatten()
}

/// Writes only `spaceTemplates` after validating it; the settings watcher emits the change.
#[tauri::command]
fn set_space_templates(templates: serde_json::Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_space_templates(&home, &templates)
}

/// Creates a space for a directory and focuses it; when a space for the directory exists (a pane
/// exactly in it, else one labelled with its name whose panes report no other directory) that one
/// is focused.
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
/// layout's root pane in a new space (a task's command); an existing space runs nothing. A backend
/// that builds layouts itself (poimen-server) gets the request as it is and answers the trust
/// question; for the others (herdr) the app gates the project file, creates the space and applies
/// the layout (`layout::apply`).
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
    if let Some(space_id) = spaces::find_space(&dir, &snapshot) {
        backend.focus_space(&space_id)?;
        return Ok(spaces::SpaceOutcome {
            space_id,
            created: false,
            warnings: Vec::new(),
            setup_started: false,
            trust: None,
        });
    }
    let label = spaces::space_label(&dir, label.unwrap_or_default(), home, &snapshot.spaces);
    // poimen-server resolves and builds the layout itself and asks for trust first.
    let request = backend::LayoutRequest {
        cwd: &dir,
        label: &label,
        decision: decision.as_ref(),
        command: root_command,
    };
    if let Some(result) = backend.create_space_with_layout(request) {
        return match result? {
            backend::LaidOut::Trust(request) => Ok(spaces::SpaceOutcome {
                space_id: String::new(),
                created: false,
                warnings: Vec::new(),
                setup_started: false,
                trust: Some(request),
            }),
            backend::LaidOut::Created {
                space_id,
                warnings,
                setup_started,
            } => {
                backend.focus_space(&space_id)?;
                Ok(spaces::SpaceOutcome {
                    space_id,
                    created: true,
                    warnings,
                    setup_started,
                    trust: None,
                })
            }
        };
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
                space_id: String::new(),
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
    let created = backend.create_space(&dir, &label)?;
    let space_id = created.space_id.clone();
    backend.focus_space(&space_id)?;
    let (agent_command, user_layout, mut warnings) = layout_inputs(&dir, user.as_ref(), home);
    warnings.extend(build_layout(
        backend,
        &dir,
        created.root_terminal.as_deref(),
        &sources,
        user_layout.as_ref(),
        allow,
        root_command,
        agent_command.as_deref(),
    ));
    let (setup_started, warning) = run_setup(backend, &space_id, &dir, &sources, allow);
    warnings.extend(warning);
    Ok(spaces::SpaceOutcome {
        space_id,
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
#[allow(clippy::too_many_arguments)]
fn build_layout(
    backend: &dyn Backend,
    dir: &str,
    root_pane: Option<&str>,
    sources: &trust::Sources,
    user_layout: Option<&layout::Layout>,
    allow_project_commands: bool,
    root_command: Option<&str>,
    agent_command: Option<&str>,
) -> Vec<String> {
    let planned = plan_layout(
        sources,
        user_layout,
        allow_project_commands,
        root_command,
        agent_command,
    );
    apply_planned(backend, planned, dir, root_pane)
}

/// Applies a layout of [`plan_layout`] from `root_pane`; every problem is a warning.
fn apply_planned(
    backend: &dyn Backend,
    planned: Result<(layout::Layout, Vec<String>), String>,
    dir: &str,
    root_pane: Option<&str>,
) -> Vec<String> {
    let (layout, mut warnings) = match planned {
        Ok(planned) => planned,
        Err(warning) => return vec![warning],
    };
    let Some(root_pane) = root_pane else {
        return vec!["the backend did not report the root pane; no layout applied".into()];
    };
    if let Err(e) = layout::apply(backend, &layout, dir, root_pane) {
        warnings.push(format!("layout not fully applied: {e}"));
    }
    warnings
}

/// The template's agent command and the user layout (the template's, else `spaces.layout`) for a
/// space of `dir`, with the template's warnings. Shared by `create_space` and `rebuild_space`.
fn layout_inputs(
    dir: &str,
    user: Option<&settings::Settings>,
    home: Option<&std::path::Path>,
) -> (Option<String>, Option<layout::Layout>, Vec<String>) {
    let Some(user) = user else {
        return (None, None, Vec::new());
    };
    let path = std::sync::OnceLock::new();
    poimen_core::space_layout::inputs(
        dir,
        &user.space_templates,
        &user.harnesses,
        user.spaces_layout.as_ref(),
        home,
        |p| gui_path::resolve_program(p, path.get_or_init(gui_path::child_path)).is_some(),
    )
}

/// The layout a new space gets from the already read project `sources` (the precedence of
/// [`build_layout`]) with the resolution warnings; `Err` when none could be resolved.
fn plan_layout(
    sources: &trust::Sources,
    user_layout: Option<&layout::Layout>,
    allow_project_commands: bool,
    root_command: Option<&str>,
    agent_command: Option<&str>,
) -> Result<(layout::Layout, Vec<String>), String> {
    poimen_core::space_layout::plan(
        sources,
        user_layout,
        allow_project_commands,
        root_command,
        agent_command,
    )
}

/// What `rebuild_space` did with a space (wire shape, tagged by `state`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum RebuildOutcome {
    /// The layout was built again (and the setup started, when allowed).
    Rebuilt {
        warnings: Vec<String>,
        setup_started: bool,
    },
    /// Something needs the user's go first (an agent pane, the setup, an untrusted project file);
    /// nothing was created.
    Consent { reasons: Vec<String> },
    /// A confirmed rebuild found the project file untrusted; nothing was created.
    Trust { request: trust::TrustRequest },
    /// The space has no directory or the directory is gone; nothing was created.
    Missing { reason: String },
    /// The space has terminals or no longer exists; nothing to do.
    NotEmpty,
}

/// Whether `command` starts one of the agent `harnesses`: its program (first word, without path
/// and `.exe`) is a harness id or the program of a harness command.
fn is_agent_command(command: &str, harnesses: &[harness::Harness]) -> bool {
    fn program(word: &str) -> String {
        let word = word.trim_matches(|c| c == '\'' || c == '"');
        let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
        let lower = base.to_ascii_lowercase();
        lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
    }
    let Some(first) = command.split_whitespace().next() else {
        return false;
    };
    let wanted = program(first);
    harnesses.iter().any(|h| {
        program(&h.id) == wanted || h.command.first().is_some_and(|c| program(c) == wanted)
    })
}

/// Why rebuilding needs the user's go: every pane of `layout` that starts an agent, and `setup`.
fn consent_reasons(
    layout: Option<&layout::Layout>,
    harnesses: &[harness::Harness],
    setup: Option<&str>,
) -> Vec<String> {
    let mut reasons: Vec<String> = layout
        .into_iter()
        .flat_map(|l| &l.panes)
        .filter_map(|p| {
            let command = p.command.as_deref()?;
            is_agent_command(command, harnesses)
                .then(|| format!("pane \"{}\" starts an agent: {command}", p.name))
        })
        .collect();
    if let Some(setup) = setup {
        reasons.push(format!("setup runs: {setup}"));
    }
    reasons
}

/// Rebuilds the layout of a space without terminals (after a server restart: spaces persist,
/// terminals do not) in the space's own directory, through the path of `create_space`: the
/// layout is resolved again (project file > template > `spaces.layout` > default), the project
/// file goes through `trust::gate` and the setup runs only when allowed. A backend that builds
/// layouts itself (poimen-server, `Backend::apply_layout`) gets the confirmed rebuild and the
/// trust answer and stores the trust; for the others (herdr) the app stores it and applies the
/// layout (`layout::apply`). Unconfirmed, it builds
/// only when nothing needs consent (no agent pane, no setup, nothing to trust); otherwise it
/// answers `Consent` and creates nothing. A space with terminals is left alone.
fn rebuild_space_with(
    backend: &dyn Backend,
    home: Option<&std::path::Path>,
    space_id: &str,
    decision: Option<trust::Decision>,
    confirmed: bool,
) -> Result<RebuildOutcome, String> {
    let snapshot = backend.snapshot()?;
    let Some(space) = snapshot.spaces.iter().find(|s| s.space_id == space_id) else {
        return Ok(RebuildOutcome::NotEmpty);
    };
    if snapshot.terminals_of(space_id).next().is_some() {
        return Ok(RebuildOutcome::NotEmpty);
    }
    let cwd = space.cwd.as_deref().unwrap_or_default().trim();
    if cwd.is_empty() {
        return Ok(RebuildOutcome::Missing {
            reason: "the space has no directory".into(),
        });
    }
    let Ok(dir) = spaces::resolve_dir(cwd, home) else {
        return Ok(RebuildOutcome::Missing {
            reason: format!("the directory {cwd} no longer exists"),
        });
    };
    let root = git::toplevel(&dir).unwrap_or_else(|| dir.clone());
    let sources = trust::Sources::read(&root);
    let user = home.map(settings::load);
    let trusted = user
        .as_ref()
        .map(|s| s.trusted_projects.clone())
        .unwrap_or_default();
    // Nothing is stored yet: a backend that builds layouts itself stores the trust on its own.
    let (allow, store, asked) = match trust::gate(&sources, &trusted, decision.as_ref()) {
        trust::Gate::Ask(_) if !confirmed => {
            return Ok(RebuildOutcome::Consent {
                reasons: vec!["the project file is not trusted yet".into()],
            });
        }
        trust::Gate::Ask(request) => (false, None, Some(request)),
        trust::Gate::Strip => (false, None, None),
        trust::Gate::Run { store } => (true, store, None),
    };
    let plan = || {
        let (agent_command, user_layout, warnings) = layout_inputs(&dir, user.as_ref(), home);
        let planned = plan_layout(
            &sources,
            user_layout.as_ref(),
            allow,
            None,
            agent_command.as_deref(),
        );
        (planned, warnings)
    };
    if !confirmed {
        let harnesses = user
            .as_ref()
            .map(|s| s.harnesses.clone())
            .unwrap_or_else(harness::default_harnesses);
        let setup = allow.then(|| project::setup_of(sources.refs().0)).flatten();
        let (planned, _) = plan();
        let layout = planned.as_ref().ok().map(|(l, _)| l);
        let reasons = consent_reasons(layout, &harnesses, setup.as_deref());
        if !reasons.is_empty() {
            return Ok(RebuildOutcome::Consent { reasons });
        }
    }
    // poimen-server resolves the layout again, answers the trust question itself and builds the
    // tabs into the space (`space.apply_layout`).
    let request = backend::ApplyRequest {
        space_id,
        cwd: &dir,
        preset: None,
        decision: decision.as_ref(),
    };
    if let Some(result) = backend.apply_layout(request) {
        return Ok(match result? {
            backend::LaidOut::Trust(request) => RebuildOutcome::Trust { request },
            backend::LaidOut::Created {
                warnings,
                setup_started,
                ..
            } => RebuildOutcome::Rebuilt {
                warnings,
                setup_started,
            },
        });
    }
    if let Some(request) = asked {
        return Ok(RebuildOutcome::Trust { request });
    }
    if let Some(hash) = store {
        let home = home.ok_or("the home directory is not known")?;
        settings::trust_project(home, &sources.root, &hash)?;
    }
    let (planned, mut warnings) = plan();
    let root_pane = backend.create_terminal(space_id, Some(&dir))?;
    warnings.extend(apply_planned(backend, planned, &dir, root_pane.as_deref()));
    let (setup_started, warning) = run_setup(backend, space_id, &dir, &sources, allow);
    warnings.extend(warning);
    Ok(RebuildOutcome::Rebuilt {
        warnings,
        setup_started,
    })
}

/// Rebuilds the layout of a space without terminals (see [`rebuild_space_with`]); `confirmed` is
/// the user's "Rebuild", `decision` the answer of the trust dialog.
#[tauri::command]
async fn rebuild_space(
    backend: State<'_, Shared>,
    space_id: String,
    decision: Option<trust::Decision>,
    confirmed: bool,
) -> Result<RebuildOutcome, String> {
    on_backend(backend.inner(), move |b| {
        rebuild_space_with(b, home_dir().as_deref(), &space_id, decision, confirmed)
    })
    .await
}

/// Starts the `setup` command of the project file in a `setup` tab of a just-created space. Only
/// when project commands are allowed (trusted); a failure is a warning, never a reason to close
/// the space. Returns whether it started and the warning, if any.
fn run_setup(
    backend: &dyn Backend,
    space_id: &str,
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
    match project::start_setup(backend, space_id, dir, &command) {
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

/// The text of the project file of the repository containing `dir`, for the space overview.
#[tauri::command]
async fn project_file(dir: String) -> Result<project_edit::ProjectFile, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = git::toplevel(&dir).unwrap_or(dir);
        project_edit::read(&root)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Merges `patch` (`layout`, `setup`, `teardown`) into the project file; the new content goes
/// through `trust::gate` first (an untrusted hash answers with the request, nothing written).
#[tauri::command]
async fn project_save(
    dir: String,
    patch: serde_json::Value,
    decision: Option<trust::Decision>,
) -> Result<project_edit::SaveOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = git::toplevel(&dir).unwrap_or(dir);
        let trusted = get_settings().trusted_projects;
        let outcome = project_edit::save(&root, &patch, &trusted, decision.as_ref())?;
        if let Some(hash) = &outcome.store {
            let home = home_dir().ok_or("the home directory is not known")?;
            settings::trust_project(&home, &root, hash)?;
        }
        Ok(outcome)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The `commands[]` and `presets[]` of the project file of the repository of `dir`, for the
/// palette (names only), and whether the file still needs the user's trust.
#[tauri::command]
async fn project_entries(dir: String) -> project::Entries {
    tauri::async_runtime::spawn_blocking(move || {
        let root = git::toplevel(&dir).unwrap_or(dir);
        let sources = trust::Sources::read(&root);
        project::entries(&sources, &get_settings().trusted_projects)
    })
    .await
    .unwrap_or_default()
}

/// Runs a palette entry of the project file in the space `space_id` (a user action). Goes
/// through `trust::gate` with the one read of the file; an untrusted file answers `ask`.
#[tauri::command]
async fn project_run(
    backend: State<'_, Shared>,
    space_id: String,
    dir: String,
    entry: project::Entry,
    decision: Option<trust::Decision>,
) -> Result<project::EntryOutcome, String> {
    on_backend(backend.inner(), move |b| {
        let root = git::toplevel(&dir).unwrap_or_else(|| dir.clone());
        let sources = trust::Sources::read(&root);
        let home = home_dir();
        let trusted = get_settings().trusted_projects;
        let persist = |hash: &str| -> Result<(), String> {
            let home = home.as_deref().ok_or("the home directory is not known")?;
            settings::trust_project(home, &sources.root, hash)
        };
        project::run_entry(
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

type SharedMachines = std::sync::Arc<machines::Machines>;

/// The machines of the settings with their connection state (#279).
#[tauri::command]
fn list_machines(machines: State<'_, SharedMachines>) -> Vec<machines::MachineStatus> {
    machines.list()
}

/// Tries to connect an offline or incompatible machine at once.
#[tauri::command]
fn reconnect_machine(machines: State<'_, SharedMachines>, name: String) -> Result<(), String> {
    machines.reconnect(&name)
}

/// What a connected machine has for tasks (git, gh and its login, agents, hooks) and the command
/// to fix each gap (#281); fails while the machine is not connected. Nothing is run or installed
/// by poimen beyond the read-only checks of the machine's own server.
#[tauri::command]
async fn machine_check(
    machines: State<'_, SharedMachines>,
    name: String,
) -> Result<poimen_client::protocol::MachineCheckResult, String> {
    let backend = machines.backend(&name)?;
    tauri::async_runtime::spawn_blocking(move || backend.machine_check())
        .await
        .map_err(|e| e.to_string())?
}

/// The spaces of a machine's server; fails while the machine is not connected.
#[tauri::command]
async fn machine_snapshot(
    machines: State<'_, SharedMachines>,
    name: String,
) -> Result<backend::Snapshot, String> {
    let backend: Shared = machines.backend(&name)?;
    on_backend(&backend, |b| b.snapshot()).await
}

/// `repo.ensure` on a machine: its clone of `owner/repo`, cloned when absent (#283).
#[tauri::command]
async fn machine_repo_ensure(
    machines: State<'_, SharedMachines>,
    name: String,
    owner: String,
    repo: String,
) -> Result<poimen_client::protocol::RepoEnsureResult, String> {
    let backend = machines.backend(&name)?;
    tauri::async_runtime::spawn_blocking(move || backend.ensure_repo(&owner, &repo))
        .await
        .map_err(|e| e.to_string())?
}

/// `worktree.create` on a machine: the worktree of a task issue in its clone (#283).
#[tauri::command]
async fn machine_worktree_create(
    machines: State<'_, SharedMachines>,
    name: String,
    repo_dir: String,
    issue: poimen_client::protocol::TaskIssue,
    mode: String,
) -> Result<poimen_client::protocol::WorktreeCreateResult, String> {
    let backend = machines.backend(&name)?;
    tauri::async_runtime::spawn_blocking(move || backend.create_worktree(&repo_dir, issue, &mode))
        .await
        .map_err(|e| e.to_string())?
}

/// Creates the space of a task on a machine, or focuses its existing space for `cwd`.
#[tauri::command]
async fn machine_create_space(
    machines: State<'_, SharedMachines>,
    name: String,
    cwd: String,
    label: Option<String>,
    command: Option<String>,
) -> Result<spaces::SpaceOutcome, String> {
    let backend: Shared = machines.backend(&name)?;
    on_backend(&backend, move |b| {
        remote_create_space(
            b,
            &cwd,
            label.as_deref().unwrap_or_default(),
            command.as_deref(),
        )
    })
    .await
}

/// A tab with a command in a space of a machine (a task mode without a worktree).
#[tauri::command]
async fn machine_task_tab(
    machines: State<'_, SharedMachines>,
    name: String,
    space_id: String,
    dir: String,
    tab: String,
    command: String,
) -> Result<(), String> {
    let backend: Shared = machines.backend(&name)?;
    on_backend(&backend, move |b| {
        task_start::start_tab(b, &space_id, &dir, &tab, &command)
    })
    .await
}

/// "Clean up merged tasks" on a machine (#284): the merged pull requests come from the local `gh`
/// for the GitHub repository of the local clone at `dir`; the git work runs on the machine's
/// server (`worktree.cleanup` in its clone of that repository, the rules of #192). Only while the
/// machine is `connected`. Destructive, so only on a user action.
#[tauri::command]
async fn machine_task_cleanup(
    machines: State<'_, SharedMachines>,
    name: String,
    dir: String,
    focused_space: Option<String>,
    skip_teardown: Option<bool>,
) -> Result<worktree::CleanupReport, String> {
    let backend = machines.connected_backend(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = worktree::main_worktree(std::path::Path::new(&dir))?;
        let repo = github::repo_of(&root)
            .ok_or_else(|| "the repository has no GitHub origin".to_owned())?;
        let merged = worktree::merged_prs(&github::SystemRunner, &repo)?;
        let result = backend.cleanup_worktrees(remote_cleanup_params(
            &repo,
            &merged,
            focused_space,
            skip_teardown == Some(true),
        ))?;
        Ok(remote_cleanup_report(result))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The `worktree.cleanup` request for `repo` and its merged pull requests.
fn remote_cleanup_params(
    repo: &github::Repo,
    merged: &[worktree::MergedPr],
    focused_space: Option<String>,
    skip_teardown: bool,
) -> poimen_client::protocol::WorktreeCleanup {
    poimen_client::protocol::WorktreeCleanup {
        owner: repo.owner.clone(),
        repo: repo.name.clone(),
        merged: merged
            .iter()
            .map(|m| poimen_client::protocol::MergedPull {
                branch: m.branch.clone(),
                number: m.number,
            })
            .collect(),
        focused_space,
        skip_teardown,
    }
}

/// A machine's `worktree.cleanup` result as the report of the local `task_cleanup`.
fn remote_cleanup_report(
    result: poimen_client::protocol::WorktreeCleanupResult,
) -> worktree::CleanupReport {
    worktree::CleanupReport {
        removed: result
            .removed
            .into_iter()
            .map(|r| worktree::Removed {
                path: r.path,
                branch: r.branch,
                pr: r.pr,
                branch_deleted: r.branch_deleted,
                note: r.note,
            })
            .collect(),
        left: result
            .left
            .into_iter()
            .map(|l| worktree::Left {
                path: l.path,
                branch: l.branch,
                pr: l.pr,
                reason: l.reason,
                command: l.command,
            })
            .collect(),
        teardown_untrusted: result.teardown_untrusted,
    }
}

/// The body of `machine_create_space`, against the backend of a machine. `cwd` is a path on that
/// machine, so it is not checked here: an existing space for it is focused and nothing is created;
/// else the server builds the layout. A project file that needs trust is an error (the dialog
/// shows the local file only).
fn remote_create_space(
    backend: &dyn Backend,
    cwd: &str,
    label: &str,
    command: Option<&str>,
) -> Result<spaces::SpaceOutcome, String> {
    let snapshot = backend.snapshot()?;
    let outcome = |space_id: String, created: bool, warnings, setup_started| spaces::SpaceOutcome {
        space_id,
        created,
        warnings,
        setup_started,
        trust: None,
    };
    if let Some(id) = spaces::find_space(cwd, &snapshot) {
        backend.focus_space(&id)?;
        return Ok(outcome(id, false, Vec::new(), false));
    }
    let request = backend::LayoutRequest {
        cwd,
        label,
        decision: None,
        command,
    };
    match backend.create_space_with_layout(request) {
        None => Err("the machine's server cannot build a layout".into()),
        Some(result) => match result? {
            backend::LaidOut::Trust(_) => Err(
                "the project file of this repository is not trusted on the machine; \
                 open the repository there once to trust it"
                    .into(),
            ),
            backend::LaidOut::Created {
                space_id,
                warnings,
                setup_started,
            } => {
                backend.focus_space(&space_id)?;
                Ok(outcome(space_id, true, warnings, setup_started))
            }
        },
    }
}

/// Starts, keeps or stops the machine connections for `configs`; state changes go to the
/// frontend as `machine:state_changed`, the runs of a machine (`run.events`, #692) as `deliver`
/// with its name, every other event as `machine:event`.
fn sync_machines(
    machines: &machines::Machines,
    configs: &[machines::MachineConfig],
    handle: &tauri::AppHandle,
) {
    let runs = handle
        .state::<std::sync::Arc<remote_runs::RemoteRuns>>()
        .inner()
        .clone();
    machines.sync(configs, |config| {
        let handle = handle.clone();
        machines::Machine::start(
            config.clone(),
            std::sync::Arc::new(machines::SshConnector::new(&config.ssh)),
            {
                let handle = handle.clone();
                move |status| {
                    let _ = handle.emit("machine:state_changed", status);
                }
            },
            {
                let handle = handle.clone();
                let machine = config.name.clone();
                let runs = runs.clone();
                move |event| {
                    // Progress events of the machine's runs go to the Deliver panel (#692).
                    if event.kind == "run.events" {
                        if let Some(payload) = runs.apply(&machine, &event.data, deliver::now_secs())
                        {
                            let _ = handle.emit("deliver", payload);
                        }
                        return;
                    }
                    let _ = handle.emit(
                        "machine:event",
                        serde_json::json!({"machine": machine, "kind": event.kind, "data": event.data}),
                    );
                }
            },
        )
    });
    let names: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
    runs.retain(&names);
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

/// Stops the running server and starts the bundled one. Ends every terminal: the frontend calls it
/// only after the user confirmed which terminals end. Serialized with `ensure_backend`.
#[tauri::command]
async fn restart_server(
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
        let status: backend::BackendStatus = b.restart_server().into();
        if poimen && status.state == "started" {
            let _ = colors.push(&poimen_client::Client::default());
        }
        Ok(status)
    })
    .await
}

/// The confirmed "Quit poimen and stop server": stops the server (ending every terminal), then
/// quits the app. When the server cannot be stopped the app stays open and the error is returned.
#[tauri::command]
async fn quit_and_stop_server(
    app: AppHandle,
    backend: State<'_, Shared>,
    lock: State<'_, StartLock>,
) -> Result<(), String> {
    let lock = lock.0.clone();
    on_backend(backend.inner(), move |b| {
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        b.stop_server().map_err(|e| e.to_string())
    })
    .await?;
    app.exit(0);
    Ok(())
}

/// Whether Claude Code and Copilot CLI have the poimen hooks (`poimen-server integration status`).
#[tauri::command]
async fn hooks_status() -> Result<Vec<hooks::HookStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| hooks::status_all(&hooks::SystemExec))
        .await
        .map_err(|e| e.to_string())
}

/// Installs the poimen hooks of one agent (`claude` or `copilot`); only on a user click.
#[tauri::command]
async fn hooks_install(agent: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || hooks::install(&hooks::SystemExec, &agent))
        .await
        .map_err(|e| e.to_string())?
}

/// The running poimen build's version (Tauri app version).
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The release channel of this build (`stable` or `rc`, `POIMEN_CHANNEL` at build time); the
/// frontend shows `RC` in the title and the status bar.
#[tauri::command]
fn app_channel() -> poimen_client::protocol::Channel {
    poimen_client::protocol::CHANNEL
}

/// The native window title: `Poimen`, `Poimen RC` on rc.
fn window_title(channel: poimen_client::protocol::Channel) -> String {
    match channel {
        poimen_client::protocol::Channel::Stable => "Poimen".into(),
        poimen_client::protocol::Channel::Rc => "Poimen RC".into(),
    }
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
        task_start::plan(&resolved.modes, &settings.harnesses, &repo, &issue, &mode)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes the worktrees, spaces and local branches of the repository of `dir` whose pull request
/// is merged (see `worktree::cleanup_merged`). The first task mode with a `cleanup` command replaces the
/// built-in removal. `focused_space` is never touched. Destructive, so only on a user action.
#[tauri::command]
async fn task_cleanup(
    backend: State<'_, Shared>,
    dir: String,
    focused_space: Option<String>,
    skip_teardown: Option<bool>,
) -> Result<worktree::CleanupReport, String> {
    let backend = backend.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = worktree::main_worktree(std::path::Path::new(&dir))?;
        let repo = github::repo_of(&root)
            .ok_or_else(|| "the repository has no GitHub origin".to_owned())?;
        let merged = worktree::merged_prs(&github::SystemRunner, &repo)?;
        let settings = get_settings();
        worktree::cleanup_merged(
            &root,
            &merged,
            &settings.tasks,
            &settings.trusted_projects,
            &worktree::BackendSpaces(backend.as_ref()),
            focused_space.as_deref(),
            skip_teardown == Some(true),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs the `teardown` of the project file of the repository of `dir` in `dir` (the worktree of
/// the focused space), when the project file is trusted. A user action (palette entry).
#[tauri::command]
async fn project_teardown(dir: String) -> Result<project::TeardownReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = worktree::main_worktree(std::path::Path::new(&dir))?;
        let sources = trust::Sources::read(&root.to_string_lossy());
        let plan = project::teardown_plan(&sources, &get_settings().trusted_projects);
        let top = worktree::toplevel(std::path::Path::new(&dir))?;
        project::teardown_now(&project::SystemRunner, &plan, &top)
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
    space_id: String,
    dir: String,
    name: String,
    command: String,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        task_start::start_tab(b, &space_id, &dir, &name, &command)
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

/// The agent actions for the repository of `root`: defaults, the user's `github.agentActions` and
/// the trusted project file's, each entry with its source; without `root` only the first two.
#[tauri::command]
async fn get_agent_actions(root: Option<String>) -> agent_actions::Resolved {
    tauri::async_runtime::spawn_blocking(move || resolved_agent_actions(root))
        .await
        .unwrap_or_default()
}

fn resolved_agent_actions(root: Option<String>) -> agent_actions::Resolved {
    let settings = get_settings();
    let Some(dir) = root else {
        return agent_actions::Resolved {
            actions: agent_actions::resolve(&settings.agent_actions, &[]),
            note: None,
        };
    };
    let root = git::toplevel(&dir).unwrap_or(dir);
    let sources = trust::Sources::read(&root);
    agent_actions::for_project(
        &settings.agent_actions,
        &settings.harnesses,
        &sources,
        &settings.trusted_projects,
    )
}

/// The programs to start for the action `id` on `items` in the repository of `root`; `harness` and
/// `model` replace the action's own (the dialog of Shift+click).
#[tauri::command]
async fn agent_action_plan(
    root: String,
    id: String,
    repo: String,
    items: Vec<agent_actions::Item>,
    harness: Option<String>,
    model: Option<String>,
    suffix: Option<String>,
) -> Result<Vec<agent_actions::Launch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = resolved_agent_actions(Some(root));
        let action = resolved
            .actions
            .iter()
            .find(|a| a.spec.id == id)
            .ok_or_else(|| format!("unknown agent action {id:?}"))?;
        agent_actions::plan(
            &action.spec,
            &get_settings().harnesses,
            &repo,
            &items,
            harness.as_deref(),
            model.as_deref(),
            suffix.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Writes only `terminal.fontFamily`; the settings watcher emits the change.
#[tauri::command]
fn set_terminal_font_family(family: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_terminal_font_family(&home, &family)
}

/// Writes only `terminal.fontSize`; the settings watcher emits the change.
#[tauri::command]
fn set_terminal_font_size(size: f64) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_terminal_font_size(&home, size)
}

/// Removes only `terminal.fontSize` (back to the default); the settings watcher emits the change.
#[tauri::command]
fn clear_terminal_font_size() -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::clear_terminal_font_size(&home)
}

/// Writes only `ui.fontFamily`; the settings watcher emits the change.
#[tauri::command]
fn set_ui_font_family(family: String) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_ui_font_family(&home, &family)
}

/// Writes only `ui.fontSize`; the settings watcher emits the change.
#[tauri::command]
fn set_ui_font_size(size: f64) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    settings::save_ui_font_size(&home, size)
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

/// The default pane layout of a new space (the first-start seeding for the installed agents),
/// shown by the settings view as the "Default" layout.
#[tauri::command]
fn default_spaces_layout() -> layout::Layout {
    let path = gui_path::child_path();
    templates::seeded(|p| gui_path::resolve_program(p, &path).is_some()).0
}

/// Writes the first-start defaults (`spaces.layout` and the default space template) when the
/// config has neither key; `true` when it wrote them. Called once by the app (the UI state
/// remembers that it ran).
#[tauri::command]
fn seed_space_defaults() -> Result<bool, String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    let path = gui_path::child_path();
    let (layout, templates) =
        templates::seed_values(|p| gui_path::resolve_program(p, &path).is_some());
    settings::seed_defaults(&home, &layout, &templates)
}

/// The changed files of the worktree `dir` against its base or `HEAD` (read-only git).
#[tauri::command]
async fn changes_list(dir: String, scope: changes::Scope) -> Result<changes::ChangeList, String> {
    tauri::async_runtime::spawn_blocking(move || changes::list(&dir, scope, home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Old and new content of one changed file for the merge view (read-only git).
#[tauri::command]
async fn changes_file(
    dir: String,
    scope: changes::Scope,
    path: String,
) -> Result<changes::FileContent, String> {
    tauri::async_runtime::spawn_blocking(move || {
        changes::file(&dir, scope, &path, home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Writes review comments to a new `.agents/state/comments-<timestamp>.md` of the worktree `dir`
/// (excluded from git) and returns its absolute path.
#[tauri::command]
async fn changes_write_comments(dir: String, content: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        changes::write_comments(&dir, &content, home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The review findings of the worktree `dir` (`.agents/state/findings.json`, read only).
#[tauri::command]
async fn changes_findings(dir: String) -> Result<changes::FindingsResult, String> {
    tauri::async_runtime::spawn_blocking(move || changes::findings(&dir, home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Stages (`stage: true`) or unstages the repository-relative `paths` of the worktree `dir`.
#[tauri::command]
async fn changes_stage(dir: String, paths: Vec<String>, stage: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        changes_git::stage(&dir, &paths, stage, home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Commits the staged changes of the worktree `dir` with `message`.
#[tauri::command]
async fn changes_commit(dir: String, message: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        changes_git::commit(&dir, &message, home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pushes the current branch of the worktree `dir` (never forced, refused on protected branches).
#[tauri::command]
async fn changes_push(dir: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || changes_git::push(&dir, home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// `git fetch --prune` of the repository containing `dir`.
#[tauri::command]
async fn git_fetch(dir: String) -> Result<git_sync::Outcome, String> {
    tauri::async_runtime::spawn_blocking(move || git_sync::fetch(&dir, home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// `git pull --ff-only` of the repository containing `dir`; refusals come back as the error.
#[tauri::command]
async fn git_pull(dir: String) -> Result<git_sync::Outcome, String> {
    tauri::async_runtime::spawn_blocking(move || git_sync::pull(&dir, home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Fetches every repository below `dir`; `git-sync-progress` events carry `{ done, total, path }`.
#[tauri::command]
async fn git_fetch_all(app: AppHandle, dir: String) -> Result<git_sync::Summary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_sync::fetch_all(&dir, home_dir().as_deref(), |done, total, path| {
            let _ = app.emit(
                "git-sync-progress",
                json!({ "done": done, "total": total, "path": path }),
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Branch and worktree flag per directory (read-only `git rev-parse`).
#[tauri::command]
async fn repo_info(dirs: Vec<String>) -> Result<HashMap<String, git::RepoInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || git::infos(dirs))
        .await
        .map_err(|e| e.to_string())
}

/// Starts following the subagent transcripts of the Claude Code or GitHub Copilot CLI agent in
/// `terminal_id` (replacing the previous watch); updates arrive as `subagents` events. `false` when the
/// pane has no such session or its transcript is not found; nothing is watched then. Read-only.
#[tauri::command]
async fn subagents_watch(
    app: AppHandle,
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    watch: State<'_, SubagentWatch>,
    key: String,
    terminal_id: String,
) -> Result<bool, String> {
    let generation = watch.0.lock().map_err(|e| e.to_string())?.begin(&key);
    // The transcript files of a machine's agent are on that machine: its server reads them
    // (#698). A remote pane is never looked up in the local `~/.claude` or `~/.copilot` (#693).
    if machines::split_id(&terminal_id).0.is_some() {
        let routed = machines.route_connected(backend.inner(), &terminal_id)?;
        let (follower, key_for_poll) = (key.clone(), key.clone());
        let started = tauri::async_runtime::spawn_blocking(move || {
            remote_transcripts::start(
                routed.backend,
                &routed.id,
                &key_for_poll,
                remote_transcripts::INTERVAL,
                move |session, update| {
                    let event = json!({ "key": follower, "session": session, "update": update });
                    let _ = app.emit("subagents", event);
                },
            )
        })
        .await
        .map_err(|e| e.to_string())??;
        let handle = match started {
            remote_transcripts::Started::Following(poller) => {
                Some(Box::new(poller) as FollowHandle)
            }
            remote_transcripts::Started::NotYet => None,
        };
        let following = handle.is_some();
        let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
        return Ok(watches.store(&key, generation, handle) && following);
    }
    let session = on_routed(backend.inner(), machines.inner(), &terminal_id, |b, id| {
        b.agent_session(id)
    })
    .await?;
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
        herdr::TranscriptAgent::Claude => {
            Box::new(transcripts::watch(&dir, &session, emit)?) as FollowHandle
        }
        herdr::TranscriptAgent::Copilot => Box::new(copilot::watch(&dir, emit)?) as FollowHandle,
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

/// Watches the deliver runs (progress events in `<git-common-dir>/.agents/events`, else the
/// status file `.agents/deliver-loop/status.json`) of the repositories of `dirs` (one watch per
/// repository: its worktrees count as one, named by the toplevel of the first), replacing the
/// watched set; each change arrives as a `deliver` event `{ root, read }`, first with the current
/// content. Returns the watched roots. Read-only.
#[tauri::command]
async fn deliver_watch(
    app: AppHandle,
    watch: State<'_, DeliverWatch>,
    dirs: Vec<String>,
) -> Result<Vec<String>, String> {
    let roots = tauri::async_runtime::spawn_blocking(move || git::repo_roots(&dirs))
        .await
        .map_err(|e| e.to_string())?;
    let mut watches = watch.0.lock().map_err(|e| e.to_string())?;
    let current = watches.keys().cloned().collect();
    let (stop, start) = deliver::plan(&current, &roots);
    for root in stop {
        watches.remove(&root);
    }
    for root in start {
        let (app, name) = (app.clone(), root.clone());
        let handle = deliver::watch(PathBuf::from(&root), move |read| {
            let _ = app.emit("deliver", json!({ "root": name, "read": read }));
        });
        watches.insert(root, handle);
    }
    Ok(roots)
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
#[allow(clippy::too_many_arguments)]
fn open_terminal(
    backend: State<'_, Shared>,
    mode: Mode,
    target: String,
    cols: u16,
    rows: u16,
    takeover: bool,
    machine: Option<String>,
    on_record: Channel<InvokeResponseBody>,
    machines: State<'_, SharedMachines>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
) -> Result<backend::OpenedStream, String> {
    // A terminal of a machine opens over that machine's backend (never started by poimen).
    let backend: Shared = match &machine {
        Some(name) => machines.backend(name)?,
        None => backend.inner().clone(),
    };
    let request = StreamRequest {
        mode,
        target: &target,
        cols,
        rows,
        takeover,
    };
    let emulated = backend.emulated();
    let opened = backend.open_stream(
        request,
        Box::new(move |_, record| {
            for message in channel_messages(&record, emulated) {
                let _ = on_record.send(message);
            }
        }),
    )?;
    if machine.is_none() {
        return Ok(opened);
    }
    Ok(backend::OpenedStream {
        id: remote.add(backend, opened.id),
        ..opened
    })
}

/// The backend and stream id a stream command works on: a machine's stream by its mapped id, else
/// the local backend.
fn stream_of(backend: &Shared, remote: &machines::RemoteStreams, id: u32) -> (Shared, u32) {
    if id >= machines::REMOTE_STREAM_BASE
        && let Some(found) = remote.get(id)
    {
        return found;
    }
    (backend.clone(), id)
}

#[tauri::command]
fn terminal_input(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    text: String,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_input(id, &text)
}

#[tauri::command]
fn terminal_resize(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_resize(id, cols, rows)
}

#[tauri::command]
fn terminal_scroll(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    up: bool,
    lines: u16,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_scroll(id, up, lines)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn terminal_mouse(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    action: String,
    button: String,
    column: u16,
    row: u16,
    modifiers: u8,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_mouse(id, &action, &button, column, row, modifiers)
}

#[tauri::command]
fn close_terminal(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
) {
    let (backend, id) = if id >= machines::REMOTE_STREAM_BASE {
        remote
            .remove(id)
            .unwrap_or_else(|| (backend.inner().clone(), id))
    } else {
        (backend.inner().clone(), id)
    };
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
    let edit = Submenu::with_items(
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
    )?;
    // Cmd chords reach the app through the menu on macOS; elsewhere the terminal view handles
    // the key itself.
    if cfg!(target_os = "macos") {
        edit.append(&sep()?)?;
        edit.append(&item("find", "Find in Terminal")?)?;
    }
    menu.append(&edit)?;
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

/// The backend named by the `backend` setting (anything but `herdr` is poimen) and its name.
fn make_backend(kind: &str) -> (Shared, &'static str) {
    if kind == "herdr" {
        let client = herdr::Client::from_env();
        let backend = backend::HerdrBackend::new(
            backend::SystemRunner::new(client.clone()),
            client,
            stream::resolve_bin_from_env(),
        );
        return (std::sync::Arc::new(backend), "herdr");
    }
    let backend = poimen_backend::PoimenBackend::new(poimen_client::Client::default());
    (std::sync::Arc::new(backend), "poimen")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let kind = home_dir()
        .map(|h| settings::load(&h).backend)
        .unwrap_or_else(|| "poimen".into());
    let (backend, kind) = make_backend(&kind);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init());
    // An rc build never updates itself (no updater artifacts, no endpoint in tauri.rc.conf.json).
    let builder = if updater::enabled(poimen_client::protocol::CHANNEL) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    builder
        .manage(updater::PendingUpdate::default())
        .manage(backend.clone())
        .manage(BackendKind(kind))
        .manage::<GithubClient>(std::sync::Arc::new(github::Client::system(
            home_dir()
                .map(|h| {
                    poimen_core::settings::config_dir(&h)
                        .join("cache")
                        .join("github")
                })
                .unwrap_or_else(|| {
                    std::env::temp_dir()
                        .join(poimen_client::protocol::CHANNEL.named("poimen-github"))
                }),
        )))
        .manage(SubagentWatch::default())
        .manage(DeliverWatch::default())
        .manage(StartLock::default())
        .manage(terminal_colors::TerminalColors::default())
        .manage(SharedMachines::default())
        .manage(std::sync::Arc::new(machines::RemoteStreams::default()))
        .manage(std::sync::Arc::new(remote_runs::RemoteRuns::default()))
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&window_title(poimen_client::protocol::CHANNEL));
            }
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
                let machines = app.state::<SharedMachines>().inner().clone();
                sync_machines(&machines, &settings::load(&home).machines, &handle);
                std::thread::spawn(move || {
                    settings::watch(&home, std::time::Duration::from_secs(1), |s| {
                        let _ = handle.emit("settings", s);
                        sync_machines(&machines, &s.machines, &handle);
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
            restart_server,
            quit_and_stop_server,
            backend_kind,
            list_machines,
            reconnect_machine,
            machine_snapshot,
            machine_repo_ensure,
            machine_worktree_create,
            machine_create_space,
            machine_task_tab,
            machine_task_cleanup,
            machine_check,
            set_terminal_colors,
            hooks_status,
            hooks_install,
            set_herdr_auto_start,
            set_terminal_font_family,
            set_terminal_font_size,
            clear_terminal_font_size,
            set_ui_font_family,
            set_ui_font_size,
            pane_split,
            tab_resize_split,
            tab_focus,
            pane_close,
            pane_restart,
            tab_close,
            tab_rename,
            space_close,
            agent_start,
            agent_rename,
            agent_prompt,
            pane_rename,
            space_rename,
            terminal_create,
            github_pulls,
            github_pull_detail,
            branch_pulls,
            github_issues,
            github_issue_detail,
            github_milestones,
            task_issues,
            github_repo,
            create_space,
            rebuild_space,
            list_dirs,
            list_child_dirs,
            app_version,
            app_channel,
            updater::update_check,
            updater::update_install,
            get_settings,
            get_harnesses,
            get_agent_actions,
            agent_action_plan,
            get_task_modes,
            task_worktree,
            changes_list,
            changes_file,
            changes_write_comments,
            changes_findings,
            changes_stage,
            changes_commit,
            changes_push,
            git_fetch,
            git_pull,
            git_fetch_all,
            pr_worktree,
            worktree_branches,
            task_plan,
            task_tab,
            task_cleanup,
            project_teardown,
            get_color_schemes,
            set_theme,
            set_color_schema,
            set_sidebar_layout,
            set_explorer_roots,
            set_explorer_show_hidden,
            set_spaces_layout,
            set_space_templates,
            resolve_space_template,
            revoke_project,
            remove_spaces_layout,
            default_spaces_layout,
            seed_space_defaults,
            project_config,
            project_file,
            project_save,
            project_entries,
            project_run,
            repo_info,
            subagents_watch,
            subagents_unwatch,
            deliver_watch,
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
    use serde_json::{Value, json};

    /// A snapshot from its neutral wire shape (missing fields take their defaults).
    fn snapshot_of(wire: Value) -> Snapshot {
        serde_json::from_value(wire).unwrap()
    }

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
            None,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        fake.calls()
    }

    fn typed(calls: &[String]) -> Vec<&String> {
        calls.iter().filter(|c| c.starts_with("input ")).collect()
    }

    #[test]
    fn missing_root_pane_warning_names_no_backend() {
        let fake = FakeBackend::default();
        let warnings = build_layout(
            &fake,
            "/work/app",
            None,
            &sources(PROJECT_TEXT),
            None,
            false,
            None,
            None,
        );
        assert_eq!(
            warnings,
            ["the backend did not report the root pane; no layout applied"]
        );
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
        assert_eq!(outcome.space_id, "w9");
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
        *fake.snapshot.lock().unwrap() = snapshot_of(json!({
            "spaces": [{ "space_id": "w2", "label": "demo", "number": 1 }],
            "terminals": [{ "terminal_id": "w2:p1", "space_id": "w2", "cwd": dir }],
        }));
        let outcome = create_space_with(&fake, None, &dir, None, None, Some("claude 'x'")).unwrap();
        assert!(!outcome.created);
        assert_eq!(outcome.space_id, "w2");
        // Focused only: no layout, no command.
        assert_eq!(fake.calls(), ["snapshot", "focus w2"]);
    }

    #[test]
    fn create_space_focuses_a_task_space_without_terminals_on_the_poimen_backend() {
        let fake = FakeBackend::default();
        let dir = tmp_dir();
        let space = poimen_client::protocol::Space {
            id: "s3".into(),
            name: "#12 Add picker".into(),
            cwd: dir.clone(),
            ..Default::default()
        };
        *fake.snapshot.lock().unwrap() = poimen_backend::map_snapshot(
            &poimen_client::protocol::SpaceList {
                spaces: vec![space],
                tabs: Vec::new(),
            },
            &[],
        );
        let outcome =
            create_space_with(&fake, None, &dir, Some("#12 Add picker"), None, Some("c")).unwrap();
        assert!(!outcome.created);
        assert_eq!(outcome.space_id, "s3");
        assert_eq!(fake.calls(), ["snapshot", "focus s3"]);
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

    fn home_with_templates(config: &str) -> (PathBuf, String) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!("poimen-tpl-{}-{n}", std::process::id()));
        std::fs::create_dir_all(home.join(".config/poimen")).unwrap();
        std::fs::write(home.join(".config/poimen/config.json"), config).unwrap();
        let dir = home.join("src/app");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.to_string_lossy().into_owned();
        (home, dir)
    }

    #[test]
    fn resolve_space_template_names_the_template_or_nothing() {
        let (home, dir) = home_with_templates(
            r#"{"spaceTemplates":[{"name":"app","match":{"dir":"~/src"},"agent":"claude"}]}"#,
        );
        assert_eq!(
            resolve_space_template_with(Some(&home), &dir),
            Some(TemplateHint {
                name: "app".into(),
                agent: "claude".into(),
                default: false,
            })
        );
        let outside = home.to_string_lossy().into_owned();
        assert_eq!(resolve_space_template_with(Some(&home), &outside), None);
        assert_eq!(
            resolve_space_template_with(Some(&home), "/no/such/dir"),
            None
        );
        assert_eq!(resolve_space_template_with(None, &dir), None);
    }

    #[test]
    fn the_default_template_is_marked_as_the_default_in_the_hint() {
        let (home, dir) =
            home_with_templates(r#"{"spaceTemplates":[{"name":"default","agent":"copilot"}]}"#);
        assert_eq!(
            resolve_space_template_with(Some(&home), &dir),
            Some(TemplateHint {
                name: "default".into(),
                agent: "copilot".into(),
                default: true,
            })
        );
    }

    #[test]
    fn create_space_starts_the_template_agent_in_the_root_pane_and_uses_its_layout() {
        let (home, dir) = home_with_templates(
            r#"{"harnesses":{"sh1":{"command":["sh"],"modelArgs":["-m","{model}"]}},
            "spaceTemplates":[{"name":"app","match":{"dir":"~/src"},"agent":"sh1","model":"x",
              "layout":{"panes":[{"name":"main"},
                {"name":"b","split":{"from":"main","direction":"down","ratio":0.5}}]}}]}"#,
        );
        let fake = FakeBackend::default();
        let out = create_space_with(&fake, Some(&home), &dir, None, None, None).unwrap();
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let calls = fake.calls();
        assert!(calls.contains(&"rename w9:root main".to_owned()));
        assert!(
            calls.contains(&"input w9:root sh -m x".to_owned()),
            "{calls:?}"
        );
        assert_eq!(calls.iter().filter(|c| c.starts_with("split ")).count(), 1);
    }

    #[test]
    fn a_task_command_beats_the_template_agent() {
        let (home, dir) = home_with_templates(
            r#"{"harnesses":{"sh1":{"command":["sh"]}},
            "spaceTemplates":[{"name":"app","match":{"dir":"~/src"},"agent":"sh1"}]}"#,
        );
        let fake = FakeBackend::default();
        create_space_with(&fake, Some(&home), &dir, None, None, Some("task")).unwrap();
        let calls = fake.calls();
        assert!(calls.contains(&"input w9:root task".to_owned()));
        assert!(!calls.iter().any(|c| c == "input w9:root sh"));
    }

    #[test]
    fn an_unknown_or_missing_agent_warns_with_the_template_name() {
        for agent in ["nope", "nosuchprogram-xyz"] {
            let (home, dir) = home_with_templates(&format!(
                r#"{{"harnesses":{{"gone":{{"command":["nosuchprogram-xyz"]}}}},
                "spaceTemplates":[{{"name":"app","match":{{"dir":"~/src"}},"agent":"{}"}}]}}"#,
                if agent == "nope" { "nope" } else { "gone" }
            ));
            let fake = FakeBackend::default();
            let out = create_space_with(&fake, Some(&home), &dir, None, None, None).unwrap();
            assert!(out.created);
            assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
            assert!(out.warnings[0].contains("\"app\""));
            assert!(
                !fake
                    .calls()
                    .iter()
                    .any(|c| c.starts_with("input w9:root") && c != "input w9:root claude")
            );
        }
    }

    #[test]
    fn build_layout_fills_a_free_root_pane_but_not_an_explicit_command() {
        let free = layout::parse_layout(r#"{"panes":[{"name":"a"}]}"#).unwrap();
        let own = layout::parse_layout(r#"{"panes":[{"name":"a","command":"vim"}]}"#).unwrap();
        let no_files = trust::Sources {
            root: "/w".into(),
            project: None,
            legacy: None,
        };
        let typed = |user: Option<&layout::Layout>| {
            let fake = FakeBackend::default();
            build_layout(
                &fake,
                "/w",
                Some("w1:root"),
                &no_files,
                user,
                true,
                None,
                Some("agent"),
            );
            fake.calls()
                .into_iter()
                .filter(|c| c.starts_with("input "))
                .collect::<Vec<_>>()
        };
        assert_eq!(typed(Some(&free)), ["input w1:root agent"]);
        assert_eq!(typed(Some(&own)), ["input w1:root vim"]);
        // The built-in default's `claude` is replaced by the template agent.
        assert_eq!(typed(None), ["input w1:root agent"]);
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

    // Clean up merged tasks on a machine (#284).

    #[test]
    fn remote_cleanup_sends_the_local_merged_pulls_and_maps_the_report_back() {
        let repo = github::Repo {
            owner: "o".into(),
            name: "app".into(),
        };
        let merged = [worktree::MergedPr {
            branch: "feat/7-x".into(),
            number: 70,
        }];
        let p = remote_cleanup_params(&repo, &merged, Some("s1".into()), true);
        assert_eq!((p.owner.as_str(), p.repo.as_str()), ("o", "app"));
        assert_eq!(p.merged[0].branch, "feat/7-x");
        assert_eq!(p.merged[0].number, 70);
        assert_eq!(p.focused_space.as_deref(), Some("s1"));
        assert!(p.skip_teardown);

        let result: poimen_client::protocol::WorktreeCleanupResult =
            serde_json::from_value(serde_json::json!({
                "path": "/srv/o/app",
                "removed": [{"path": "/srv/o/app-7", "branch": "feat/7-x", "pr": 70,
                             "branch_deleted": false, "note": "branch kept: protected"}],
                "left": [{"path": "/srv/o/app-8", "branch": "feat/8-y", "pr": 80,
                          "reason": "it has uncommitted or untracked changes", "command": "c"}],
                "teardown_untrusted": false
            }))
            .unwrap();
        let report = serde_json::to_value(remote_cleanup_report(result)).unwrap();
        assert_eq!(
            report,
            serde_json::json!({
                "removed": [{"path": "/srv/o/app-7", "branch": "feat/7-x", "pr": 70,
                             "branchDeleted": false, "note": "branch kept: protected"}],
                "left": [{"path": "/srv/o/app-8", "branch": "feat/8-y", "pr": 80,
                          "reason": "it has uncommitted or untracked changes", "command": "c"}],
                "teardownUntrusted": false
            })
        );
    }

    // Spaces on a machine (#283).

    #[test]
    fn remote_create_space_builds_the_layout_without_checking_the_local_disk() {
        let fake = layout_backend(built("s1"));
        let out = remote_create_space(&fake, "/srv/wt/app-12", "#12 x", Some("claude")).unwrap();
        assert!(out.created);
        assert_eq!(out.space_id, "s1");
        let calls = fake.calls();
        assert!(
            calls.contains(&"create_space_with_layout /srv/wt/app-12 #12 x - claude".to_string()),
            "{calls:?}"
        );
    }

    #[test]
    fn remote_create_space_focuses_an_existing_space_and_creates_nothing() {
        let fake = layout_backend(built("s1"));
        *fake.snapshot.lock().unwrap() = snapshot_of(json!({
            "spaces": [{"space_id": "s7", "label": "x", "cwd": "/srv/wt/app-12"}],
            "terminals": [],
        }));
        let out = remote_create_space(&fake, "/srv/wt/app-12", "x", None).unwrap();
        assert!(!out.created);
        assert_eq!(out.space_id, "s7");
        assert!(!fake.calls().iter().any(|c| c.starts_with("create_space")));
    }

    #[test]
    fn remote_create_space_refuses_an_untrusted_project_and_a_failing_server() {
        let ask = backend::LaidOut::Trust(trust::TrustRequest {
            root: "/r".into(),
            files: Vec::new(),
            hash: "h".into(),
            commands: Vec::new(),
        });
        let err = remote_create_space(&layout_backend(ask), "/r", "l", None).unwrap_err();
        assert!(err.contains("not trusted"), "{err}");
        let fake = layout_backend(built("s1"));
        fake.fail_on("create_space_with_layout");
        assert_eq!(
            remote_create_space(&fake, "/r", "l", None),
            Err("boom".into())
        );
    }

    // A backend that builds the layout itself (poimen-server's `space.create`).

    fn layout_backend(answer: backend::LaidOut) -> FakeBackend {
        let fake = FakeBackend::default();
        *fake.laid_out.lock().unwrap() = Some(answer);
        fake
    }

    fn built(space_id: &str) -> backend::LaidOut {
        backend::LaidOut::Created {
            space_id: space_id.into(),
            warnings: vec!["layout: pane x could not be split".into()],
            setup_started: true,
        }
    }

    /// A directory with a project file whose `setup` is not trusted by anyone.
    fn untrusted_project() -> String {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("poimen-513-{}-{n}", std::process::id()));
        std::fs::create_dir_all(dir.join(".poimen")).unwrap();
        std::fs::write(dir.join(".poimen/config.json"), r#"{"setup": "make"}"#).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn a_layout_backend_builds_the_new_space_and_the_app_only_focuses_it() {
        let fake = layout_backend(built("s9"));
        let dir = tmp_dir();
        let outcome = create_space_with(&fake, None, &dir, Some("demo"), None, None).unwrap();
        assert_eq!(
            outcome,
            spaces::SpaceOutcome {
                space_id: "s9".into(),
                created: true,
                warnings: vec!["layout: pane x could not be split".into()],
                setup_started: true,
                trust: None,
            }
        );
        // No terminal, split, rename or input from the app: `layout::apply` is herdr's only.
        assert_eq!(
            fake.calls(),
            [
                "snapshot".to_owned(),
                format!("create_space_with_layout {dir} demo - -"),
                "focus s9".to_owned(),
            ]
        );
    }

    #[test]
    fn a_layout_backend_gets_the_task_command_and_the_trust_answer() {
        let fake = layout_backend(built("s9"));
        let dir = untrusted_project();
        let decision = trust::Decision::Trust { hash: "abc".into() };
        create_space_with(
            &fake,
            None,
            &dir,
            Some("#12 x"),
            Some(decision),
            Some("claude 'x'"),
        )
        .unwrap();
        assert_eq!(
            fake.calls()[1],
            format!("create_space_with_layout {dir} #12 x trust:abc claude 'x'")
        );
        let fake = layout_backend(built("s9"));
        create_space_with(
            &fake,
            None,
            &dir,
            None,
            Some(trust::Decision::Without),
            None,
        )
        .unwrap();
        assert!(
            fake.calls()[1].ends_with(" without -"),
            "{:?}",
            fake.calls()
        );
    }

    #[test]
    fn the_trust_question_of_a_layout_backend_reaches_the_dialog_and_nothing_is_focused() {
        let request = trust::TrustRequest {
            root: "/work/app".into(),
            files: vec!["/work/app/.poimen/config.json".into()],
            hash: "abc".into(),
            commands: vec![trust::ListedCommand {
                what: "setup".into(),
                command: "make".into(),
            }],
        };
        let fake = layout_backend(backend::LaidOut::Trust(request.clone()));
        let dir = tmp_dir();
        let outcome = create_space_with(&fake, None, &dir, None, None, None).unwrap();
        assert!(!outcome.created);
        assert_eq!(outcome.space_id, "");
        assert_eq!(outcome.trust, Some(request));
        assert_eq!(fake.calls().len(), 2, "no focus: {:?}", fake.calls());
    }

    #[test]
    fn the_app_does_not_gate_the_project_file_of_a_layout_backend() {
        // On herdr this directory asks for trust first; on poimen-server the server asks.
        let dir = untrusted_project();
        let herdr = FakeBackend::default();
        let asked = create_space_with(&herdr, None, &dir, None, None, None).unwrap();
        assert!(asked.trust.is_some());
        let fake = layout_backend(built("s9"));
        let outcome = create_space_with(&fake, None, &dir, None, None, None).unwrap();
        assert!(outcome.created && outcome.trust.is_none());
    }

    #[test]
    fn an_existing_space_is_focused_without_asking_a_layout_backend() {
        let fake = layout_backend(built("s9"));
        let dir = tmp_dir();
        *fake.snapshot.lock().unwrap() = snapshot_of(json!({
            "spaces": [{ "space_id": "s2", "label": "demo", "number": 1 }],
            "terminals": [{ "terminal_id": "t1", "space_id": "s2", "cwd": dir }],
        }));
        let outcome = create_space_with(&fake, None, &dir, None, None, Some("c")).unwrap();
        assert!(!outcome.created);
        assert_eq!(fake.calls(), ["snapshot", "focus s2"]);
    }

    #[test]
    fn a_failing_layout_create_is_an_error_and_focuses_nothing() {
        let fake = layout_backend(built("s9"));
        fake.fail_on("create_space_with_layout");
        let err = create_space_with(&fake, None, &tmp_dir(), None, None, None).unwrap_err();
        assert_eq!(err, "boom");
        assert_eq!(fake.calls(), ["snapshot"]);
    }

    // Rebuilding a space that came back without terminals after a server restart (#579).

    /// A fake whose snapshot has the persisted space `s1` in `cwd` and no terminals, as the
    /// poimen backend reports it right after a restart.
    fn restarted(cwd: &str) -> FakeBackend {
        let fake = FakeBackend::default();
        let space = poimen_client::protocol::Space {
            id: "s1".into(),
            name: "app".into(),
            cwd: cwd.into(),
            ..Default::default()
        };
        *fake.snapshot.lock().unwrap() = poimen_backend::map_snapshot(
            &poimen_client::protocol::SpaceList {
                spaces: vec![space],
                tabs: Vec::new(),
            },
            &[],
        );
        fake
    }

    const SHELLS: &str = r#"{"spaces":{"layout":{"panes":[{"name":"main"},
        {"name":"logs","split":{"from":"main","direction":"down","ratio":0.5}}]}}}"#;

    #[test]
    fn an_empty_space_without_agents_is_rebuilt_in_its_directory() {
        let (home, dir) = home_with_templates(SHELLS);
        let fake = restarted(&dir);
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Rebuilt {
                warnings: vec![],
                setup_started: false
            }
        );
        assert_eq!(
            fake.calls(),
            [
                "snapshot".to_owned(),
                format!("create_terminal s1 {dir}"),
                "rename s1:t1 main".to_owned(),
                format!("split s1:t1 down {dir} 0.5"),
                "rename w1:p13 logs".to_owned(),
            ]
        );
    }

    #[test]
    fn an_agent_pane_needs_consent_and_nothing_is_created() {
        // No user config: the built-in default with `claude` in the root pane.
        let fake = restarted(&tmp_dir());
        let out = rebuild_space_with(&fake, None, "s1", None, false).unwrap();
        let RebuildOutcome::Consent { reasons } = out else {
            panic!("expected consent, got {out:?}");
        };
        assert_eq!(reasons, ["pane \"agent\" starts an agent: claude"]);
        assert_eq!(fake.calls(), ["snapshot"]);
    }

    #[test]
    fn a_confirmed_rebuild_starts_the_agent_like_create_space() {
        let dir = tmp_dir();
        let fake = restarted(&dir);
        let out = rebuild_space_with(&fake, None, "s1", None, true).unwrap();
        assert!(matches!(out, RebuildOutcome::Rebuilt { .. }), "{out:?}");
        let calls = fake.calls();
        assert_eq!(calls[1], format!("create_terminal s1 {dir}"));
        assert!(
            calls.contains(&"input s1:t1 claude".to_owned()),
            "{calls:?}"
        );
        assert_eq!(calls.iter().filter(|c| c.starts_with("split ")).count(), 2);
    }

    #[test]
    fn a_template_agent_needs_consent() {
        let (home, dir) = home_with_templates(
            r#"{"harnesses":{"sh1":{"command":["sh"]}},
            "spaceTemplates":[{"name":"app","match":{"dir":"~/src"},"agent":"sh1",
              "layout":{"panes":[{"name":"main"}]}}]}"#,
        );
        let fake = restarted(&dir);
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert!(matches!(out, RebuildOutcome::Consent { .. }), "{out:?}");
        assert_eq!(fake.calls(), ["snapshot"]);
    }

    #[test]
    fn a_space_with_terminals_is_left_alone() {
        let dir = tmp_dir();
        let fake = FakeBackend::default();
        *fake.snapshot.lock().unwrap() = snapshot_of(json!({
            "spaces": [{ "space_id": "s1", "label": "app", "number": 1, "cwd": dir }],
            "terminals": [{ "terminal_id": "t1", "space_id": "s1", "cwd": dir }],
        }));
        let out = rebuild_space_with(&fake, None, "s1", None, true).unwrap();
        assert_eq!(out, RebuildOutcome::NotEmpty);
        assert_eq!(fake.calls(), ["snapshot"]);
        // A space that is gone is nothing to do either.
        let out = rebuild_space_with(&fake, None, "s9", None, true).unwrap();
        assert_eq!(out, RebuildOutcome::NotEmpty);
    }

    #[test]
    fn a_space_whose_directory_is_gone_stays_empty_with_a_hint() {
        let fake = restarted("/no/such/dir/at/all");
        let out = rebuild_space_with(&fake, None, "s1", None, true).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Missing {
                reason: "the directory /no/such/dir/at/all no longer exists".into()
            }
        );
        let fake = restarted("");
        let out = rebuild_space_with(&fake, None, "s1", None, true).unwrap();
        assert!(matches!(out, RebuildOutcome::Missing { .. }));
        assert_eq!(fake.calls(), ["snapshot"]);
    }

    #[test]
    fn an_untrusted_project_file_asks_through_the_trust_gate() {
        let (home, dir) = home_with_templates(SHELLS);
        let poimen_dir = std::path::Path::new(&dir).join(".poimen");
        std::fs::create_dir_all(&poimen_dir).unwrap();
        std::fs::write(
            poimen_dir.join("config.json"),
            r#"{"layout":{"panes":[{"name":"dev","command":"pnpm dev"}]},"setup":"pnpm i"}"#,
        )
        .unwrap();
        let fake = restarted(&dir);
        // Automatic: consent needed, nothing created.
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Consent {
                reasons: vec!["the project file is not trusted yet".into()]
            }
        );
        // Confirmed without an answer: the trust request, still nothing created.
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, true).unwrap();
        assert!(matches!(out, RebuildOutcome::Trust { .. }), "{out:?}");
        assert_eq!(fake.calls(), ["snapshot", "snapshot"]);
        // "Run without project commands": the panes, but neither the command nor the setup.
        let decision = Some(trust::Decision::Without);
        let out = rebuild_space_with(&fake, Some(&home), "s1", decision, true).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Rebuilt {
                warnings: vec![],
                setup_started: false
            }
        );
        assert!(!fake.calls().iter().any(|c| c.starts_with("input ")));
        // Trusted: the setup needs consent, unconfirmed nothing more is created.
        let RebuildOutcome::Trust { request } =
            rebuild_space_with(&fake, Some(&home), "s1", None, true).unwrap()
        else {
            panic!("expected the trust request");
        };
        settings::trust_project(&home, &request.root, &request.hash).unwrap();
        let before = fake.calls().len();
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Consent {
                reasons: vec!["setup runs: pnpm i".into()]
            }
        );
        assert_eq!(fake.calls().len(), before + 1);
    }

    // The same rebuilds on a backend that builds layouts itself (poimen-server, #645).

    /// [`restarted`] on a fake that answers `apply_layout` with `answer`.
    fn restarted_layout(cwd: &str, answer: backend::LaidOut) -> FakeBackend {
        let fake = restarted(cwd);
        *fake.laid_out.lock().unwrap() = Some(answer);
        fake
    }

    fn trust_question(dir: &str) -> trust::TrustRequest {
        trust::TrustRequest {
            root: dir.into(),
            files: vec![format!("{dir}/.poimen/config.json")],
            hash: "abc".into(),
            commands: vec![],
        }
    }

    #[test]
    fn a_layout_backend_rebuilds_the_space_itself_and_the_app_creates_nothing() {
        let (home, dir) = home_with_templates(SHELLS);
        let fake = restarted_layout(&dir, built("s1"));
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Rebuilt {
                warnings: vec!["layout: pane x could not be split".into()],
                setup_started: true
            }
        );
        // No terminal, split, rename or input from the app: `layout::apply` is herdr's only.
        assert_eq!(
            fake.calls(),
            ["snapshot".to_owned(), format!("apply_layout s1 {dir} - -")]
        );
    }

    #[test]
    fn a_layout_backend_still_needs_consent_for_an_agent_pane_before_anything_is_sent() {
        let fake = restarted_layout(&tmp_dir(), built("s1"));
        let out = rebuild_space_with(&fake, None, "s1", None, false).unwrap();
        assert!(matches!(out, RebuildOutcome::Consent { .. }), "{out:?}");
        assert_eq!(fake.calls(), ["snapshot"]);
        // Confirmed, the server builds it.
        let out = rebuild_space_with(&fake, None, "s1", None, true).unwrap();
        assert!(matches!(out, RebuildOutcome::Rebuilt { .. }), "{out:?}");
        assert_eq!(
            fake.calls().last().unwrap(),
            &format!("apply_layout s1 {} - -", tmp_dir())
        );
    }

    #[test]
    fn a_layout_backend_answers_the_trust_question_and_stores_the_trust_itself() {
        let (home, dir) = home_with_templates(SHELLS);
        let poimen_dir = std::path::Path::new(&dir).join(".poimen");
        std::fs::create_dir_all(&poimen_dir).unwrap();
        let file = r#"{"layout":{"panes":[{"name":"dev","command":"pnpm dev"}]}}"#;
        std::fs::write(poimen_dir.join("config.json"), file).unwrap();
        let config = home.join(".config/poimen/config.json");

        // Unconfirmed: consent, nothing sent.
        let fake = restarted_layout(&dir, backend::LaidOut::Trust(trust_question(&dir)));
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, false).unwrap();
        assert!(matches!(out, RebuildOutcome::Consent { .. }), "{out:?}");
        assert_eq!(fake.calls(), ["snapshot"]);
        // Confirmed: the server's question goes to the trust dialog.
        let out = rebuild_space_with(&fake, Some(&home), "s1", None, true).unwrap();
        assert_eq!(
            out,
            RebuildOutcome::Trust {
                request: trust_question(&dir)
            }
        );
        assert_eq!(
            fake.calls().last().unwrap(),
            &format!("apply_layout s1 {dir} - -")
        );

        // The answer goes to the server; the app writes nothing to the user config.
        let fake = restarted_layout(&dir, built("s1"));
        let hash = trust::sha256_hex(file);
        let decision = Some(trust::Decision::Trust { hash: hash.clone() });
        let out = rebuild_space_with(&fake, Some(&home), "s1", decision, true).unwrap();
        assert!(matches!(out, RebuildOutcome::Rebuilt { .. }), "{out:?}");
        assert_eq!(
            fake.calls().last().unwrap(),
            &format!("apply_layout s1 {dir} - trust:{hash}")
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), SHELLS);
        let decision = Some(trust::Decision::Without);
        rebuild_space_with(&fake, Some(&home), "s1", decision, true).unwrap();
        assert_eq!(
            fake.calls().last().unwrap(),
            &format!("apply_layout s1 {dir} - without")
        );
    }

    #[test]
    fn a_failing_apply_layout_is_an_error() {
        let (home, dir) = home_with_templates(SHELLS);
        let fake = restarted_layout(&dir, built("s1"));
        fake.fail_on("apply_layout");
        let err = rebuild_space_with(&fake, Some(&home), "s1", None, true).unwrap_err();
        assert_eq!(err, "boom");
        assert_eq!(fake.calls(), ["snapshot"]);
    }

    #[test]
    fn a_returned_id_is_qualified_like_the_id_the_request_addressed() {
        assert_eq!(qualify_like("box::3", "9".into()), "box::9");
        assert_eq!(qualify_like("3", "9".into()), "9");
    }

    #[test]
    fn agent_commands_are_recognized_by_their_program() {
        let harnesses = harness::default_harnesses();
        for command in [
            "claude",
            "claude --resume x",
            "/usr/bin/claude",
            "'copilot'",
        ] {
            assert!(is_agent_command(command, &harnesses), "{command}");
        }
        assert!(is_agent_command(r"C:\bin\Copilot.EXE -p x", &harnesses));
        for command in ["", "  ", "vim", "pnpm dev", "claudette"] {
            assert!(!is_agent_command(command, &harnesses), "{command}");
        }
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
    fn the_app_package_is_poimen_app_and_the_identity_is_unchanged() {
        let cargo: toml::Value = include_str!("../Cargo.toml").parse().unwrap();
        assert_eq!(cargo["package"]["name"].as_str(), Some("poimen-app"));
        assert_eq!(cargo["package"]["default-run"].as_str(), Some("poimen-app"));
        assert_eq!(cargo["lib"]["name"].as_str(), Some("poimen_lib"));
        let base: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(base["productName"], "poimen");
        assert_eq!(base["identifier"], "dev.gravionlabs.poimen");
    }

    #[test]
    fn the_rc_overlay_installs_beside_the_stable_app_without_updates() {
        use poimen_client::protocol::Channel;
        let base: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let bundle: Value =
            serde_json::from_str(include_str!("../tauri.bundle.conf.json")).unwrap();
        let rc: Value = serde_json::from_str(include_str!("../tauri.rc.conf.json")).unwrap();
        assert_eq!(rc["identifier"], "dev.gravionlabs.poimen.rc");
        assert_ne!(rc["identifier"], base["identifier"]);
        assert_eq!(rc["productName"], "Poimen RC");
        assert_eq!(rc["productName"], window_title(Channel::Rc));
        // Its own binary (Linux: /usr/bin/poimen-rc next to /usr/bin/poimen).
        assert_eq!(rc["mainBinaryName"], "poimen-rc");
        assert_eq!(rc["bundle"]["createUpdaterArtifacts"], false);
        assert_eq!(rc["plugins"]["updater"]["endpoints"], json!([]));
        // The sidecars are the names the app of each channel looks for beside itself: the server,
        // the PTY host the server looks for beside itself, and the console (`poimen`,
        // `poimen-cli-rc` on rc so it can sit beside the stable one).
        assert_eq!(
            bundle["bundle"]["externalBin"],
            json!([
                format!("binaries/{}", Channel::Stable.server_program(false)),
                format!("binaries/{}", Channel::Stable.host_program(false)),
                "binaries/poimen"
            ])
        );
        assert_eq!(
            rc["bundle"]["externalBin"],
            json!([
                format!("binaries/{}", Channel::Rc.server_program(false)),
                format!("binaries/{}", Channel::Rc.host_program(false)),
                "binaries/poimen-cli-rc"
            ])
        );
    }

    #[test]
    fn the_window_title_names_the_rc_channel() {
        use poimen_client::protocol::Channel;
        assert_eq!(window_title(Channel::Stable), "Poimen");
        assert_eq!(window_title(Channel::Rc), "Poimen RC");
        let base: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(
            base["app"]["windows"][0]["title"],
            window_title(Channel::Stable)
        );
    }

    #[test]
    fn the_backend_setting_picks_the_backend_() {
        assert_eq!(make_backend("poimen").1, "poimen");
        assert_eq!(make_backend("herdr").1, "herdr");
        assert_eq!(make_backend("").1, "poimen");
        assert!(make_backend("poimen").0.emulated());
        assert!(!make_backend("herdr").0.emulated());
    }
}
