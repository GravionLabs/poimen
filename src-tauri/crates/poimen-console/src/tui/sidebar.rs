//! The sidebar model: spaces, their tabs and terminals from the server (`space.list` and
//! `terminal.list`; events only make the TUI read them again), rendered into rows with the
//! tokens of `sidebar.rows` the way the app's sidebar does (`src/app/sidebar/spaces.ts`).
//!
//! Tokens the TUI has no source for yet (`branch`, `dirty`, `sync`, `pr`) are empty and skipped.

use std::collections::HashSet;

use poimen_client::protocol::{AgentStatus, Space, Tab, Terminal};
use poimen_client::{Client, ClientError};
use poimen_core::settings::{RowToken, SidebarRows};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub spaces: Vec<Space>,
    pub tabs: Vec<Tab>,
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Space,
    /// A tab with more than one terminal; a tab of one terminal is just its terminal row.
    Tab,
    Terminal,
}

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
            tokens(&[
                "state_icon",
                "name",
                "state_text",
                "branch",
                "dirty",
                "sync",
            ]),
            tokens(&["dir"]),
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
        AgentStatus::Gone => None,
    }
}

fn rank(status: Option<AgentStatus>) -> u8 {
    match status {
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
    terminal: Option<&'a Terminal>,
    tab: Option<&'a str>,
    space: &'a Space,
}

fn value(token: &str, s: &Subject<'_>, home: Option<&str>) -> Option<String> {
    let t = s.terminal;
    match token {
        "name" => Some(s.name.to_owned()),
        "state_text" => s.status.and_then(status_name).map(str::to_owned),
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
        let terminals = client.list_terminals(None)?;
        Ok(Self {
            spaces: layout.spaces,
            tabs: layout.tabs,
            terminals,
        })
    }

    pub fn terminal(&self, id: &str) -> Option<&Terminal> {
        self.terminals.iter().find(|t| t.id == id)
    }

    /// The tabs of a space in order, each with its terminals depth first; terminals of the
    /// space in no tab (an older server) come last, as tabs of their own.
    fn tabs_of<'a>(&'a self, space: &'a Space) -> Vec<(Option<&'a Tab>, Vec<&'a Terminal>)> {
        let mut out = Vec::new();
        let mut placed = HashSet::new();
        for id in &space.tabs {
            let Some(tab) = self.tabs.iter().find(|t| &t.tab_id == id) else {
                continue;
            };
            let terms: Vec<&Terminal> = tab
                .root
                .terminal_ids()
                .into_iter()
                .filter_map(|id| self.terminal(id))
                .collect();
            placed.extend(terms.iter().map(|t| t.id.as_str()));
            out.push((Some(tab), terms));
        }
        for t in &self.terminals {
            if t.space_id == space.id && !placed.contains(t.id.as_str()) {
                out.push((None, vec![t]));
            }
        }
        out
    }

    /// The sidebar rows, spaces in server order.
    pub fn rows(&self, config: &SidebarRows, home: Option<&str>) -> Vec<Row> {
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
        for space in &self.spaces {
            let tabs = self.tabs_of(space);
            let status = tabs
                .iter()
                .flat_map(|(_, ts)| ts.iter().map(|t| terminal_status(t)))
                .max_by_key(|s| rank(*s))
                .flatten();
            let subject = Subject {
                name: &space.name,
                status,
                terminal: None,
                tab: None,
                space,
            };
            rows.push(Row {
                kind: RowKind::Space,
                terminal_id: None,
                depth: 0,
                status,
                lines: render(&workspace, &subject, home),
            });
            for (tab, terms) in tabs {
                let mut depth = 1;
                if let (Some(tab), true) = (tab, terms.len() > 1) {
                    rows.push(Row {
                        kind: RowKind::Tab,
                        terminal_id: None,
                        depth: 1,
                        status: None,
                        lines: vec![vec![Segment {
                            kind: SegmentKind::Name,
                            text: tab.name.clone(),
                            bold: false,
                            dim: true,
                        }]],
                    });
                    depth = 2;
                }
                for t in terms {
                    let status = terminal_status(t);
                    let subject = Subject {
                        name: &t.name,
                        status,
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
            agent: None,
            tokens: BTreeMap::new(),
        }
    }

    fn agent(status: AgentStatus) -> Option<AgentInfo> {
        Some(AgentInfo {
            status,
            agent: Some("claude".into()),
            session_id: None,
            message: Some("needs approval".into()),
            message_at: None,
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

    #[test]
    fn rows_list_spaces_tabs_and_terminals_in_order() {
        let rows = model().rows(&SidebarRows::default(), Some("/home/u"));
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
        let rows = model().rows(&SidebarRows::default(), Some("/home/u"));
        // The space takes the highest status of its terminals.
        assert_eq!(texts(&rows[0]), ["▲ work blocked", "~/work"]);
        assert_eq!(rows[0].status, Some(AgentStatus::Blocked));
        // A plain terminal in the space's directory: only its name.
        assert_eq!(texts(&rows[1]), ["· shell"]);
        assert_eq!(texts(&rows[2]), ["pair"]);
        assert_eq!(texts(&rows[3]), ["▲ claude blocked"]);
        // A differing directory is shown.
        assert_eq!(texts(&rows[4]), ["· logs", "~/work/sub"]);
        assert_eq!(texts(&rows[5]), ["· empty", "/srv"]);
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
        let rows = model().rows(&config, None);
        assert_eq!(texts(&rows[0]), ["work"]);
        // `tab` equal to the name is skipped as a repeat.
        assert_eq!(texts(&rows[1]), ["shell"]);
        assert_eq!(texts(&rows[3]), ["▲ claude", "needs approval"]);
        assert_eq!(texts(&rows[4]), ["logs $1.20 pair"]);
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
        let rows = model().rows(&SidebarRows::default(), None);
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
        let rows = m.rows(&SidebarRows::default(), None);
        assert_eq!(rows.last().unwrap().terminal_id.as_deref(), Some("t9"));
    }
}
