//! `create_space` and `list_repositories` (#1209): how a lead agent gets a space for a repository
//! by name or directory, and which repositories poimen knows.

use std::collections::BTreeMap;
use std::path::Path;

use poimen_client::Client;
use poimen_client::protocol::{AgentWriteAction, SpaceCreate, SpaceCreateResult};
use poimen_core::{git, github, settings};
use serde_json::{Value, json};

use super::{Env, Fail, announce, authorize, runs, server_error, text_arg};

/// The tool definition of `create_space`.
pub(super) fn create_tool() -> Value {
    json!({
        "name": "create_space",
        "description": "Makes a space (a project in the poimen sidebar) for a directory, e.g. the checkout of a repository found with list_repositories, so you can start agents or shells there with start_agent or start_shell (`space`). Use it when no space has that directory yet; if one has, it is returned unchanged with `created: false` and nothing is asked. A new space always needs the user's confirmation in the poimen app (waits up to 60 s, refused when declined, not answered, or poimen is not open). The project's own layout is built as for any new space, and a project file with commands that the user has not trusted yet is refused: nothing is made, the user has to trust it in the app first. To work on an issue in its own worktree use start_agent with `worktree` instead.",
        "inputSchema": {"type": "object", "properties": {
            "directory": {"type": "string", "description": "An absolute path of an existing directory."},
            "name": {"type": "string", "description": "The name of the space; the name of the directory by default."}
        }, "required": ["directory"], "additionalProperties": false},
    })
}

/// The tool definition of `list_repositories`.
pub(super) fn list_tool() -> Value {
    json!({
        "name": "list_repositories",
        "description": "The git repositories poimen knows: those of its spaces (linked worktrees count as their main checkout) and the projects the user trusted, each with the path of its main checkout, the `origin` URL, `owner/repo` when that remote is on GitHub, and the spaces on it. Use it to find the directory that a repository name such as \"nixe\" refers to, then create_space for it when `spaces` is empty. Read-only; any space, no confirmation.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
    })
}

fn canonical(dir: &str) -> String {
    Path::new(dir)
        .canonicalize()
        .map_or_else(|_| dir.to_owned(), |p| p.display().to_string())
}

fn space_json(space: &poimen_client::protocol::Space) -> Value {
    json!({"id": space.id, "name": space.name, "directory": space.cwd})
}

/// `create_space {directory, name?}`.
pub(super) fn create_space(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["directory", "name"])?;
    let directory = text_arg(args, "directory")
        .ok_or_else(|| Fail::Tool("missing argument `directory`".into()))?;
    if !Path::new(directory).is_absolute() {
        return Err(Fail::Tool("`directory` must be an absolute path".into()));
    }
    let dir = Path::new(directory)
        .canonicalize()
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| Fail::Tool(format!("{directory} is not an existing directory")))?
        .display()
        .to_string();
    let spaces = client.list_spaces().map_err(server_error)?;
    if let Some(space) = spaces.iter().find(|s| canonical(&s.cwd) == dir) {
        return Ok(json!({"space": space_json(space), "created": false}));
    }
    let name = text_arg(args, "name").map(str::trim).map_or_else(
        || {
            Path::new(&dir)
                .file_name()
                .map_or_else(|| dir.clone(), |n| n.to_string_lossy().into_owned())
        },
        str::to_owned,
    );
    // The confirmation is asked in the caller's own space: a new space has no id yet.
    let caller = env.terminal_id.filter(|i| !i.is_empty());
    let own_space = match caller {
        Some(caller) => client
            .list_terminals(None)
            .map_err(server_error)?
            .into_iter()
            .find(|t| t.id == caller)
            .map(|t| t.space_id),
        None => None,
    };
    let own_space = own_space.unwrap_or_default();
    let text = format!("create a space \"{name}\" for {dir}");
    let confirmed = authorize(
        client,
        env,
        &own_space,
        AgentWriteAction::CreateSpace,
        None,
        &text,
        true,
    )?;
    let created = match client
        .create_space_with(&SpaceCreate::with_layout(&name, &dir))
        .map_err(server_error)?
    {
        SpaceCreateResult::TrustRequired { trust_required } => {
            return Err(Fail::Tool(format!(
                "refused: the project file of {} has commands that are not trusted yet, so no space \
                 was made; the user has to open the directory in the poimen app and trust it first",
                trust_required.root
            )));
        }
        SpaceCreateResult::Created(created) => created,
    };
    announce(
        client,
        env,
        &own_space,
        AgentWriteAction::CreateSpace,
        None,
        &text,
        confirmed,
    );
    Ok(json!({
        "space": space_json(&created.space),
        "created": true,
        "warnings": created.warnings,
    }))
}

/// One repository: its main checkout and what is known of its remote.
struct Repo {
    path: String,
    spaces: Vec<String>,
}

/// `list_repositories {}`.
pub(super) fn list_repositories(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &[])?;
    let spaces = client.list_spaces().map_err(server_error)?;
    let configured: Vec<String> = env
        .home
        .as_deref()
        .map(|h| settings::trusted_projects(h).into_keys().collect())
        .unwrap_or_default();
    // Keyed by the common git dir, so a worktree and its main checkout are one repository.
    let mut repos: BTreeMap<String, Repo> = BTreeMap::new();
    let dirs = spaces
        .iter()
        .map(|s| (s.cwd.as_str(), Some(s.id.as_str())))
        .chain(configured.iter().map(|d| (d.as_str(), None)));
    for (dir, space) in dirs {
        let Some((top, common)) = git::roots(dir) else {
            continue;
        };
        let main = poimen_core::worktree::main_worktree(Path::new(dir))
            .map_or(top, |p| p.display().to_string());
        let repo = repos.entry(common).or_insert_with(|| Repo {
            path: main,
            spaces: Vec::new(),
        });
        if let Some(id) = space {
            repo.spaces.push(id.to_owned());
        }
    }
    let mut list: Vec<Value> = repos
        .into_values()
        .map(|repo| {
            let remote = git::origin_url(&repo.path);
            let github = remote
                .as_deref()
                .and_then(github::parse_remote)
                .map(|r| format!("{}/{}", r.owner, r.name));
            json!({"path": repo.path, "remote": remote, "github": github, "spaces": repo.spaces})
        })
        .collect();
    list.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({"repositories": list}))
}
