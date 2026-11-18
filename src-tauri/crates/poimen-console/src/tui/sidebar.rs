//! The sidebar model: spaces, their tabs and terminals from the server (`space.list` and
//! `terminal.list`; events only make the TUI read them again), rendered into rows with the
//! tokens of `sidebar.rows` the way the app's sidebar does (`src/app/sidebar/spaces.ts`).
//!
//! Tokens the TUI has no source for yet (`branch`, `dirty`, `sync`, `pr`) are empty and skipped.

use std::collections::{HashMap, HashSet};

use poimen_client::protocol::{AgentStatus, Space, Tab, Terminal};
use poimen_client::{Client, ClientError};
use poimen_core::order::{attention_rank, compare_space_names};
use poimen_core::settings::{RowToken, SidebarRows, SidebarSort};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub spaces: Vec<Space>,
    pub tabs: Vec<Tab>,
    pub terminals: Vec<Terminal>,
    /// Repository of a space's `cwd` (by directory), for the worktree groups; empty when
    /// grouping is off. A directory missing here is a flat space.
    pub repos: HashMap<String, RepoRef>,
}

/// What the worktree groups need of a directory's repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub common_dir: String,
    /// True for a linked worktree, false for the main checkout.
    pub worktree: bool,
}

/// Repository info per directory, read once: a failed read (not a repository, or a directory
/// that does not exist on this machine, as for a remote server) is kept as `None`.
pub struct RepoCache {
    known: HashMap<String, Option<RepoRef>>,
    lookup: fn(&str) -> Option<RepoRef>,
}

fn git_lookup(dir: &str) -> Option<RepoRef> {
    poimen_core::git::info(dir).map(|i| RepoRef {
        common_dir: i.common_dir,
        worktree: i.worktree,
    })
}

impl Default for RepoCache {
    fn default() -> Self {
        Self::with_lookup(git_lookup)
    }
}

impl RepoCache {
    pub fn with_lookup(lookup: fn(&str) -> Option<RepoRef>) -> Self {
        Self {
            known: HashMap::new(),
            lookup,
        }
    }

    /// The repositories of `dirs` that resolve, reading each directory the first time only.
    pub fn resolve<'a>(&mut self, dirs: impl Iterator<Item = &'a str>) -> HashMap<String, RepoRef> {
        let mut out = HashMap::new();
        for dir in dirs {
            let lookup = self.lookup;
            let found = self
                .known
                .entry(dir.to_owned())
                .or_insert_with(|| lookup(dir));
            if let Some(r) = found {
                out.insert(dir.to_owned(), r.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Space,
    /// A tab with more than one terminal; a tab of one terminal is just its terminal row.
    Tab,
    Terminal,
    /// A child record of the terminal above (OSC 7501 with an `id`, #684).
    Record,
}

/// More child records than this are folded into one summary row.
pub const RECORDS_SHOWN: usize = 4;

/// What a segment of a row line is, for its style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Dot,
    Name,
    State,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
    pub bold: bool,
    pub dim: bool,
}

/// A tab (none for a terminal in no tab) with its terminals and its nesting level.
type TabEntry<'a> = (Option<&'a Tab>, Vec<&'a Terminal>, u16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    /// Set on terminal rows: selecting the row attaches to it.
    pub terminal_id: Option<String>,
    /// Indentation level: 0 space, 1 tab or terminal, 2 terminal of a split tab.
    pub depth: u16,
    /// The agent status of the terminal, or the highest of a space's terminals.
    pub status: Option<AgentStatus>,
    pub lines: Vec<Vec<Segment>>,
}

impl Row {
    pub fn height(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }
}

fn tokens(names: &[&str]) -> Vec<RowToken> {
    names
        .iter()
        .map(|t| RowToken {
            token: (*t).to_owned(),
            bold: false,
            dim: false,
        })
        .collect()
}

/// The default lines per row type, as in the app (`DEFAULT_SIDEBAR_ROWS`).
pub fn default_lines(kind: &str) -> Vec<Vec<RowToken>> {
    match kind {
        "workspace" => vec![
            tokens(&["state_icon", "name", "state_text", "dir"]),
            tokens(&["branch", "dirty", "sync"]),
        ],
        _ => vec![
            tokens(&["state_icon", "name", "state_text"]),
            tokens(&["agent", "title", "branch", "dir"]),
        ],
    }
}

/// The icon of a status (`state_icon`); `·` without an agent.
pub fn icon(status: Option<AgentStatus>) -> &'static str {
    match status {
        Some(AgentStatus::Working) => "●",
        Some(AgentStatus::Blocked) => "▲",
        Some(AgentStatus::Error) => "✗",
        Some(AgentStatus::Done) => "✓",
        Some(AgentStatus::Idle) => "○",
        Some(AgentStatus::Gone) | None => "·",
    }
}

fn status_name(status: AgentStatus) -> Option<&'static str> {
    match status {
        AgentStatus::Idle => Some("idle"),
        AgentStatus::Working => Some("working"),
        AgentStatus::Blocked => Some("blocked"),
        AgentStatus::Done => Some("done"),
        AgentStatus::Error => Some("error"),
        AgentStatus::Gone => None,
    }
}

fn rank(status: Option<AgentStatus>) -> u8 {
    match status {
        Some(AgentStatus::Error) => 5,
        Some(AgentStatus::Blocked) => 4,
        Some(AgentStatus::Working) => 3,
        Some(AgentStatus::Done) => 2,
        Some(AgentStatus::Idle) => 1,
        _ => 0,
    }
}

fn terminal_status(t: &Terminal) -> Option<AgentStatus> {
    t.agent
        .as_ref()
        .map(|a| a.status)
        .filter(|s| *s != AgentStatus::Gone)
}

/// `~` for the home directory at the start of a path.
pub fn short_path(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty()) {
        Some(h) if path == h => "~".into(),
        Some(h) => match path.strip_prefix(h) {
            Some(rest) if rest.starts_with(['/', '\\']) => format!("~{rest}"),
            _ => path.into(),
        },
        None => path.into(),
    }
}

fn same(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// What a row's tokens read from.
struct Subject<'a> {
    name: &'a str,
    status: Option<AgentStatus>,
    /// 0 to 100, of the terminal or of the terminals that give a space its status.
    progress: Option<u8>,
    /// Why the agent is blocked (`permission`, `question`, `auth`).
    kind: Option<&'a str>,
    terminal: Option<&'a Terminal>,
    tab: Option<&'a str>,
    space: &'a Space,
}

/// Text a program reported is shown as plain text whatever server sent it (an older or remote
/// one may not clean it): bidi overrides and invisible characters go, an unknown kind goes.
pub fn plain_agent_text(t: &mut Terminal) {
    for r in &mut t.records {
        r.message = r
            .message
            .as_deref()
            .and_then(poimen_protocol::clean_message);
        r.title = r.title.as_deref().and_then(poimen_protocol::clean_message);
        r.app = r.app.as_deref().and_then(poimen_protocol::clean_message);
        r.progress = r.progress.map(|p| p.min(100));
        r.kind = r
            .kind
            .take()
            .filter(|k| poimen_protocol::BLOCK_KINDS.contains(&k.as_str()));
    }
    let Some(a) = t.agent.as_mut() else { return };
    a.message = a
        .message
        .as_deref()
        .and_then(poimen_protocol::clean_message);
    a.title = a.title.as_deref().and_then(poimen_protocol::clean_message);
    a.kind = a
        .kind
        .take()
        .filter(|k| poimen_protocol::BLOCK_KINDS.contains(&k.as_str()));
    a.progress = a.progress.map(|p| p.min(100));
}

/// `blocked: permission`, `working 40%`: the status word with what OSC 7501 adds.
fn state_text(s: &Subject<'_>) -> Option<String> {
    let mut text = status_name(s.status?)?.to_owned();
    if s.status == Some(AgentStatus::Blocked)
        && let Some(kind) = s.kind
    {
        text.push_str(": ");
        text.push_str(kind);
    }
    if let Some(progress) = s.progress {
        text.push_str(&format!(" {progress}%"));
    }
    Some(text)
}

/// The rows of the child records of a terminal: one per record in id order (a parent before its
/// children, indented by its level), or one summary row when there are more than
/// [`RECORDS_SHOWN`].
fn record_rows(t: &Terminal, depth: u16) -> Vec<Row> {
    let seg = |kind, text: String, dim| Segment {
        kind,
        text,
        bold: false,
        dim,
    };
    if t.records.len() > RECORDS_SHOWN {
        let count = |s: AgentStatus| t.records.iter().filter(|r| r.status == s).count();
        let mut parts = vec![format!("{} records", t.records.len())];
        for (status, word) in [
            (AgentStatus::Error, "error"),
            (AgentStatus::Blocked, "blocked"),
            (AgentStatus::Working, "working"),
        ] {
            if count(status) > 0 {
                parts.push(format!("{} {word}", count(status)));
            }
        }
        let worst = AgentStatus::Error;
        let status = [
            worst,
            AgentStatus::Blocked,
            AgentStatus::Working,
            AgentStatus::Done,
        ]
        .into_iter()
        .find(|s| count(*s) > 0);
        return vec![Row {
            kind: RowKind::Record,
            terminal_id: None,
            depth,
            status,
            lines: vec![vec![
                seg(SegmentKind::Dot, icon(status).into(), false),
                seg(SegmentKind::Text, parts.join(", "), true),
            ]],
        }];
    }
    let mut sorted: Vec<&poimen_protocol::ChildRecord> = t.records.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    sorted
        .into_iter()
        .map(|r| {
            let level = u16::try_from(r.id.matches('/').count()).unwrap_or(0);
            let leaf = r.id.rsplit('/').next().unwrap_or(&r.id);
            let name = r.title.clone().unwrap_or_else(|| leaf.to_owned());
            let mut state = status_name(r.status).unwrap_or("").to_owned();
            if r.status == AgentStatus::Blocked
                && let Some(kind) = &r.kind
            {
                state.push_str(&format!(": {kind}"));
            }
            if let Some(p) = r.progress {
                state.push_str(&format!(" {p}%"));
            }
            let mut line = vec![
                seg(SegmentKind::Dot, icon(Some(r.status)).into(), false),
                seg(SegmentKind::Name, name.clone(), false),
                seg(SegmentKind::State, state, false),
            ];
            if let Some(m) = r.message.as_ref().filter(|m| same(m) != same(&name)) {
                line.push(seg(SegmentKind::Text, m.clone(), true));
            }
            Row {
                kind: RowKind::Record,
                terminal_id: None,
                depth: depth + level,
                status: Some(r.status),
                lines: vec![line],
            }
        })
        .collect()
}

/// The progress shown for `status` among `terminals`: the first one with that status that has one.
fn progress_of<'a>(
    status: Option<AgentStatus>,
    terminals: impl Iterator<Item = &'a Terminal>,
) -> Option<u8> {
    let status = status?;
    terminals
        .filter(|t| terminal_status(t) == Some(status))
        .find_map(|t| t.agent.as_ref()?.progress)
        .map(|p| p.min(100))
}

fn value(token: &str, s: &Subject<'_>, home: Option<&str>) -> Option<String> {
    let t = s.terminal;
    match token {
        "name" => Some(s.name.to_owned()),
        "state_text" => state_text(s),
        "agent" => t.and_then(|t| t.agent.as_ref()?.agent.clone()),
        // As the app: an agent's step (`deliver_step`), nothing for a plain terminal.
        "title" => t
            .filter(|t| t.agent.is_some())
            .and_then(|t| t.tokens.get("deliver_step").cloned()),
        "tab" => s.tab.map(str::to_owned),
        "notification" => t.and_then(|t| t.agent.as_ref()?.message.clone()),
        "dir" => match t {
            None => Some(short_path(&s.space.cwd, home)),
            Some(t) if t.cwd != s.space.cwd => Some(short_path(&t.cwd, home)),
            Some(_) => None,
        },
        other => other
            .strip_prefix('$')
            .and_then(|name| t?.tokens.get(name).cloned()),
    }
}

/// The lines of a row: empty values are skipped, and so is a value equal to an earlier one on
/// the row (the name counts as shown); lines without anything but the icon are dropped.
fn render(lines: &[Vec<RowToken>], s: &Subject<'_>, home: Option<&str>) -> Vec<Vec<Segment>> {
    let mut seen = HashSet::from([same(s.name)]);
    lines
        .iter()
        .map(|spec| {
            let mut out = Vec::new();
            for RowToken { token, bold, dim } in spec {
                let seg = |kind, text: String| Segment {
                    kind,
                    text,
                    bold: *bold,
                    dim: *dim,
                };
                if token == "state_icon" {
                    out.push(seg(SegmentKind::Dot, icon(s.status).into()));
                    continue;
                }
                let Some(text) = value(token, s, home)
                    .map(|v| v.trim().to_owned())
                    .filter(|v| !v.is_empty())
                else {
                    continue;
                };
                match token.as_str() {
                    "name" => out.push(seg(SegmentKind::Name, text)),
                    "state_text" => out.push(seg(SegmentKind::State, text)),
                    _ => {
                        if seen.insert(same(&text)) {
                            out.push(seg(SegmentKind::Text, text));
                        }
                    }
                }
            }
            out
        })
        .filter(|l: &Vec<Segment>| l.iter().any(|s| s.kind != SegmentKind::Dot))
        .collect()
}

impl Model {
    /// Reads spaces, tabs and terminals from the server.
    pub fn load(client: &Client) -> Result<Self, ClientError> {
        let layout = client.list_layout()?;
        let mut terminals = client.list_terminals(None)?;
        terminals.iter_mut().for_each(plain_agent_text);
        Ok(Self {
            spaces: layout.spaces,
            tabs: layout.tabs,
            terminals,
            repos: HashMap::new(),
        })
    }

    /// [`Model::load`] plus the repositories of the spaces' directories for the worktree
    /// groups (`sidebar.groupWorktrees`), read through `cache`.
    pub fn load_grouped(client: &Client, cache: &mut RepoCache) -> Result<Self, ClientError> {
        let mut model = Self::load(client)?;
        model.repos = cache.resolve(model.spaces.iter().map(|s| s.cwd.as_str()));
        Ok(model)
    }

    /// Puts the spaces of linked worktrees directly after the space of the main checkout of
    /// their repository, keeping the given order inside heads and inside members. A group
    /// without a main checkout and a space without repository stay flat. Returns each space
    /// with whether it is a member.
    fn grouped<'a>(&self, spaces: Vec<&'a Space>) -> Vec<(&'a Space, bool)> {
        let repo = |s: &Space| self.repos.get(&s.cwd);
        let mut heads: HashMap<&str, &str> = HashMap::new();
        for s in &spaces {
            if let Some(r) = repo(s).filter(|r| !r.worktree) {
                heads.entry(r.common_dir.as_str()).or_insert(s.id.as_str());
            }
        }
        let head_of = |s: &Space| {
            repo(s)
                .filter(|r| r.worktree)
                .and_then(|r| heads.get(r.common_dir.as_str()).copied())
        };
        let mut out = Vec::new();
        for s in &spaces {
            if head_of(s).is_some() {
                continue;
            }
            out.push((*s, false));
            for m in &spaces {
                if head_of(m) == Some(s.id.as_str()) {
                    out.push((*m, true));
                }
            }
        }
        out
    }

    pub fn terminal(&self, id: &str) -> Option<&Terminal> {
        self.terminals.iter().find(|t| t.id == id)
    }

    /// The tab whose tree holds the terminal.
    pub fn tab_of(&self, terminal_id: &str) -> Option<&Tab> {
        self.tabs
            .iter()
            .find(|t| t.root.contains_terminal(terminal_id))
    }

    /// The tabs of a space in the space's order.
    pub fn space_tabs(&self, space_id: &str) -> Vec<&Tab> {
        let Some(space) = self.spaces.iter().find(|s| s.id == space_id) else {
            return Vec::new();
        };
        space
            .tabs
            .iter()
            .filter_map(|id| self.tabs.iter().find(|t| &t.tab_id == id))
            .collect()
    }

    /// The tabs of a space as a tree (#656): a tab right before its children (`Tab::parent`), each
    /// with its terminals depth first and its nesting level (0 top level). A tab whose parent is
    /// not in the space is top level. Terminals of the space in no tab (an older server) come
    /// last, as tabs of their own.
    fn tabs_of<'a>(&'a self, space: &'a Space) -> Vec<TabEntry<'a>> {
        let tabs: Vec<&Tab> = space
            .tabs
            .iter()
            .filter_map(|id| self.tabs.iter().find(|t| &t.tab_id == id))
            .collect();
        let listed = |id: &str| tabs.iter().any(|t| t.tab_id == id);
        let mut out = Vec::new();
        let mut placed = HashSet::new();
        let mut seen = HashSet::new();
        // Depth first from the top level tabs; a parent cycle is never reached from one.
        fn visit<'a>(
            model: &'a Model,
            tabs: &[&'a Tab],
            tab: &'a Tab,
            level: u16,
            seen: &mut HashSet<&'a str>,
            placed: &mut HashSet<&'a str>,
            out: &mut Vec<TabEntry<'a>>,
        ) {
            if !seen.insert(tab.tab_id.as_str()) {
                return;
            }
            let terms: Vec<&Terminal> = tab
                .root
                .terminal_ids()
                .into_iter()
                .filter_map(|id| model.terminal(id))
                .collect();
            placed.extend(terms.iter().map(|t| t.id.as_str()));
            out.push((Some(tab), terms, level));
            for child in tabs
                .iter()
                .filter(|c| c.parent.as_deref() == Some(tab.tab_id.as_str()))
            {
                visit(model, tabs, child, level + 1, seen, placed, out);
            }
        }
        for tab in &tabs {
            let top = tab
                .parent
                .as_deref()
                .is_none_or(|p| !listed(p) || p == tab.tab_id);
            if top {
                visit(self, &tabs, tab, 0, &mut seen, &mut placed, &mut out);
            }
        }
        // A cycle of parents has no top level tab: show what was not reached as top level.
        for tab in &tabs {
            visit(self, &tabs, tab, 0, &mut seen, &mut placed, &mut out);
        }
        for t in &self.terminals {
            if t.space_id == space.id && !placed.contains(t.id.as_str()) {
                out.push((None, vec![t], 0));
            }
        }
        out
    }

    /// The sidebar rows: spaces in server order for `manual`, by name for `name` and `recent`
    /// (the focus times are UI state of the app), by agent urgency for `attention` without any
    /// unread terminal.
    pub fn rows(&self, config: &SidebarRows, sort: SidebarSort, home: Option<&str>) -> Vec<Row> {
        self.rows_unread(config, sort, home, &HashSet::new())
    }

    /// [`Model::rows`] with the terminals the TUI marks unread (`tui/attention.rs`): a done agent
    /// ranks as unread only through that set.
    pub fn rows_unread(
        &self,
        config: &SidebarRows,
        sort: SidebarSort,
        home: Option<&str>,
        unread: &HashSet<String>,
    ) -> Vec<Row> {
        let workspace = config
            .workspace
            .clone()
            .unwrap_or_else(|| default_lines("workspace"));
        let agent = config
            .agent
            .clone()
            .unwrap_or_else(|| default_lines("agent"));
        let terminal = config
            .terminal
            .clone()
            .unwrap_or_else(|| default_lines("terminal"));
        let mut rows = Vec::new();
        let mut spaces: Vec<&Space> = self.spaces.iter().collect();
        match sort {
            SidebarSort::Name | SidebarSort::Recent => {
                spaces.sort_by(|a, b| compare_space_names(&a.name, &a.id, &b.name, &b.id));
            }
            SidebarSort::Attention => {
                let rank_of = |space: &Space| {
                    self.terminals
                        .iter()
                        .filter(|t| t.space_id == space.id)
                        .map(|t| attention_rank(terminal_status(t), unread.contains(&t.id)))
                        .min()
                        .unwrap_or(4)
                };
                spaces.sort_by(|a, b| {
                    rank_of(a)
                        .cmp(&rank_of(b))
                        .then_with(|| compare_space_names(&a.name, &a.id, &b.name, &b.id))
                });
            }
            SidebarSort::Manual => {}
        }
        for (space, member) in self.grouped(spaces) {
            let base = u16::from(member);
            let tabs = self.tabs_of(space);
            let status = tabs
                .iter()
                .flat_map(|(_, ts, _)| ts.iter().map(|t| terminal_status(t)))
                .max_by_key(|s| rank(*s))
                .flatten();
            let subject = Subject {
                name: &space.name,
                status,
                progress: progress_of(
                    status,
                    tabs.iter().flat_map(|(_, ts, _)| ts.iter().copied()),
                ),
                kind: None,
                terminal: None,
                tab: None,
                space,
            };
            rows.push(Row {
                kind: RowKind::Space,
                terminal_id: None,
                depth: base,
                status,
                lines: render(&workspace, &subject, home),
            });
            for (tab, terms, level) in tabs {
                let mut depth = base + 1 + level;
                if let (Some(tab), true) = (tab, terms.len() > 1) {
                    rows.push(Row {
                        kind: RowKind::Tab,
                        terminal_id: None,
                        depth: base + 1 + level,
                        status: None,
                        lines: vec![vec![Segment {
                            kind: SegmentKind::Name,
                            text: tab.name.clone(),
                            bold: false,
                            dim: true,
                        }]],
                    });
                    depth = base + 2 + level;
                }
                for t in terms {
                    let status = terminal_status(t);
                    let agent_info = t.agent.as_ref();
                    let subject = Subject {
                        name: &t.name,
                        status,
                        progress: agent_info.and_then(|a| a.progress).map(|p| p.min(100)),
                        kind: agent_info.and_then(|a| a.kind.as_deref()),
                        terminal: Some(t),
                        tab: tab.map(|t| t.name.as_str()),
                        space,
                    };
                    let lines = if t.agent.is_some() { &agent } else { &terminal };
                    rows.push(Row {
                        kind: RowKind::Terminal,
                        terminal_id: Some(t.id.clone()),
                        depth,
                        status,
                        lines: render(lines, &subject, home),
                    });
                    rows.extend(record_rows(t, depth + 1));
                }
            }
        }
        rows
    }

    /// The terminal to show first: the focused terminal of the first space's active tab, else
    /// the first terminal; never `skip` (the terminal the TUI itself runs in).
    pub fn initial(&self, skip: Option<&str>) -> Option<String> {
        let usable = |id: &str| Some(id) != skip && self.terminal(id).is_some();
        for space in &self.spaces {
            let active = space
                .active_tab
                .as_ref()
                .and_then(|id| self.tabs.iter().find(|t| &t.tab_id == id));
            if let Some(tab) = active {
                if usable(&tab.active_terminal) {
                    return Some(tab.active_terminal.clone());
                }
                if let Some(id) = tab.root.terminal_ids().into_iter().find(|id| usable(id)) {
                    return Some(id.to_owned());
                }
            }
        }
        self.terminals
            .iter()
            .map(|t| t.id.as_str())
            .find(|id| usable(id))
            .map(str::to_owned)
    }
}

/// The leaf to focus when a tab is shown: its focused one, else the first that is not `skip`
/// (the terminal the TUI runs in). `None` when the tab holds nothing else.
pub fn focus_target(tab: &Tab, skip: Option<&str>) -> Option<String> {
    let ok = |id: &str| Some(id) != skip;
    if ok(&tab.active_terminal) && tab.root.contains_terminal(&tab.active_terminal) {
        return Some(tab.active_terminal.clone());
    }
    tab.root
        .terminal_ids()
        .into_iter()
        .find(|id| ok(id))
        .map(str::to_owned)
}

/// The terminal to focus when the server's focus moved while `selected` stayed: the other
/// active tab of the selected terminal's space, or the other focused leaf of its tab. `None`
/// when nothing moved (also on the first read, and for the terminal this client just focused).
pub fn remote_focus(
    old: &Model,
    new: &Model,
    selected: &str,
    skip: Option<&str>,
) -> Option<String> {
    let tab = new.tab_of(selected)?;
    let old_tab = old.tab_of(selected).filter(|t| t.tab_id == tab.tab_id);
    let old_space = old.spaces.iter().find(|s| s.id == tab.space_id)?;
    let space = new.spaces.iter().find(|s| s.id == tab.space_id)?;
    if old_space.active_tab != space.active_tab
        && let Some(active) = space
            .active_tab
            .as_ref()
            .and_then(|id| new.tabs.iter().find(|t| &t.tab_id == id))
        && active.tab_id != tab.tab_id
    {
        return focus_target(active, skip);
    }
    let old_tab = old_tab?;
    (old_tab.active_terminal != tab.active_terminal
        && tab.active_terminal != selected
        && Some(tab.active_terminal.as_str()) != skip
        && tab.root.contains_terminal(&tab.active_terminal))
    .then(|| tab.active_terminal.clone())
}

/// The terminal to focus when `selected` is gone from `new`: the focus of its tab when the
/// tab is still there (a pane closed), else the active tab of its space (the tab closed), else
/// the first usable terminal.
pub fn after_loss(old: &Model, new: &Model, selected: &str, skip: Option<&str>) -> Option<String> {
    let lost = old.tab_of(selected);
    if let Some(lost) = lost {
        if let Some(tab) = new.tabs.iter().find(|t| t.tab_id == lost.tab_id)
            && let Some(id) = focus_target(tab, skip)
        {
            return Some(id);
        }
        let active = new
            .spaces
            .iter()
            .find(|s| s.id == lost.space_id)
            .and_then(|s| s.active_tab.as_ref())
            .and_then(|id| new.tabs.iter().find(|t| &t.tab_id == id));
        if let Some(id) = active.and_then(|t| focus_target(t, skip)) {
            return Some(id);
        }
    }
    new.initial(skip)
}

/// The terminal `delta` terminal rows away from `current` (wrapping), skipping `skip`; the first
/// one when `current` is not in the rows.
pub fn step(
    rows: &[Row],
    current: Option<&str>,
    delta: isize,
    skip: Option<&str>,
) -> Option<String> {
    let ids: Vec<&str> = rows
        .iter()
        .filter_map(|r| r.terminal_id.as_deref())
        .filter(|id| Some(*id) != skip)
        .collect();
    if ids.is_empty() {
        return None;
    }
    let Some(at) = current.and_then(|c| ids.iter().position(|id| *id == c)) else {
        return Some(ids[0].to_owned());
    };
    let n = ids.len() as isize;
    let next = (at as isize + delta).rem_euclid(n);
    Some(ids[next as usize].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_client::protocol::{AgentInfo, Node, SplitDirection, TerminalStatus};
    use std::collections::BTreeMap;

    fn term(id: &str, space: &str, name: &str, cwd: &str) -> Terminal {
        Terminal {
            id: id.into(),
            space_id: space.into(),
            name: name.into(),
            cwd: cwd.into(),
            cols: 80,
            rows: 24,
            status: TerminalStatus::Running,
            exit_code: None,
            program: None,
            args: Vec::new(),
            agent: None,
            tokens: BTreeMap::new(),
            survives_restart: false,
            host_version: None,
            host_bundled_version: None,
            records: Vec::new(),
            hibernated: None,
        }
    }

    fn agent(status: AgentStatus) -> Option<AgentInfo> {
        Some(AgentInfo {
            reported_at: None,
            status_since: None,
            status,
            agent: Some("claude".into()),
            session_id: None,
            message: Some("needs approval".into()),
            message_at: None,
            progress: None,
            kind: None,
            title: None,
        })
    }

    fn tab(id: &str, space: &str, name: &str, root: Node) -> Tab {
        let active = root.first_terminal().to_owned();
        Tab {
            tab_id: id.into(),
            space_id: space.into(),
            name: name.into(),
            root,
            active_terminal: active,
            parent: None,
            place: None,
        }
    }

    /// Space `work` (`/home/u/work`): tab `shell` with t1, tab `pair` split into t2 (agent,
    /// blocked) and t3 (in a subdirectory, a token). Space `empty` has nothing.
    fn model() -> Model {
        let mut t2 = term("t2", "s1", "claude", "/home/u/work");
        t2.agent = agent(AgentStatus::Blocked);
        let mut t3 = term("t3", "s1", "logs", "/home/u/work/sub");
        t3.tokens.insert("cost".into(), "$1.20".into());
        Model {
            spaces: vec![
                Space {
                    id: "s1".into(),
                    name: "work".into(),
                    cwd: "/home/u/work".into(),
                    tabs: vec!["a".into(), "b".into()],
                    active_tab: Some("b".into()),
                },
                Space {
                    id: "s2".into(),
                    name: "empty".into(),
                    cwd: "/srv".into(),
                    tabs: vec![],
                    active_tab: None,
                },
            ],
            tabs: vec![
                tab("a", "s1", "shell", Node::terminal("t1")),
                tab(
                    "b",
                    "s1",
                    "pair",
                    Node::Split {
                        split_id: "x".into(),
                        direction: SplitDirection::Right,
                        ratio: 0.5,
                        children: Box::new([Node::terminal("t2"), Node::terminal("t3")]),
                    },
                ),
            ],
            terminals: vec![term("t1", "s1", "shell", "/home/u/work"), t2, t3],
            repos: HashMap::new(),
        }
    }

    fn texts(row: &Row) -> Vec<String> {
        row.lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    fn record(id: &str, status: AgentStatus) -> poimen_protocol::ChildRecord {
        poimen_protocol::ChildRecord {
            id: id.into(),
            status,
            app: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            title: None,
        }
    }

    #[test]
    fn child_records_are_rows_below_their_terminal_and_fold_when_many() {
        let mut t = term("t1", "s1", "zsh", "/w");
        let mut a = record("run/a", AgentStatus::Blocked);
        a.kind = Some("permission".into());
        a.progress = Some(30);
        a.message = Some("Allow?\u{202E}".into());
        t.records = vec![record("run", AgentStatus::Working), a];
        plain_agent_text(&mut t);
        let rows = record_rows(&t, 2);
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.kind == RowKind::Record && r.terminal_id.is_none())
        );
        assert_eq!((rows[0].depth, rows[1].depth), (2, 3));
        let text = |r: &Row| {
            r.lines[0]
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("|")
        };
        assert_eq!(text(&rows[0]), "●|run|working");
        assert_eq!(text(&rows[1]), "▲|a|blocked: permission 30%|Allow?");
        t.records = (0..5)
            .map(|i| record(&format!("r{i}"), AgentStatus::Working))
            .collect();
        t.records[0].status = AgentStatus::Error;
        let folded = record_rows(&t, 2);
        assert_eq!(folded.len(), 1);
        assert_eq!(text(&folded[0]), "✗|5 records, 1 error, 4 working");
    }

    #[test]
    fn child_tabs_follow_their_parent_indented() {
        let mut m = model();
        // `shell` (a) gets a child `logs` (c) with t4; the space lists a, b, c.
        m.terminals.push(term("t4", "s1", "tail", "/home/u/work"));
        let mut c = tab("c", "s1", "logs", Node::terminal("t4"));
        c.parent = Some("a".into());
        m.tabs.push(c);
        m.spaces[0].tabs.push("c".into());
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        let shape: Vec<_> = rows
            .iter()
            .filter(|r| r.kind != RowKind::Tab)
            .map(|r| (r.depth, r.terminal_id.clone()))
            .collect();
        assert_eq!(
            shape,
            [
                (0, None),
                (1, Some("t1".into())),
                (2, Some("t4".into())),
                (2, Some("t2".into())),
                (2, Some("t3".into())),
                (0, None),
            ]
        );
    }

    #[test]
    fn a_tab_with_an_unknown_parent_or_a_cycle_is_still_listed() {
        let mut m = model();
        m.tabs[0].parent = Some("b".into());
        m.tabs[1].parent = Some("a".into());
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(rows.iter().filter(|r| r.terminal_id.is_some()).count(), 3);
        m.tabs[0].parent = Some("gone".into());
        m.tabs[1].parent = None;
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(rows.iter().filter(|r| r.terminal_id.is_some()).count(), 3);
    }

    #[test]
    fn rows_list_spaces_tabs_and_terminals_in_order() {
        let rows = model().rows(
            &SidebarRows::default(),
            SidebarSort::Manual,
            Some("/home/u"),
        );
        let shape: Vec<_> = rows
            .iter()
            .map(|r| (r.kind, r.depth, r.terminal_id.clone()))
            .collect();
        assert_eq!(
            shape,
            [
                (RowKind::Space, 0, None),
                (RowKind::Terminal, 1, Some("t1".into())),
                (RowKind::Tab, 1, None),
                (RowKind::Terminal, 2, Some("t2".into())),
                (RowKind::Terminal, 2, Some("t3".into())),
                (RowKind::Space, 0, None),
            ]
        );
    }

    #[test]
    fn default_rows_show_status_agent_and_paths() {
        let rows = model().rows(
            &SidebarRows::default(),
            SidebarSort::Manual,
            Some("/home/u"),
        );
        // The space takes the highest status of its terminals.
        assert_eq!(texts(&rows[0]), ["▲ work blocked ~/work"]);
        assert_eq!(rows[0].status, Some(AgentStatus::Blocked));
        // A plain terminal in the space's directory: only its name.
        assert_eq!(texts(&rows[1]), ["· shell"]);
        assert_eq!(texts(&rows[2]), ["pair"]);
        assert_eq!(texts(&rows[3]), ["▲ claude blocked"]);
        // A differing directory is shown.
        assert_eq!(texts(&rows[4]), ["· logs", "~/work/sub"]);
        assert_eq!(texts(&rows[5]), ["· empty /srv"]);
    }

    #[test]
    fn configured_rows_show_tokens_and_notifications() {
        let config = SidebarRows {
            terminal: Some(vec![tokens(&["name", "$cost", "tab"])]),
            agent: Some(vec![
                tokens(&["state_icon", "name"]),
                tokens(&["notification", "agent"]),
            ]),
            workspace: Some(vec![tokens(&["name"])]),
        };
        let rows = model().rows(&config, SidebarSort::Manual, None);
        assert_eq!(texts(&rows[0]), ["work"]);
        // `tab` equal to the name is skipped as a repeat.
        assert_eq!(texts(&rows[1]), ["shell"]);
        assert_eq!(texts(&rows[3]), ["▲ claude", "needs approval"]);
        assert_eq!(texts(&rows[4]), ["logs $1.20 pair"]);
    }

    fn repo_space(id: &str, name: &str, cwd: &str) -> Space {
        Space {
            id: id.into(),
            name: name.into(),
            cwd: cwd.into(),
            tabs: vec![],
            active_tab: None,
        }
    }

    fn repo_ref(common: &str, worktree: bool) -> RepoRef {
        RepoRef {
            common_dir: common.into(),
            worktree,
        }
    }

    /// Spaces in server order: a worktree of repo A, a flat one, the main of A, a worktree of B
    /// (no main), another worktree of A, and one more flat.
    fn grouped_model() -> Model {
        let mut m = Model {
            spaces: vec![
                repo_space("s1", "a-fix", "/w/a-fix"),
                repo_space("s2", "notes", "/w/notes"),
                repo_space("s3", "a", "/w/a"),
                repo_space("s4", "b-fix", "/w/b-fix"),
                repo_space("s5", "a-feat", "/w/a-feat"),
            ],
            ..Model::default()
        };
        m.repos.insert("/w/a-fix".into(), repo_ref("/a/.git", true));
        m.repos.insert("/w/a".into(), repo_ref("/a/.git", false));
        m.repos.insert("/w/b-fix".into(), repo_ref("/b/.git", true));
        m.repos
            .insert("/w/a-feat".into(), repo_ref("/a/.git", true));
        m
    }

    fn space_rows(m: &Model, sort: SidebarSort) -> Vec<(String, u16)> {
        m.rows(&SidebarRows::default(), sort, None)
            .iter()
            .filter(|r| r.kind == RowKind::Space)
            .map(|r| (r.lines[0][1].text.clone(), r.depth))
            .collect()
    }

    fn named(rows: &[(&str, u16)]) -> Vec<(String, u16)> {
        rows.iter().map(|(n, d)| (n.to_string(), *d)).collect()
    }

    #[test]
    fn worktree_spaces_follow_their_head_indented_in_manual_order() {
        assert_eq!(
            space_rows(&grouped_model(), SidebarSort::Manual),
            named(&[
                ("notes", 0),
                ("a", 0),
                ("a-fix", 1),
                ("a-feat", 1),
                ("b-fix", 0),
            ])
        );
    }

    #[test]
    fn members_are_ordered_inside_their_head_by_the_sort_mode() {
        assert_eq!(
            space_rows(&grouped_model(), SidebarSort::Name),
            named(&[
                ("a", 0),
                ("a-feat", 1),
                ("a-fix", 1),
                ("b-fix", 0),
                ("notes", 0),
            ])
        );
    }

    #[test]
    fn without_repository_info_every_space_stays_flat() {
        let mut m = grouped_model();
        m.repos.clear();
        let rows = space_rows(&m, SidebarSort::Manual);
        assert!(rows.iter().all(|(_, d)| *d == 0));
        assert_eq!(rows[0].0, "a-fix");
    }

    #[test]
    fn tabs_and_terminals_of_a_member_sit_one_level_deeper() {
        let mut m = grouped_model();
        m.spaces[0].tabs = vec!["ta".into()];
        m.tabs = vec![tab("ta", "s1", "ta", Node::terminal("x1"))];
        m.terminals = vec![term("x1", "s1", "sh", "/w/a-fix")];
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        let t = rows.iter().find(|r| r.terminal_id.is_some()).unwrap();
        assert_eq!(t.depth, 2);
    }

    #[test]
    fn step_follows_the_grouped_order() {
        let mut m = grouped_model();
        for (i, s) in ["s1", "s2", "s3"].iter().enumerate() {
            let id = format!("x{i}");
            let space = m.spaces.iter_mut().find(|sp| sp.id == *s).unwrap();
            space.tabs = vec![format!("t{i}")];
            m.tabs.push(tab(
                &format!("t{i}"),
                s,
                &format!("t{i}"),
                Node::terminal(&id),
            ));
            m.terminals.push(term(&id, s, "sh", "/x"));
        }
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        // Shown order: notes (x1), a (x2), a-fix (x0).
        assert_eq!(step(&rows, Some("x1"), 1, None).as_deref(), Some("x2"));
        assert_eq!(step(&rows, Some("x2"), 1, None).as_deref(), Some("x0"));
    }

    #[test]
    fn the_cache_reads_a_directory_once_and_keeps_a_failure_flat() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn lookup(dir: &str) -> Option<RepoRef> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            (dir == "/w/a").then(|| repo_ref("/a/.git", false))
        }
        let mut cache = RepoCache::with_lookup(lookup);
        let dirs = ["/w/a", "/remote/only"];
        let first = cache.resolve(dirs.iter().copied());
        let second = cache.resolve(dirs.iter().copied());
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert!(first.contains_key("/w/a"));
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn initial_is_the_focused_terminal_of_the_active_tab() {
        let m = model();
        assert_eq!(m.initial(None).as_deref(), Some("t2"));
        // Never the terminal the TUI runs in.
        assert_eq!(m.initial(Some("t2")).as_deref(), Some("t3"));
        assert_eq!(Model::default().initial(None), None);
    }

    #[test]
    fn step_moves_between_terminal_rows_and_wraps() {
        let rows = model().rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(step(&rows, Some("t1"), 1, None).as_deref(), Some("t2"));
        assert_eq!(step(&rows, Some("t3"), 1, None).as_deref(), Some("t1"));
        assert_eq!(step(&rows, Some("t1"), -1, None).as_deref(), Some("t3"));
        assert_eq!(
            step(&rows, Some("t1"), 1, Some("t2")).as_deref(),
            Some("t3")
        );
        assert_eq!(step(&rows, Some("gone"), 1, None).as_deref(), Some("t1"));
        assert_eq!(step(&[], None, 1, None), None);
    }

    #[test]
    fn name_sort_orders_spaces_and_step_follows_it() {
        let rows = model().rows(&SidebarRows::default(), SidebarSort::Name, None);
        // "empty" (s2) before "work" (s1); its terminal-less space has no terminal row.
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds[0], RowKind::Space);
        assert!(
            texts(&rows[0])[0].contains("empty"),
            "{:?}",
            texts(&rows[0])
        );
        assert!(texts(&rows[1])[0].contains("work"), "{:?}", texts(&rows[1]));
        // Terminal rows keep their tab order below the second space; step walks them as shown.
        assert_eq!(step(&rows, Some("t3"), 1, None).as_deref(), Some("t1"));
    }

    #[test]
    fn error_progress_and_the_kind_of_a_block_show_in_the_rows() {
        let mut m = model();
        let info = |status, progress, kind: Option<&str>| {
            Some(AgentInfo {
                reported_at: None,
                status_since: None,
                status,
                agent: Some("codex".into()),
                session_id: None,
                message: None,
                message_at: None,
                progress,
                kind: kind.map(str::to_owned),
                title: None,
            })
        };
        m.terminals[1].agent = info(AgentStatus::Blocked, None, Some("permission"));
        m.terminals[2].agent = info(AgentStatus::Working, Some(40), None);
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        let line = |name: &str| {
            rows.iter()
                .find(|r| texts(r)[0].contains(name))
                .map(|r| texts(r)[0].clone())
                .unwrap()
        };
        assert!(
            line("claude").contains("blocked: permission"),
            "{}",
            line("claude")
        );
        assert!(line("logs").contains("working 40%"), "{}", line("logs"));
        // The space row shows the progress only when its own status is the one that has it.
        assert!(!line("work").contains('%'), "{}", line("work"));
        m.terminals[1].agent = info(AgentStatus::Idle, None, None);
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert!(
            texts(&rows[0])[0].contains("working 40%"),
            "{:?}",
            texts(&rows[0])
        );
        // Error has its own icon and outranks blocked on the space row.
        m.terminals[1].agent = info(AgentStatus::Error, None, None);
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(rows[0].status, Some(AgentStatus::Error));
        assert!(texts(&rows[0])[0].starts_with("✗"), "{:?}", texts(&rows[0]));
        assert!(texts(&rows[0])[0].contains("error"));
    }

    #[test]
    fn reported_text_is_plain_text() {
        let mut t = term("t9", "s1", "x", "/w");
        t.agent = Some(AgentInfo {
            reported_at: None,
            status_since: None,
            status: AgentStatus::Blocked,
            agent: None,
            session_id: None,
            message: Some("Allow \u{202E}gnp.exe\u{200B}?".into()),
            message_at: None,
            progress: Some(200),
            kind: Some("evil\u{202E}".into()),
            title: Some("\u{202E}".into()),
        });
        plain_agent_text(&mut t);
        let a = t.agent.unwrap();
        assert_eq!(a.message.as_deref(), Some("Allow gnp.exe?"));
        assert_eq!((a.kind, a.title, a.progress), (None, None, Some(100)));
    }

    #[test]
    fn attention_sort_ranks_by_the_most_urgent_agent_then_by_name() {
        let mut m = model();
        for (id, name) in [("s3", "Zed"), ("s4", "Mid"), ("s5", "Alpha")] {
            m.spaces.push(Space {
                id: id.into(),
                name: name.into(),
                cwd: "/srv".into(),
                tabs: vec![],
                active_tab: None,
            });
        }
        let mut blocked = term("b1", "s3", "b", "/srv");
        blocked.agent = agent(AgentStatus::Blocked);
        let mut done = term("d1", "s4", "d", "/srv");
        done.agent = agent(AgentStatus::Done);
        let mut working = term("w1", "s5", "w", "/srv");
        working.agent = agent(AgentStatus::Working);
        m.terminals.extend([blocked, done, working]);
        let order = |unread: &[&str]| -> Vec<String> {
            let unread = unread.iter().map(|s| (*s).to_owned()).collect();
            m.rows_unread(
                &SidebarRows::default(),
                SidebarSort::Attention,
                None,
                &unread,
            )
            .iter()
            .filter(|r| r.kind == RowKind::Space)
            .map(|r| texts(r)[0].clone())
            .collect()
        };
        let first = |v: Vec<String>| {
            v.iter()
                .map(|t| {
                    ["Zed", "Mid", "Alpha", "work", "empty"]
                        .into_iter()
                        .find(|n| t.contains(n))
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        // Blocked ("work" and "Zed", by name), working, the read done one, no agent.
        assert_eq!(first(order(&[])), ["work", "Zed", "Alpha", "Mid", "empty"]);
        // Unread done ranks right after blocked.
        assert_eq!(
            first(order(&["d1"])),
            ["work", "Zed", "Mid", "Alpha", "empty"]
        );
    }

    #[test]
    fn recent_sort_falls_back_to_the_name() {
        let m = model();
        let recent = m.rows(&SidebarRows::default(), SidebarSort::Recent, None);
        let by_name = m.rows(&SidebarRows::default(), SidebarSort::Name, None);
        assert_eq!(recent, by_name);
    }

    #[test]
    fn name_sort_moves_a_later_space_first_and_manual_keeps_server_order() {
        let mut m = model();
        m.terminals.push(term("t9", "s3", "z", "/srv"));
        m.spaces.push(Space {
            id: "s3".into(),
            name: "Alpha 9".into(),
            cwd: "/srv".into(),
            tabs: vec![],
            active_tab: None,
        });
        let by_name = m.rows(&SidebarRows::default(), SidebarSort::Name, None);
        // Alpha 9, empty, work: t9 comes before t1.
        assert_eq!(step(&by_name, Some("t9"), 1, None).as_deref(), Some("t1"));
        let manual = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(step(&manual, Some("t3"), 1, None).as_deref(), Some("t9"));
    }

    #[test]
    fn short_path_replaces_the_home_directory() {
        assert_eq!(short_path("/home/u", Some("/home/u")), "~");
        assert_eq!(short_path("/home/u/x", Some("/home/u")), "~/x");
        assert_eq!(short_path("/home/user", Some("/home/u")), "/home/user");
        assert_eq!(short_path("/srv", None), "/srv");
    }

    #[test]
    fn terminals_without_a_tab_still_show() {
        let mut m = model();
        m.terminals.push(term("t9", "s2", "loose", "/srv"));
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        assert_eq!(rows.last().unwrap().terminal_id.as_deref(), Some("t9"));
    }

    #[test]
    fn tab_lookup_and_order() {
        let m = model();
        assert_eq!(m.tab_of("t3").unwrap().tab_id, "b");
        assert!(m.tab_of("nope").is_none());
        let names: Vec<_> = m.space_tabs("s1").iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, ["shell", "pair"]);
        assert!(m.space_tabs("s2").is_empty());
        assert!(m.space_tabs("nope").is_empty());
    }

    #[test]
    fn focus_target_is_the_focused_leaf_else_the_first_usable() {
        let mut m = model();
        let pair = m.tabs[1].clone();
        assert_eq!(focus_target(&pair, None).as_deref(), Some("t2"));
        m.tabs[1].active_terminal = "t3".into();
        assert_eq!(focus_target(&m.tabs[1], None).as_deref(), Some("t3"));
        // The terminal the TUI runs in is skipped.
        assert_eq!(focus_target(&m.tabs[1], Some("t3")).as_deref(), Some("t2"));
        assert_eq!(focus_target(&m.tabs[0], Some("t1")), None);
        // A stale focus falls back to the first leaf.
        m.tabs[1].active_terminal = "gone".into();
        assert_eq!(focus_target(&m.tabs[1], None).as_deref(), Some("t2"));
    }

    #[test]
    fn remote_focus_follows_a_leaf_or_tab_focused_elsewhere() {
        let old = model();
        // Nothing moved.
        assert_eq!(remote_focus(&old, &old, "t2", None), None);
        // Another client focused t3 in our tab.
        let mut new = old.clone();
        new.tabs[1].active_terminal = "t3".into();
        assert_eq!(remote_focus(&old, &new, "t2", None).as_deref(), Some("t3"));
        // Our own focus (we are on t3 already) is not a move.
        assert_eq!(remote_focus(&old, &new, "t3", None), None);
        // Another client switched the space to tab a.
        let mut new = old.clone();
        new.spaces[0].active_tab = Some("a".into());
        assert_eq!(remote_focus(&old, &new, "t2", None).as_deref(), Some("t1"));
        // We are on that tab already.
        assert_eq!(remote_focus(&old, &new, "t1", None), None);
        // The first read has no earlier model.
        assert_eq!(remote_focus(&Model::default(), &old, "t2", None), None);
    }

    #[test]
    fn after_loss_prefers_the_same_tab_then_the_active_tab_then_anything() {
        let old = model();
        // t3 closed: the pair tab stays with t2.
        let mut new = old.clone();
        new.tabs[1].root = Node::terminal("t2");
        new.terminals.retain(|t| t.id != "t3");
        assert_eq!(after_loss(&old, &new, "t3", None).as_deref(), Some("t2"));
        // The whole pair tab closed: the server's active tab of the space is shown.
        let mut new = old.clone();
        new.tabs.retain(|t| t.tab_id != "b");
        new.spaces[0].tabs = vec!["a".into()];
        new.spaces[0].active_tab = Some("a".into());
        new.terminals.retain(|t| t.id == "t1");
        assert_eq!(after_loss(&old, &new, "t2", None).as_deref(), Some("t1"));
        // Its space is empty now: any terminal.
        let mut new = new.clone();
        new.spaces[0].active_tab = None;
        assert_eq!(after_loss(&old, &new, "t2", None).as_deref(), Some("t1"));
        assert_eq!(after_loss(&old, &Model::default(), "t2", None), None);
    }
}
