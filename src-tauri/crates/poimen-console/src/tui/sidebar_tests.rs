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
        started_by: None,
        report: None,
    }
}

fn agent(status: AgentStatus) -> Option<AgentInfo> {
    Some(AgentInfo {
        reported_at: None,
        status_since: None,
        silent_since: None,
        status,
        agent: Some("claude".into()),
        session_id: None,
        message: Some("needs approval".into()),
        message_at: None,
        progress: None,
        kind: None,
        tone: None,
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
        tone: None,
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
fn a_hibernated_terminal_shows_hibernated_and_is_not_waiting() {
    let mut m = model();
    // Its agent was `blocked` when the server hibernated it; it counts for nothing now.
    let t2 = m.terminals.iter_mut().find(|t| t.id == "t2").unwrap();
    t2.hibernated = Some(poimen_protocol::Hibernated {
        agent: "claude".into(),
        session_id: "s".into(),
        since: 1,
    });
    let rows = m.rows(
        &SidebarRows::default(),
        SidebarSort::Manual,
        Some("/home/u"),
    );
    assert_eq!(texts(&rows[3]), ["· claude hibernated"]);
    assert_eq!(rows[3].status, None);
    assert_eq!(rows[0].status, None, "the space has no waiting terminal");
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
fn silent_label_formats_the_elapsed_time() {
    assert_eq!(silent_label(1_000, 30_000), "silent now");
    assert_eq!(silent_label(0, 12 * 60_000 + 5), "silent 12m");
    assert_eq!(silent_label(0, 3 * 3_600_000), "silent 3h");
    assert_eq!(silent_label(0, 2 * 86_400_000), "silent 2d");
    assert_eq!(silent_label(10, 5), "silent now");
}

#[test]
fn a_silent_working_agent_shows_the_mark_after_its_status() {
    let mut m = model();
    let info = |status, silent_since| {
        Some(AgentInfo {
            reported_at: None,
            status_since: None,
            silent_since,
            status,
            agent: Some("codex".into()),
            session_id: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            tone: None,
            title: None,
        })
    };
    let line = |m: &Model| {
        let rows = m.rows(&SidebarRows::default(), SidebarSort::Manual, None);
        rows.iter()
            .find(|r| r.terminal_id.as_deref() == Some("t2"))
            .map(|r| texts(r)[0].clone())
            .unwrap()
    };
    m.terminals[1].agent = info(AgentStatus::Working, Some(now_ms() - 5 * 60_000));
    let text = line(&m);
    assert!(text.contains("working silent 5m"), "{text}");
    m.terminals[1].agent = info(AgentStatus::Working, None);
    assert!(!line(&m).contains("silent"));
}

#[test]
fn error_progress_and_the_kind_of_a_block_show_in_the_rows() {
    let mut m = model();
    let info = |status, progress, kind: Option<&str>| {
        Some(AgentInfo {
            reported_at: None,
            status_since: None,
            silent_since: None,
            status,
            agent: Some("codex".into()),
            session_id: None,
            message: None,
            message_at: None,
            progress,
            kind: kind.map(str::to_owned),
            tone: None,
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
        silent_since: None,
        status: AgentStatus::Blocked,
        agent: None,
        session_id: None,
        message: Some("Allow \u{202E}gnp.exe\u{200B}?".into()),
        message_at: None,
        progress: Some(200),
        kind: Some("evil\u{202E}".into()),
        tone: None,
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

#[test]
fn a_done_agent_with_a_warning_has_its_own_icon_and_text() {
    let mut m = model();
    let info = |tone: Option<&str>| {
        Some(AgentInfo {
            reported_at: None,
            status_since: None,
            silent_since: None,
            status: AgentStatus::Done,
            agent: Some("codex".into()),
            session_id: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            tone: tone.map(str::to_owned),
            title: None,
        })
    };
    let row = |m: &Model| {
        m.rows(&SidebarRows::default(), SidebarSort::Manual, None)
            .into_iter()
            .find(|r| r.terminal_id.as_deref() == Some("t2"))
            .unwrap()
    };
    m.terminals[1].agent = info(Some("warning"));
    let r = row(&m);
    assert!(r.warning);
    let text = texts(&r)[0].clone();
    assert!(text.contains("done: warning"), "{text}");
    assert!(text.contains('!'), "{text}");
    m.terminals[1].agent = info(None);
    let r = row(&m);
    assert!(!r.warning);
    assert!(!texts(&r)[0].contains("warning"));
    // A value that is no tone is dropped from reported text.
    let mut t = m.terminals[1].clone();
    t.agent = info(Some("evil"));
    plain_agent_text(&mut t);
    assert_eq!(t.agent.unwrap().tone, None);
}
