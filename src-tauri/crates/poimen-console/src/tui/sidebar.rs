//! The sidebar model: spaces, tabs and terminals from the server (`space.list`, `terminal.list`;
//! events only make the TUI read them again), rendered into rows with the tokens of `sidebar.rows`
//! like the app's sidebar (`src/app/sidebar/spaces.ts`); tokens without a source are skipped.

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
    /// Repository per directory for the worktree groups (empty when off); a missing one is a flat space.
    pub repos: HashMap<String, RepoRef>,
    /// `sidebar.groupWorktrees` is off: `repos` feeds the git tokens only.
    pub flat: bool,
    /// `sidebar.showBaseBranch`: the `branch` token shows the base branch too.
    pub show_base_branch: bool,
}

#[path = "sidebar_git.rs"]
pub mod git;
use git::GitView;
pub use git::{RepoCache, RepoRef};

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
    /// `done` with the tone `warning` (#1152): shown as a warning, not as a success.
    pub warning: bool,
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
            tokens(&["agent", "title", "started_by", "branch", "dir"]),
        ],
    }
}

/// The icon of a status (`state_icon`); `·` without an agent.
pub fn icon(status: Option<AgentStatus>, warning: bool) -> &'static str {
    match status {
        Some(AgentStatus::Done) if warning => "!",
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

/// `done` with the tone `warning` (#1152).
fn terminal_warning(t: &Terminal) -> bool {
    t.hibernated.is_none()
        && t.agent
            .as_ref()
            .is_some_and(|a| a.status == AgentStatus::Done && a.tone.as_deref() == Some("warning"))
}

fn terminal_status(t: &Terminal) -> Option<AgentStatus> {
    // A hibernated agent is not running: it is neither waiting nor unread (#904).
    if t.hibernated.is_some() {
        return None;
    }
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
    /// `done` with the tone `warning`.
    warning: bool,
    terminal: Option<&'a Terminal>,
    tab: Option<&'a str>,
    space: &'a Space,
    starter: Option<&'a str>,
    git: GitView<'a>,
}

/// Reported text is shown as plain text whatever server sent it: bidi overrides, invisible characters and an unknown kind go.
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
        r.tone = r
            .tone
            .take()
            .filter(|t| poimen_protocol::TONES.contains(&t.as_str()));
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
    a.tone = a
        .tone
        .take()
        .filter(|t| poimen_protocol::TONES.contains(&t.as_str()));
    a.progress = a.progress.map(|p| p.min(100));
}

/// `blocked: permission`, `working 40%`: the status word with what OSC 7501 adds.
fn state_text(s: &Subject<'_>) -> Option<String> {
    if s.terminal.is_some_and(|t| t.hibernated.is_some()) {
        return Some("hibernated".to_owned());
    }
    let mut text = status_name(s.status?)?.to_owned();
    if s.status == Some(AgentStatus::Blocked)
        && let Some(kind) = s.kind
    {
        text.push_str(": ");
        text.push_str(kind);
    }
    if s.warning {
        text.push_str(": warning");
    }
    if let Some(progress) = s.progress {
        text.push_str(&format!(" {progress}%"));
    }
    // The server decided the agent is silent (#898); the client only formats (#900).
    if let Some(since) = s
        .terminal
        .and_then(|t| t.agent.as_ref()?.silent_since)
        .filter(|_| s.status == Some(AgentStatus::Working))
    {
        text.push(' ');
        text.push_str(&silent_label(since, now_ms()));
    }
    Some(text)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// `silent 12m`: time as the app's `formatElapsed` (`now` under a minute, `5m`, `2h`, `3d`).
fn silent_label(since_ms: u64, now_ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    let ms = now_ms.saturating_sub(since_ms);
    let elapsed = if ms < MINUTE {
        "now".to_owned()
    } else if ms < 60 * MINUTE {
        format!("{}m", ms / MINUTE)
    } else if ms < 24 * 60 * MINUTE {
        format!("{}h", ms / (60 * MINUTE))
    } else {
        format!("{}d", ms / (24 * 60 * MINUTE))
    };
    format!("silent {elapsed}")
}

/// The child-record rows of a terminal: one per record in id order (parent first, indented by
/// level), or one summary row when there are more than [`RECORDS_SHOWN`].
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
            warning: false,
            lines: vec![vec![
                seg(SegmentKind::Dot, icon(status, false).into(), false),
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
            if record_warning(r) {
                state.push_str(": warning");
            }
            if let Some(p) = r.progress {
                state.push_str(&format!(" {p}%"));
            }
            let mut line = vec![
                seg(
                    SegmentKind::Dot,
                    icon(Some(r.status), record_warning(r)).into(),
                    false,
                ),
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
                warning: record_warning(r),
                lines: vec![line],
            }
        })
        .collect()
}

fn record_warning(r: &poimen_protocol::ChildRecord) -> bool {
    r.status == AgentStatus::Done && r.tone.as_deref() == Some("warning")
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
        "started_by" => s.starter.map(|n| format!("↳ {n}")),
        "notification" => t.and_then(|t| t.agent.as_ref()?.message.clone()),
        "branch" | "dirty" | "sync" | "pr" => s.git.token(token),
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

/// The lines of a row: empty values and repeats of an earlier one (the name counts) are skipped, icon-only lines dropped.
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
                    out.push(seg(SegmentKind::Dot, icon(s.status, s.warning).into()));
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
            flat: false,
            show_base_branch: false,
        })
    }

    /// [`Model::load`] plus what `cache` knows of the directories of the spaces and terminals (worktree groups and git tokens); unread ones fill in later.
    pub fn load_with_repos(
        client: &Client,
        cache: &RepoCache,
        config: &super::config::Config,
    ) -> Result<Self, ClientError> {
        let mut model = Self::load(client)?;
        model.repos = model.resolve_repos(cache);
        model.flat = !config.group_worktrees;
        model.show_base_branch = config.show_base_branch;
        Ok(model)
    }

    /// The repositories of the model's directories as far as `cache` has read them.
    pub fn resolve_repos(&self, cache: &RepoCache) -> HashMap<String, RepoRef> {
        cache.resolve(self.dirs())
    }

    /// [`Model::resolve_repos`] that reads on this thread first (tests).
    pub fn resolve_repos_now(&self, cache: &RepoCache) -> HashMap<String, RepoRef> {
        cache.resolve_now(self.dirs())
    }

    fn dirs(&self) -> impl Iterator<Item = &str> + Clone {
        let terminals = self.terminals.iter().map(|t| t.cwd.as_str());
        self.spaces.iter().map(|s| s.cwd.as_str()).chain(terminals)
    }

    /// Puts linked-worktree spaces directly after the space of their repository's main checkout,
    /// keeping the order inside heads and members. A group without a main checkout and a space
    /// without repository stay flat. Returns each space with whether it is a member.
    fn grouped<'a>(&self, spaces: Vec<&'a Space>) -> Vec<(&'a Space, bool)> {
        let repo = |s: &Space| self.repos.get(&s.cwd).filter(|_| !self.flat);
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

    /// The repositories a row's git tokens read: a space's own, a terminal's next to its space's.
    fn git_view(&self, space: &Space, terminal_cwd: Option<&str>) -> GitView<'_> {
        let space_repo = self.repos.get(&space.cwd);
        let (own, space) = match terminal_cwd {
            None => (space_repo, None),
            Some(cwd) => (self.repos.get(cwd), space_repo),
        };
        GitView {
            own,
            space,
            show_base_branch: self.show_base_branch,
        }
    }

    /// The name of the terminal that started `t` (#982), when it is still there.
    fn starter_of(&self, t: &Terminal) -> Option<&str> {
        let id = t.started_by.as_deref()?;
        Some(&self.terminal(id)?.name)
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
    /// with its terminals depth first and its level (0 top level); a tab whose parent is not in
    /// the space is top level. Terminals in no tab (an older server) come last, as tabs of their own.
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

    /// The sidebar rows: spaces in server order for `manual`, by name for `name` and `recent` (the
    /// focus times are app UI state), by agent urgency for `attention` without any unread terminal.
    pub fn rows(&self, config: &SidebarRows, sort: SidebarSort, home: Option<&str>) -> Vec<Row> {
        self.rows_unread(config, sort, home, &HashSet::new())
    }

    /// [`Model::rows`] with the terminals the TUI marks unread (`tui/attention.rs`); a done agent ranks as unread only so.
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
            let warning = status == Some(AgentStatus::Done)
                && tabs
                    .iter()
                    .flat_map(|(_, ts, _)| ts.iter())
                    .any(|t| terminal_warning(t));
            let subject = Subject {
                name: &space.name,
                status,
                warning,
                progress: progress_of(
                    status,
                    tabs.iter().flat_map(|(_, ts, _)| ts.iter().copied()),
                ),
                kind: None,
                terminal: None,
                tab: None,
                space,
                starter: None,
                git: self.git_view(space, None),
            };
            rows.push(Row {
                kind: RowKind::Space,
                terminal_id: None,
                depth: base,
                status,
                warning,
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
                        warning: false,
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
                        warning: terminal_warning(t),
                        progress: agent_info.and_then(|a| a.progress).map(|p| p.min(100)),
                        kind: agent_info.and_then(|a| a.kind.as_deref()),
                        terminal: Some(t),
                        tab: tab.map(|t| t.name.as_str()),
                        space,
                        starter: self.starter_of(t),
                        git: self.git_view(space, Some(&t.cwd)),
                    };
                    let lines = if t.agent.is_some() { &agent } else { &terminal };
                    rows.push(Row {
                        kind: RowKind::Terminal,
                        terminal_id: Some(t.id.clone()),
                        depth,
                        status,
                        warning: terminal_warning(t),
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

/// The terminal to focus when the server's focus moved while `selected` stayed: the other active
/// tab of its space, or the other focused leaf of its tab. `None` when nothing moved (also on the
/// first read, and for the terminal this client just focused).
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

/// The terminal to focus when `selected` is gone from `new`: the focus of its tab (a pane closed),
/// else the active tab of its space (the tab closed), else the first usable terminal.
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
#[path = "sidebar_tests.rs"]
mod tests;
