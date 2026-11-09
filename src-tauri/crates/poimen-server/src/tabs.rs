//! The tabs and split trees of the spaces. Pure: no PTYs, no I/O. Every change returns the events
//! to publish; the server applies a change and publishes its events under the one lock it also
//! changes the terminals under, so the trees and the terminal set never disagree.
//!
//! Invariant (kept by the server): every leaf is a running terminal and every running terminal is
//! in exactly one leaf. A `split_id` is never given to another node (the id counter is persisted).

use poimen_protocol::{ErrorCode, Event, Node, Space, SplitDirection, Tab, valid_ratio};

pub(crate) type Failure = (ErrorCode, String);

fn not_found(what: &str, id: &str) -> Failure {
    (ErrorCode::NotFound, format!("no {what} {id}"))
}

fn bad_ratio(ratio: f64) -> Failure {
    (
        ErrorCode::InvalidParams,
        format!("ratio {ratio} must be above 0 and below 1"),
    )
}

/// The spaces with their tabs, as persisted in `server.json`.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Layout {
    pub(crate) spaces: Vec<Space>,
    pub(crate) tabs: Vec<Tab>,
}

/// A running terminal, as [`Layout::reconcile`] sees it.
pub(crate) struct Live<'a> {
    pub(crate) id: &'a str,
    pub(crate) space_id: &'a str,
    pub(crate) name: &'a str,
}

impl Layout {
    pub(crate) fn new(spaces: Vec<Space>, tabs: Vec<Tab>) -> Self {
        Self { spaces, tabs }
    }

    /// Every id of the layout (spaces, tabs, splits), for the id counter.
    pub(crate) fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.spaces.iter().map(|s| s.id.as_str()).collect();
        for tab in &self.tabs {
            ids.push(&tab.tab_id);
            ids.extend(tab.root.split_ids());
        }
        ids
    }

    pub(crate) fn space(&self, space_id: &str) -> Result<&Space, Failure> {
        self.spaces
            .iter()
            .find(|s| s.id == space_id)
            .ok_or_else(|| not_found("space", space_id))
    }

    fn space_mut(&mut self, space_id: &str) -> Result<&mut Space, Failure> {
        self.spaces
            .iter_mut()
            .find(|s| s.id == space_id)
            .ok_or_else(|| not_found("space", space_id))
    }

    pub(crate) fn tab(&self, tab_id: &str) -> Result<&Tab, Failure> {
        self.tabs
            .iter()
            .find(|t| t.tab_id == tab_id)
            .ok_or_else(|| not_found("tab", tab_id))
    }

    fn tab_mut(&mut self, tab_id: &str) -> Result<&mut Tab, Failure> {
        self.tabs
            .iter_mut()
            .find(|t| t.tab_id == tab_id)
            .ok_or_else(|| not_found("tab", tab_id))
    }

    /// The tab whose tree has a leaf of the terminal.
    pub(crate) fn tab_of(&self, terminal_id: &str) -> Option<&Tab> {
        self.tabs
            .iter()
            .find(|t| t.root.contains_terminal(terminal_id))
    }

    fn space_changed(&self, space_id: &str) -> Vec<Event> {
        self.space(space_id)
            .map(|space| Event::SpaceChanged {
                space: space.clone(),
            })
            .into_iter()
            .collect()
    }

    /// A new tab at the end of the space with one leaf, the terminal. It becomes the active tab
    /// only when the space had none.
    pub(crate) fn add_tab(
        &mut self,
        space_id: &str,
        tab_id: String,
        name: String,
        terminal_id: &str,
    ) -> Result<Vec<Event>, Failure> {
        let space = self.space_mut(space_id)?;
        space.tabs.push(tab_id.clone());
        if space.active_tab.is_none() {
            space.active_tab = Some(tab_id.clone());
        }
        let tab = Tab {
            tab_id,
            space_id: space_id.to_string(),
            name,
            root: Node::terminal(terminal_id),
            active_terminal: terminal_id.to_string(),
        };
        self.tabs.push(tab.clone());
        let mut events = vec![Event::TabCreated { tab }];
        events.extend(self.space_changed(space_id));
        Ok(events)
    }

    /// Removes the leaf of the terminal (closed or exited): its sibling takes the parent's place,
    /// and the last leaf of a tab closes the tab. Nothing when the terminal has no leaf.
    pub(crate) fn remove_terminal(&mut self, terminal_id: &str) -> Vec<Event> {
        let Some(index) = self
            .tabs
            .iter()
            .position(|t| t.root.contains_terminal(terminal_id))
        else {
            return Vec::new();
        };
        let tab = &mut self.tabs[index];
        let mut took_place = None;
        match without(tab.root.clone(), terminal_id, &mut took_place) {
            None => {
                let tab_id = tab.tab_id.clone();
                self.remove_tab(&tab_id).map(|(_, e)| e).unwrap_or_default()
            }
            Some(root) => {
                tab.root = root;
                if !tab.root.contains_terminal(&tab.active_terminal) {
                    tab.active_terminal =
                        took_place.unwrap_or_else(|| tab.root.first_terminal().to_string());
                }
                vec![Event::TabChanged { tab: tab.clone() }]
            }
        }
    }

    /// Takes the tab out of its space; when it was the active tab, the tab that took its place
    /// (else the one before it) becomes active.
    pub(crate) fn remove_tab(&mut self, tab_id: &str) -> Result<(Tab, Vec<Event>), Failure> {
        let index = self
            .tabs
            .iter()
            .position(|t| t.tab_id == tab_id)
            .ok_or_else(|| not_found("tab", tab_id))?;
        let tab = self.tabs.remove(index);
        let mut events = vec![Event::TabClosed {
            tab_id: tab.tab_id.clone(),
            space_id: tab.space_id.clone(),
        }];
        if let Ok(space) = self.space_mut(&tab.space_id) {
            if let Some(at) = space.tabs.iter().position(|t| *t == tab.tab_id) {
                space.tabs.remove(at);
                if space.active_tab.as_deref() == Some(tab_id) {
                    space.active_tab = space.tabs.get(at).or_else(|| space.tabs.last()).cloned();
                }
            }
            events.extend(self.space_changed(&tab.space_id));
        }
        Ok((tab, events))
    }

    /// Removes the space and its tabs (`tab.closed` each); the terminals are the caller's.
    pub(crate) fn remove_space(&mut self, space_id: &str) -> Result<Vec<Event>, Failure> {
        let index = self
            .spaces
            .iter()
            .position(|s| s.id == space_id)
            .ok_or_else(|| not_found("space", space_id))?;
        self.spaces.remove(index);
        let (closing, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tabs)
            .into_iter()
            .partition(|t| t.space_id == space_id);
        self.tabs = keep;
        Ok(closing
            .into_iter()
            .map(|t| Event::TabClosed {
                tab_id: t.tab_id,
                space_id: t.space_id,
            })
            .collect())
    }

    /// Splits the leaf of `terminal_id` into a split `split_id`: the old leaf first, the new
    /// terminal second. The focus stays where it was.
    pub(crate) fn split(
        &mut self,
        terminal_id: &str,
        new_terminal_id: &str,
        direction: SplitDirection,
        ratio: f64,
        split_id: String,
    ) -> Result<Vec<Event>, Failure> {
        if !valid_ratio(ratio) {
            return Err(bad_ratio(ratio));
        }
        let tab = self
            .tabs
            .iter_mut()
            .find(|t| t.root.contains_terminal(terminal_id))
            .ok_or_else(|| not_found("leaf of terminal", terminal_id))?;
        let leaf = leaf_mut(&mut tab.root, terminal_id)
            .ok_or_else(|| not_found("leaf of terminal", terminal_id))?;
        *leaf = Node::Split {
            split_id,
            direction,
            ratio,
            children: Box::new([Node::terminal(terminal_id), Node::terminal(new_terminal_id)]),
        };
        Ok(vec![Event::TabChanged { tab: tab.clone() }])
    }

    /// Sets the ratio of the split `split_id` of the tab; an unknown split is an error, never
    /// a guess.
    pub(crate) fn resize_split(
        &mut self,
        tab_id: &str,
        split_id: &str,
        ratio: f64,
    ) -> Result<Vec<Event>, Failure> {
        if !valid_ratio(ratio) {
            return Err(bad_ratio(ratio));
        }
        let tab = self.tab_mut(tab_id)?;
        let Some(Node::Split { ratio: current, .. }) = split_mut(&mut tab.root, split_id) else {
            return Err(not_found(&format!("split in tab {tab_id}:"), split_id));
        };
        *current = ratio;
        Ok(vec![Event::TabChanged { tab: tab.clone() }])
    }

    pub(crate) fn rename(&mut self, tab_id: &str, name: String) -> Result<Vec<Event>, Failure> {
        let tab = self.tab_mut(tab_id)?;
        tab.name = name;
        Ok(vec![Event::TabChanged { tab: tab.clone() }])
    }

    /// Moves the tab to `index` of its space's order; past the end it goes last.
    pub(crate) fn move_tab(&mut self, tab_id: &str, index: usize) -> Result<Vec<Event>, Failure> {
        let tab = self.tab(tab_id)?.clone();
        let space = self.space_mut(&tab.space_id)?;
        space.tabs.retain(|t| *t != tab.tab_id);
        let index = index.min(space.tabs.len());
        space.tabs.insert(index, tab.tab_id.clone());
        let mut events = vec![Event::TabChanged { tab: tab.clone() }];
        events.extend(self.space_changed(&tab.space_id));
        Ok(events)
    }

    /// Makes the tab its space's active tab and, when given, the terminal (a leaf of the tab) its
    /// active terminal. Shared by all clients: the last call wins.
    pub(crate) fn focus(
        &mut self,
        tab_id: &str,
        terminal_id: Option<&str>,
    ) -> Result<Vec<Event>, Failure> {
        let tab = self.tab_mut(tab_id)?;
        if let Some(terminal_id) = terminal_id {
            if !tab.root.contains_terminal(terminal_id) {
                return Err(not_found(
                    &format!("leaf in tab {tab_id} of terminal"),
                    terminal_id,
                ));
            }
            tab.active_terminal = terminal_id.to_string();
        }
        let tab = tab.clone();
        self.space_mut(&tab.space_id)?.active_tab = Some(tab.tab_id.clone());
        let mut events = vec![Event::TabChanged { tab: tab.clone() }];
        events.extend(self.space_changed(&tab.space_id));
        Ok(events)
    }

    /// Brings a loaded layout in line with the running terminals: drops tabs of unknown spaces,
    /// leaves of terminals that are not running in the tab's space (or already have a leaf) and
    /// the splits they leave behind, empty tabs, tab ids a space lists twice or that do not
    /// exist, fixes the focus, and gives every running terminal without a leaf a tab of its own
    /// (the protocol 3 mapping of an upgraded `server.json`). True when anything changed.
    pub(crate) fn reconcile(
        &mut self,
        live: &[Live<'_>],
        new_id: &mut dyn FnMut(&str) -> String,
    ) -> bool {
        let before = self.clone();
        let spaces: Vec<String> = self.spaces.iter().map(|s| s.id.clone()).collect();
        let mut placed: Vec<String> = Vec::new();
        let mut tabs = Vec::new();
        for mut tab in std::mem::take(&mut self.tabs) {
            if !spaces.contains(&tab.space_id) || tabs.iter().any(|t: &Tab| t.tab_id == tab.tab_id)
            {
                continue;
            }
            // A terminal listed twice in one tree loses both leaves (and gets a tab of its own).
            let mut seen: Vec<&str> = Vec::new();
            let mut dropped: Vec<String> = Vec::new();
            for id in tab.root.terminal_ids() {
                let running = live
                    .iter()
                    .any(|t| t.id == id && t.space_id == tab.space_id);
                if running && !placed.iter().any(|p| p == id) && !seen.contains(&id) {
                    seen.push(id);
                } else {
                    dropped.push(id.to_string());
                }
            }
            let mut root = Some(tab.root.clone());
            for id in &dropped {
                root = root.and_then(|r| without(r, id, &mut None));
            }
            let Some(root) = root else { continue };
            tab.root = root;
            if !tab.root.contains_terminal(&tab.active_terminal) {
                tab.active_terminal = tab.root.first_terminal().to_string();
            }
            placed.extend(tab.root.terminal_ids().into_iter().map(str::to_string));
            tabs.push(tab);
        }
        self.tabs = tabs;
        for space in &mut self.spaces {
            let own: Vec<&str> = self
                .tabs
                .iter()
                .filter(|t| t.space_id == space.id)
                .map(|t| t.tab_id.as_str())
                .collect();
            let mut order: Vec<String> = Vec::new();
            for id in &space.tabs {
                if own.contains(&id.as_str()) && !order.contains(id) {
                    order.push(id.clone());
                }
            }
            for id in own {
                if !order.iter().any(|o| o == id) {
                    order.push(id.to_string());
                }
            }
            space.tabs = order;
            if !space
                .active_tab
                .as_ref()
                .is_some_and(|a| space.tabs.contains(a))
            {
                space.active_tab = space.tabs.first().cloned();
            }
        }
        for terminal in live {
            if placed.iter().any(|p| p == terminal.id) {
                continue;
            }
            let tab_id = new_id("tab");
            // A terminal of an unknown space stays without a tab; it has no space to show in.
            let _ = self.add_tab(
                terminal.space_id,
                tab_id,
                terminal.name.to_string(),
                terminal.id,
            );
        }
        *self != before
    }
}

/// The tree without the leaf of `terminal_id`: the leaf's sibling takes the parent's place.
/// `None` when nothing is left. `took_place` gets the first leaf of the node that moved up.
fn without(node: Node, terminal_id: &str, took_place: &mut Option<String>) -> Option<Node> {
    match node {
        Node::Terminal {
            terminal_id: ref id,
        } if id == terminal_id => None,
        Node::Split {
            split_id,
            direction,
            ratio,
            children,
        } => {
            let [first, second] = *children;
            match (
                without(first, terminal_id, took_place),
                without(second, terminal_id, took_place),
            ) {
                (Some(a), Some(b)) => Some(Node::Split {
                    split_id,
                    direction,
                    ratio,
                    children: Box::new([a, b]),
                }),
                (Some(only), None) | (None, Some(only)) => {
                    took_place.get_or_insert_with(|| only.first_terminal().to_string());
                    Some(only)
                }
                (None, None) => None,
            }
        }
        leaf => Some(leaf),
    }
}

fn leaf_mut<'a>(node: &'a mut Node, terminal_id: &str) -> Option<&'a mut Node> {
    if matches!(node, Node::Terminal { terminal_id: id } if id == terminal_id) {
        return Some(node);
    }
    match node {
        Node::Split { children, .. } => {
            let [first, second] = &mut **children;
            leaf_mut(first, terminal_id).or_else(|| leaf_mut(second, terminal_id))
        }
        Node::Terminal { .. } => None,
    }
}

fn split_mut<'a>(node: &'a mut Node, split_id: &str) -> Option<&'a mut Node> {
    if matches!(node, Node::Split { split_id: id, .. } if id == split_id) {
        return Some(node);
    }
    match node {
        Node::Split { children, .. } => {
            let [first, second] = &mut **children;
            split_mut(first, split_id).or_else(|| split_mut(second, split_id))
        }
        Node::Terminal { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_protocol::SplitDirection::{Down, Right};

    fn space(id: &str) -> Space {
        Space {
            id: id.into(),
            name: id.into(),
            cwd: "/x".into(),
            ..Default::default()
        }
    }

    fn leaf(id: &str) -> Node {
        Node::terminal(id)
    }

    fn split(id: &str, direction: SplitDirection, ratio: f64, a: Node, b: Node) -> Node {
        Node::Split {
            split_id: id.into(),
            direction,
            ratio,
            children: Box::new([a, b]),
        }
    }

    /// A layout with one space `s1` and a tab `tab1` holding `t1`.
    fn one_tab() -> Layout {
        let mut layout = Layout::new(vec![space("s1")], Vec::new());
        layout
            .add_tab("s1", "tab1".into(), "sh".into(), "t1")
            .unwrap();
        layout
    }

    fn names(events: &[Event]) -> Vec<&'static str> {
        events
            .iter()
            .map(|e| match e {
                Event::TabCreated { .. } => "tab.created",
                Event::TabChanged { .. } => "tab.changed",
                Event::TabClosed { .. } => "tab.closed",
                Event::SpaceChanged { .. } => "space.changed",
                _ => "other",
            })
            .collect()
    }

    fn code(r: Result<Vec<Event>, Failure>) -> ErrorCode {
        r.expect_err("expected an error").0
    }

    #[test]
    fn a_new_tab_goes_last_and_is_active_only_in_a_space_without_one() {
        let mut layout = one_tab();
        let events = layout
            .add_tab("s1", "tab2".into(), "logs".into(), "t2")
            .unwrap();
        assert_eq!(names(&events), ["tab.created", "space.changed"]);
        let space = layout.space("s1").unwrap();
        assert_eq!(space.tabs, ["tab1", "tab2"]);
        assert_eq!(space.active_tab.as_deref(), Some("tab1"));
        let tab = layout.tab("tab2").unwrap();
        assert_eq!(
            (tab.root.clone(), tab.active_terminal.as_str()),
            (leaf("t2"), "t2")
        );
        assert_eq!(
            layout
                .add_tab("s9", "tab3".into(), "x".into(), "t3")
                .unwrap_err()
                .0,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_split_puts_the_old_leaf_first_and_keeps_the_focus() {
        let mut layout = one_tab();
        let events = layout
            .split("t1", "t2", Right, 0.3, "split5".into())
            .unwrap();
        assert_eq!(names(&events), ["tab.changed"]);
        let tab = layout.tab("tab1").unwrap();
        assert_eq!(
            tab.root,
            split("split5", Right, 0.3, leaf("t1"), leaf("t2"))
        );
        assert_eq!(tab.active_terminal, "t1");
        // A nested split replaces only the leaf it splits.
        layout
            .split("t2", "t3", Down, 0.5, "split6".into())
            .unwrap();
        assert_eq!(
            layout.tab("tab1").unwrap().root,
            split(
                "split5",
                Right,
                0.3,
                leaf("t1"),
                split("split6", Down, 0.5, leaf("t2"), leaf("t3"))
            )
        );
    }

    #[test]
    fn a_split_of_a_terminal_without_a_leaf_or_with_a_bad_ratio_fails() {
        let mut layout = one_tab();
        assert_eq!(
            code(layout.split("t9", "t2", Right, 0.5, "split2".into())),
            ErrorCode::NotFound
        );
        for ratio in [0.0, 1.0, 1.5, f64::NAN] {
            assert_eq!(
                code(layout.split("t1", "t2", Right, ratio, "split2".into())),
                ErrorCode::InvalidParams
            );
        }
        assert_eq!(layout, one_tab());
    }

    #[test]
    fn resize_finds_the_split_by_id_also_after_a_sibling_split_moved_it() {
        let mut layout = one_tab();
        layout
            .split("t1", "t2", Right, 0.5, "split5".into())
            .unwrap();
        // Another client splits the first child: the split of t1 and t2 is now one level down,
        // a path would point at the new split.
        layout
            .split("t1", "t3", Down, 0.5, "split6".into())
            .unwrap();
        let events = layout.resize_split("tab1", "split5", 0.8).unwrap();
        assert_eq!(names(&events), ["tab.changed"]);
        assert_eq!(
            layout.tab("tab1").unwrap().root,
            split(
                "split5",
                Right,
                0.8,
                split("split6", Down, 0.5, leaf("t1"), leaf("t3")),
                leaf("t2")
            )
        );
        layout.resize_split("tab1", "split6", 0.25).unwrap();
        match &layout.tab("tab1").unwrap().root {
            Node::Split { children, .. } => {
                assert_eq!(
                    children[0],
                    split("split6", Down, 0.25, leaf("t1"), leaf("t3"))
                )
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn resize_of_an_unknown_split_tab_or_ratio_is_an_error_and_changes_nothing() {
        let mut layout = one_tab();
        layout
            .split("t1", "t2", Right, 0.5, "split5".into())
            .unwrap();
        let before = layout.clone();
        assert_eq!(
            code(layout.resize_split("tab1", "split9", 0.5)),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(layout.resize_split("tab9", "split5", 0.5)),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(layout.resize_split("tab1", "split5", 0.0)),
            ErrorCode::InvalidParams
        );
        assert_eq!(layout, before);
        // A split of another tab is unknown in this one.
        layout
            .add_tab("s1", "tab2".into(), "b".into(), "t7")
            .unwrap();
        assert_eq!(
            code(layout.resize_split("tab2", "split5", 0.5)),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn removing_a_leaf_lets_its_sibling_take_the_parents_place() {
        let mut layout = one_tab();
        layout
            .split("t1", "t2", Right, 0.5, "split5".into())
            .unwrap();
        layout
            .split("t2", "t3", Down, 0.5, "split6".into())
            .unwrap();
        layout.focus("tab1", Some("t2")).unwrap();
        let events = layout.remove_terminal("t2");
        assert_eq!(names(&events), ["tab.changed"]);
        let tab = layout.tab("tab1").unwrap();
        assert_eq!(
            tab.root,
            split("split5", Right, 0.5, leaf("t1"), leaf("t3"))
        );
        // The focus moves to the node that took the place.
        assert_eq!(tab.active_terminal, "t3");
        layout.remove_terminal("t1");
        let tab = layout.tab("tab1").unwrap();
        assert_eq!(tab.root, leaf("t3"));
        assert_eq!(tab.active_terminal, "t3");
        assert!(layout.remove_terminal("t9").is_empty());
    }

    #[test]
    fn removing_an_unfocused_leaf_keeps_the_focus() {
        let mut layout = one_tab();
        layout
            .split("t1", "t2", Right, 0.5, "split5".into())
            .unwrap();
        layout.remove_terminal("t2");
        let tab = layout.tab("tab1").unwrap();
        assert_eq!(
            (tab.root.clone(), tab.active_terminal.as_str()),
            (leaf("t1"), "t1")
        );
    }

    #[test]
    fn the_last_leaf_closes_the_tab_and_the_focus_moves_to_the_neighbour() {
        let mut layout = one_tab();
        layout
            .add_tab("s1", "tab2".into(), "b".into(), "t2")
            .unwrap();
        layout
            .add_tab("s1", "tab3".into(), "c".into(), "t3")
            .unwrap();
        layout.focus("tab2", None).unwrap();
        let events = layout.remove_terminal("t2");
        assert_eq!(names(&events), ["tab.closed", "space.changed"]);
        let space = layout.space("s1").unwrap();
        assert_eq!(space.tabs, ["tab1", "tab3"]);
        // The tab that took the place, else the one before.
        assert_eq!(space.active_tab.as_deref(), Some("tab3"));
        layout.remove_terminal("t3");
        assert_eq!(
            layout.space("s1").unwrap().active_tab.as_deref(),
            Some("tab1")
        );
        layout.remove_terminal("t1");
        let space = layout.space("s1").unwrap();
        assert!(space.tabs.is_empty());
        assert_eq!(space.active_tab, None);
        assert!(layout.tabs.is_empty());
    }

    #[test]
    fn move_reorders_and_clamps_past_the_end() {
        let mut layout = one_tab();
        layout
            .add_tab("s1", "tab2".into(), "b".into(), "t2")
            .unwrap();
        layout
            .add_tab("s1", "tab3".into(), "c".into(), "t3")
            .unwrap();
        let events = layout.move_tab("tab3", 0).unwrap();
        assert_eq!(names(&events), ["tab.changed", "space.changed"]);
        assert_eq!(layout.space("s1").unwrap().tabs, ["tab3", "tab1", "tab2"]);
        layout.move_tab("tab3", 99).unwrap();
        assert_eq!(layout.space("s1").unwrap().tabs, ["tab1", "tab2", "tab3"]);
        layout.move_tab("tab1", 1).unwrap();
        assert_eq!(layout.space("s1").unwrap().tabs, ["tab2", "tab1", "tab3"]);
        assert_eq!(code(layout.move_tab("tab9", 0)), ErrorCode::NotFound);
    }

    #[test]
    fn focus_sets_the_active_tab_and_leaf_and_refuses_a_foreign_leaf() {
        let mut layout = one_tab();
        layout
            .add_tab("s1", "tab2".into(), "b".into(), "t2")
            .unwrap();
        layout
            .split("t2", "t3", Down, 0.5, "split4".into())
            .unwrap();
        let events = layout.focus("tab2", Some("t3")).unwrap();
        assert_eq!(names(&events), ["tab.changed", "space.changed"]);
        assert_eq!(
            layout.space("s1").unwrap().active_tab.as_deref(),
            Some("tab2")
        );
        assert_eq!(layout.tab("tab2").unwrap().active_terminal, "t3");
        // The last focus wins; without a terminal the tab's leaf stays.
        layout.focus("tab1", None).unwrap();
        layout.focus("tab2", None).unwrap();
        assert_eq!(layout.tab("tab2").unwrap().active_terminal, "t3");
        let before = layout.clone();
        assert_eq!(code(layout.focus("tab2", Some("t1"))), ErrorCode::NotFound);
        assert_eq!(code(layout.focus("tab9", None)), ErrorCode::NotFound);
        assert_eq!(layout, before);
    }

    #[test]
    fn rename_changes_only_the_name() {
        let mut layout = one_tab();
        let events = layout.rename("tab1", "build".into()).unwrap();
        assert_eq!(names(&events), ["tab.changed"]);
        assert_eq!(layout.tab("tab1").unwrap().name, "build");
        assert_eq!(code(layout.rename("tab9", "x".into())), ErrorCode::NotFound);
    }

    #[test]
    fn removing_a_space_closes_its_tabs_only() {
        let mut layout = one_tab();
        layout.spaces.push(space("s2"));
        layout
            .add_tab("s2", "tab2".into(), "b".into(), "t2")
            .unwrap();
        let events = layout.remove_space("s1").unwrap();
        assert_eq!(names(&events), ["tab.closed"]);
        assert_eq!(layout.tabs.len(), 1);
        assert_eq!(layout.tabs[0].tab_id, "tab2");
        assert_eq!(
            layout.remove_space("s1").unwrap_err().0,
            ErrorCode::NotFound
        );
    }

    fn ids() -> impl FnMut(&str) -> String {
        let mut n = 100;
        move |prefix| {
            n += 1;
            format!("{prefix}{n}")
        }
    }

    fn live<'a>(id: &'a str, space_id: &'a str) -> Live<'a> {
        Live {
            id,
            space_id,
            name: "sh",
        }
    }

    /// `s1` with `tab1` (t1 | (t2 / t3), focus t2) and `tab2` (t4).
    fn persisted() -> Layout {
        let mut s1 = space("s1");
        s1.tabs = vec!["tab1".into(), "tab2".into()];
        s1.active_tab = Some("tab2".into());
        Layout::new(
            vec![s1],
            vec![
                Tab {
                    tab_id: "tab1".into(),
                    space_id: "s1".into(),
                    name: "dev".into(),
                    root: split(
                        "split5",
                        Right,
                        0.3,
                        leaf("t1"),
                        split("split6", Down, 0.6, leaf("t2"), leaf("t3")),
                    ),
                    active_terminal: "t2".into(),
                },
                Tab {
                    tab_id: "tab2".into(),
                    space_id: "s1".into(),
                    name: "logs".into(),
                    root: leaf("t4"),
                    active_terminal: "t4".into(),
                },
            ],
        )
    }

    #[test]
    fn reconcile_drops_dangling_leaves_collapses_their_splits_and_drops_empty_tabs() {
        let mut layout = persisted();
        // t1 and t3 survived (an adopted PTY), t2 and t4 are gone.
        let changed = layout.reconcile(&[live("t1", "s1"), live("t3", "s1")], &mut ids());
        assert!(changed);
        assert_eq!(layout.tabs.len(), 1);
        let tab = &layout.tabs[0];
        assert_eq!(
            tab.root,
            split("split5", Right, 0.3, leaf("t1"), leaf("t3"))
        );
        assert_eq!(tab.active_terminal, "t1");
        let space = layout.space("s1").unwrap();
        assert_eq!(space.tabs, ["tab1"]);
        assert_eq!(space.active_tab.as_deref(), Some("tab1"));
    }

    #[test]
    fn reconcile_without_terminals_keeps_the_spaces_and_drops_every_tab() {
        let mut layout = persisted();
        assert!(layout.reconcile(&[], &mut ids()));
        assert!(layout.tabs.is_empty());
        let space = layout.space("s1").unwrap();
        assert!(space.tabs.is_empty());
        assert_eq!(space.active_tab, None);
        // A consistent layout stays as it is.
        let mut kept = persisted();
        let all = [
            live("t1", "s1"),
            live("t2", "s1"),
            live("t3", "s1"),
            live("t4", "s1"),
        ];
        assert!(!kept.reconcile(&all, &mut ids()));
        assert_eq!(kept, persisted());
    }

    #[test]
    fn reconcile_gives_each_terminal_without_a_leaf_a_tab_of_its_own() {
        // A protocol 3 file: spaces without tabs, and two running terminals.
        let mut layout = Layout::new(vec![space("s1"), space("s2")], Vec::new());
        let changed = layout.reconcile(
            &[
                live("t1", "s1"),
                live("t2", "s1"),
                live("t3", "s2"),
                live("t4", "gone"),
            ],
            &mut ids(),
        );
        assert!(changed);
        let s1 = layout.space("s1").unwrap();
        assert_eq!(s1.tabs, ["tab101", "tab102"]);
        assert_eq!(s1.active_tab.as_deref(), Some("tab101"));
        assert_eq!(layout.tab("tab102").unwrap().root, leaf("t2"));
        assert_eq!(layout.tab("tab102").unwrap().name, "sh");
        assert_eq!(layout.space("s2").unwrap().tabs, ["tab103"]);
        // A terminal of an unknown space gets no tab.
        assert!(layout.tab_of("t4").is_none());
    }

    #[test]
    fn reconcile_repairs_tab_lists_foreign_spaces_and_doubled_leaves() {
        let mut layout = persisted();
        // tab1 listed twice, an unknown tab listed, tab2 not listed, the focus on a gone tab.
        layout.spaces[0].tabs = vec!["tab1".into(), "tab9".into(), "tab1".into()];
        layout.spaces[0].active_tab = Some("tab9".into());
        // t1 also has a leaf in tab2, and a tab of an unknown space.
        layout.tabs[1].root = split("split7", Down, 0.5, leaf("t4"), leaf("t1"));
        layout.tabs.push(Tab {
            tab_id: "tab3".into(),
            space_id: "s9".into(),
            name: "x".into(),
            root: leaf("t8"),
            active_terminal: "t8".into(),
        });
        let all = [
            live("t1", "s1"),
            live("t2", "s1"),
            live("t3", "s1"),
            live("t4", "s1"),
            live("t8", "s9"),
        ];
        assert!(layout.reconcile(&all, &mut ids()));
        let space = layout.space("s1").unwrap();
        assert_eq!(space.tabs, ["tab1", "tab2"]);
        assert_eq!(space.active_tab.as_deref(), Some("tab1"));
        // The first leaf of t1 wins; its second is dropped and its split collapses.
        assert_eq!(layout.tab("tab2").unwrap().root, leaf("t4"));
        assert!(layout.tab("tab3").is_err());
        // Every running terminal of a known space is in exactly one leaf.
        let mut leaves: Vec<&str> = layout
            .tabs
            .iter()
            .flat_map(|t| t.root.terminal_ids())
            .collect();
        leaves.sort_unstable();
        assert_eq!(leaves, ["t1", "t2", "t3", "t4"]);
    }

    #[test]
    fn the_ids_of_a_layout_include_tabs_and_splits() {
        let layout = persisted();
        assert_eq!(layout.ids(), ["s1", "tab1", "split5", "split6", "tab2"]);
    }
}
