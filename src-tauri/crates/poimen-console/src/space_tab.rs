//! `poimen space ...` and `poimen tab ...` (#1172): only `space.*`, `tab.*` and `terminal.*`
//! requests of `poimen-client`. Spaces and tabs are found by id, else by exact name.

use std::io::Write;
use std::path::Path;

use poimen_client::Client;
use poimen_client::protocol::{
    SpaceCreate, SpaceCreateResult, Tab, TabCreate, TabSplit, Terminal, TerminalCreate,
    TerminalStatus, TrustAnswer, TrustRequired,
};

use crate::cli::{Request, TrustFlag};
use crate::commands::{Failure, find_space};

/// Runs a space or tab request; any other request is not this module's.
pub fn run(client: &Client, request: &Request, out: &mut dyn Write) -> Result<(), Failure> {
    match request {
        Request::SpaceNew { dir, name, trust } => {
            space_new(client, dir, name.as_deref(), *trust, out)
        }
        Request::SpaceRename { space, name } => {
            let space = find_space(client, space)?;
            client.rename_space(&space.id, name)?;
            Ok(())
        }
        Request::SpaceClose { space, yes } => {
            let space = find_space(client, space)?;
            if !yes {
                refuse_running(client, &space.name, "space", Some(&space.id), None)?;
            }
            client.close_space(&space.id)?;
            Ok(())
        }
        Request::TabNew {
            space,
            name,
            command,
        } => tab_new(client, space, name.as_deref(), command.as_deref(), out),
        Request::TabRename { tab, name } => {
            let tab = find_tab(client, tab)?;
            client.rename_tab(&tab.tab_id, name)?;
            Ok(())
        }
        Request::TabClose { tab, yes } => {
            let tab = find_tab(client, tab)?;
            if !yes {
                refuse_running(client, &tab.name, "tab", None, Some(&tab))?;
            }
            client.close_tab(&tab.tab_id)?;
            Ok(())
        }
        Request::TabSplit {
            terminal,
            direction,
            ratio,
            command,
        } => {
            let (_, t) = client.split(&TabSplit {
                terminal_id: terminal.clone(),
                direction: *direction,
                ratio: *ratio,
                cwd: None,
                name: None,
                command: command.clone(),
                env: Default::default(),
            })?;
            writeln!(out, "{}", t.id)?;
            Ok(())
        }
        Request::TabFocus { tab, terminal } => {
            let tab = find_tab(client, tab)?;
            client.focus_tab(&tab.tab_id, terminal.as_deref())?;
            Ok(())
        }
        _ => unreachable!("not a space or tab request"),
    }
}

fn space_new(
    client: &Client,
    dir: &str,
    name: Option<&str>,
    trust: Option<TrustFlag>,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let path = Path::new(dir)
        .canonicalize()
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| Failure(format!("{dir} is not an existing directory")))?;
    let cwd = path.display().to_string();
    let name = name.map(str::to_owned).unwrap_or_else(|| {
        path.file_name()
            .map_or_else(|| cwd.clone(), |n| n.to_string_lossy().into_owned())
    });
    let mut params = SpaceCreate::with_layout(name, cwd);
    let mut sent_answer = false;
    loop {
        match client.create_space_with(&params)? {
            SpaceCreateResult::Created(created) => {
                for w in &created.warnings {
                    eprintln!("poimen: warning: {w}");
                }
                writeln!(out, "{}", created.space.id)?;
                return Ok(());
            }
            SpaceCreateResult::TrustRequired { trust_required } => {
                // An answer was sent and the server still asks (the files changed meanwhile).
                if sent_answer {
                    return Err(Failure(not_trusted(&trust_required)));
                }
                params.trust = Some(match trust {
                    None => return Err(Failure(not_trusted(&trust_required))),
                    Some(TrustFlag::Without) => TrustAnswer::Without,
                    Some(TrustFlag::Trust) => TrustAnswer::Trust {
                        hash: trust_required.hash,
                    },
                });
                sent_answer = true;
            }
        }
    }
}

/// The message of a refused `space new`: the commands and the hash, and how to answer.
fn not_trusted(t: &TrustRequired) -> String {
    let mut m = format!(
        "the project file of {} has commands that are not trusted yet; no space was made\n",
        t.root
    );
    for c in &t.commands {
        m.push_str(&format!("  {}: {}\n", c.what, c.command));
    }
    m.push_str(&format!(
        "hash: {}\nrun again with --trust to run them, or --without-commands to build the panes without them",
        t.hash
    ));
    m
}

fn tab_new(
    client: &Client,
    space: &str,
    name: Option<&str>,
    command: Option<&[String]>,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let space = find_space(client, space)?;
    let tab_id = match command {
        // A terminal of its own is a tab of its own (`terminal.create`).
        Some(command) => {
            let t = client.create_terminal(&TerminalCreate {
                space_id: space.id,
                name: name.map(str::to_owned),
                command: Some(command.to_vec()),
                ..Default::default()
            })?;
            client
                .list_layout()?
                .tabs
                .into_iter()
                .find(|tab| tab.root.contains_terminal(&t.id))
                .map(|tab| tab.tab_id)
                .ok_or_else(|| Failure(format!("the tab of terminal {} was not found", t.id)))?
        }
        None => {
            client
                .create_tab(&TabCreate {
                    space_id: space.id,
                    name: name.map(str::to_owned),
                    ..Default::default()
                })?
                .0
                .tab_id
        }
    };
    writeln!(out, "{tab_id}")?;
    Ok(())
}

/// A tab by id, else by exact name (an ambiguous name is an error).
fn find_tab(client: &Client, key: &str) -> Result<Tab, Failure> {
    let tabs = client.list_layout()?.tabs;
    if let Some(t) = tabs.iter().find(|t| t.tab_id == key) {
        return Ok(t.clone());
    }
    let mut named = tabs.into_iter().filter(|t| t.name == key);
    match (named.next(), named.next()) {
        (Some(t), None) => Ok(t),
        (None, _) => Err(Failure(format!("tab `{key}` not found"))),
        (Some(_), Some(_)) => Err(Failure(format!(
            "several tabs are named `{key}`; use the id"
        ))),
    }
}

/// Refuses a close while a terminal of the space (`space_id`) or of the tab runs an agent.
fn refuse_running(
    client: &Client,
    name: &str,
    kind: &str,
    space_id: Option<&str>,
    tab: Option<&Tab>,
) -> Result<(), Failure> {
    let running: Vec<Terminal> = client
        .list_terminals(space_id)?
        .into_iter()
        .filter(|t| t.status == TerminalStatus::Running && t.agent.is_some())
        .filter(|t| tab.is_none_or(|tab| tab.root.contains_terminal(&t.id)))
        .collect();
    if running.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = running.iter().map(|t| t.id.as_str()).collect();
    Err(Failure(format!(
        "refused: {kind} `{name}` has a running agent (terminal {}); use --yes to close it anyway",
        ids.join(", ")
    )))
}
