//! The command palette of the TUI (prefix, then the `palette` key): a filterable list of what the
//! TUI can do besides prefix keys, and the small dialogs behind an entry (a directory with
//! completion, a name, a confirmation, the trust question, the issue and mode of a task).
//!
//! This file is the pure part: [`Palette`] is the state, `key` turns a key into a [`Out`] (close,
//! or a [`Command`] the TUI carries out against the server), `draw` paints it. Nothing here talks
//! to the server or the disk, except the directory completion (`complete_dirs`).

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use poimen_core::github::OpenIssue;
use poimen_core::project::{Entries, Entry, Target};
use poimen_core::worktree::Issue;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear};

/// What an entry of the list does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NewSpace,
    RenameSpace,
    CloseSpace,
    NewTab,
    RenameTerminal,
    CloseTerminal,
    Project(Entry),
    StartTask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub action: Action,
}

/// What the entries of the list depend on.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// A space is focused: renaming and closing it make sense.
    pub space: bool,
    /// A terminal is focused.
    pub terminal: bool,
    /// The names in the project file of the focused space's repository.
    pub project: Entries,
}

/// The entries of the list, in the order they are shown without a filter.
pub fn items(ctx: &Context) -> Vec<Item> {
    let item = |label: &str, action| Item {
        label: label.to_owned(),
        action,
    };
    let mut out = vec![item("Space: New", Action::NewSpace)];
    if ctx.space {
        out.push(item("Space: Rename", Action::RenameSpace));
        out.push(item("Space: Close", Action::CloseSpace));
        out.push(item("Tab: New", Action::NewTab));
    }
    if ctx.terminal {
        out.push(item("Terminal: Rename", Action::RenameTerminal));
        out.push(item("Terminal: Close", Action::CloseTerminal));
    }
    if ctx.space {
        out.push(item("Task: Start from issue", Action::StartTask));
        for c in &ctx.project.commands {
            let target = match c.target {
                Target::Agent => " (agent)",
                Target::Terminal => "",
            };
            out.push(Item {
                label: format!("Project: {}{target}", c.name),
                action: Action::Project(Entry::Command {
                    name: c.name.clone(),
                }),
            });
        }
        for p in &ctx.project.presets {
            out.push(Item {
                label: format!("Preset: {p}"),
                action: Action::Project(Entry::Preset { name: p.clone() }),
            });
        }
    }
    out
}

/// Indices of `labels` that match `query`: every whitespace-separated word of the query is a
/// case-insensitive substring of the label. An empty query matches everything. Order is kept.
pub fn filter(labels: &[String], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    labels
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.to_lowercase();
            words.iter().all(|w| l.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}

/// What the user answered to the trust question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustChoice {
    /// Run the commands; valid for the shown hash.
    Trust,
    /// Build without the project's commands.
    Without,
}

/// The trust question as shown: the project file's hash and every command in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustPrompt {
    pub root: String,
    pub hash: String,
    /// `what: command` per line.
    pub commands: Vec<String>,
}

/// What the TUI carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    NewSpace {
        cwd: String,
    },
    RenameSpace(String),
    CloseSpace,
    NewTab,
    RenameTerminal(String),
    CloseTerminal,
    Project(Entry),
    /// Read the open issues and task modes of the focused space's repository.
    LoadIssues,
    StartTask {
        issue: Issue,
        mode: String,
    },
    Answer(TrustChoice),
}

/// The result of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Close,
    Run(Command),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    NewSpace,
    RenameSpace,
    RenameTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    List,
    Input {
        ask: Ask,
        text: String,
        /// Completions of a directory input.
        completions: Vec<String>,
        chosen: usize,
        /// The text is a suggestion: the first typed character replaces it (a directory input
        /// starts in the parent of the focused space), editing keys keep it.
        fresh: bool,
    },
    Confirm {
        text: String,
        then: Command,
    },
    Trust(TrustPrompt),
    /// Something runs in the background (reading issues, creating the worktree).
    Busy(String),
    Issues {
        all: Vec<OpenIssue>,
        modes: Vec<String>,
        query: String,
        selected: usize,
    },
    Modes {
        issue: Issue,
        modes: Vec<String>,
        selected: usize,
    },
}

#[derive(Debug, Clone)]
pub struct Palette {
    items: Vec<Item>,
    query: String,
    selected: usize,
    view: View,
    home: Option<PathBuf>,
    /// The focused space's directory and name, the defaults of the inputs.
    space_cwd: String,
    space_name: String,
    terminal_name: String,
}

/// The longest entry list the dialog shows.
const MAX_ROWS: usize = 10;

impl Palette {
    pub fn open(
        ctx: &Context,
        home: Option<&Path>,
        space: (&str, &str),
        terminal_name: &str,
    ) -> Self {
        Self {
            items: items(ctx),
            query: String::new(),
            selected: 0,
            view: View::List,
            home: home.map(Path::to_path_buf),
            space_cwd: space.0.to_owned(),
            space_name: space.1.to_owned(),
            terminal_name: terminal_name.to_owned(),
        }
    }

    /// The visible entries for the current filter.
    pub fn matches(&self) -> Vec<&Item> {
        let labels: Vec<String> = self.items.iter().map(|i| i.label.clone()).collect();
        filter(&labels, &self.query)
            .into_iter()
            .map(|i| &self.items[i])
            .collect()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.view, View::Busy(_))
    }

    pub fn show_busy(&mut self, text: &str) {
        self.view = View::Busy(text.to_owned());
    }

    pub fn show_trust(&mut self, prompt: TrustPrompt) {
        self.view = View::Trust(prompt);
    }

    /// The issues and task modes arrived.
    pub fn show_issues(&mut self, all: Vec<OpenIssue>, modes: Vec<String>) {
        self.view = View::Issues {
            all,
            modes,
            query: String::new(),
            selected: 0,
        };
    }

    /// Text typed or pasted while the palette is open (a paste is one line).
    pub fn paste(&mut self, text: &str) {
        let line = text.lines().next().unwrap_or_default();
        for c in line.chars() {
            self.type_char(c);
        }
    }

    fn type_char(&mut self, c: char) {
        match &mut self.view {
            View::List => {
                self.query.push(c);
                self.selected = 0;
            }
            View::Input { text, fresh, .. } => {
                if std::mem::take(fresh) {
                    text.clear();
                }
                text.push(c);
            }
            View::Issues {
                query, selected, ..
            } => {
                query.push(c);
                *selected = 0;
            }
            _ => {}
        }
        self.refresh_completions();
    }

    fn refresh_completions(&mut self) {
        let home = self.home.clone();
        if let View::Input {
            ask: Ask::NewSpace,
            text,
            completions,
            chosen,
            ..
        } = &mut self.view
        {
            *completions = complete_dirs(text, home.as_deref(), MAX_ROWS);
            *chosen = 0;
        }
    }

    fn backspace(&mut self) {
        match &mut self.view {
            View::List => {
                self.query.pop();
                self.selected = 0;
            }
            View::Input { text, fresh, .. } => {
                *fresh = false;
                text.pop();
            }
            View::Issues {
                query, selected, ..
            } => {
                query.pop();
                *selected = 0;
            }
            _ => {}
        }
        self.refresh_completions();
    }

    fn clear_line(&mut self) {
        match &mut self.view {
            View::List => self.query.clear(),
            View::Input { text, .. } => text.clear(),
            View::Issues { query, .. } => query.clear(),
            _ => {}
        }
        self.selected = 0;
        self.refresh_completions();
    }

    /// The issues that match the typed text: a number (with or without `#`), or words of the
    /// title or labels.
    pub fn issue_matches(all: &[OpenIssue], query: &str) -> Vec<usize> {
        let labels: Vec<String> = all
            .iter()
            .map(|i| format!("#{} {} {}", i.number, i.title, i.labels.join(" ")))
            .collect();
        let q = query.trim();
        let num = q.strip_prefix('#').unwrap_or(q);
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            let exact: Vec<usize> = all
                .iter()
                .enumerate()
                .filter(|(_, i)| i.number.to_string().starts_with(num))
                .map(|(i, _)| i)
                .collect();
            return exact;
        }
        filter(&labels, q)
    }

    fn move_selection(&mut self, delta: isize) {
        let len = match &self.view {
            View::List => self.matches().len(),
            View::Issues { all, query, .. } => Self::issue_matches(all, query).len(),
            View::Modes { modes, .. } => modes.len(),
            View::Input { completions, .. } => completions.len(),
            _ => return,
        };
        if len == 0 {
            return;
        }
        let step = |from: usize| (from as isize + delta).rem_euclid(len as isize) as usize;
        match &mut self.view {
            View::List => self.selected = step(self.selected),
            View::Issues { selected, .. } | View::Modes { selected, .. } => {
                *selected = step(*selected);
            }
            View::Input { chosen, .. } => *chosen = step(*chosen),
            _ => {}
        }
    }

    pub fn key(&mut self, key: &KeyEvent) -> Option<Out> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if matches!(self.view, View::Busy(_)) {
            return (key.code == KeyCode::Esc).then_some(Out::Close);
        }
        if let View::Trust(_) = &self.view {
            return match key.code {
                KeyCode::Char('t' | 'T') => Some(Out::Run(Command::Answer(TrustChoice::Trust))),
                KeyCode::Char('w' | 'W') => Some(Out::Run(Command::Answer(TrustChoice::Without))),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => Some(Out::Close),
                _ => None,
            };
        }
        if let View::Confirm { then, .. } = &self.view {
            return match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(Out::Run(then.clone())),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => Some(Out::Close),
                _ => None,
            };
        }
        match key.code {
            KeyCode::Esc => Some(Out::Close),
            KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('p') if ctrl => {
                self.move_selection(-1);
                None
            }
            KeyCode::Char('n') if ctrl => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('u') if ctrl => {
                self.clear_line();
                None
            }
            KeyCode::Backspace => {
                self.backspace();
                None
            }
            KeyCode::Tab => {
                self.complete();
                None
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Char(c) if !ctrl => {
                self.type_char(c);
                None
            }
            _ => None,
        }
    }

    /// Tab in a directory input: extends the text to the common prefix of the completions, and
    /// to the chosen one when there is nothing more in common.
    fn complete(&mut self) {
        let home = self.home.clone();
        let View::Input {
            ask: Ask::NewSpace,
            text,
            completions,
            chosen,
            fresh,
        } = &mut self.view
        else {
            return;
        };
        if completions.is_empty() {
            return;
        }
        let common = common_prefix(completions);
        let typed = expand_home(text, home.as_deref());
        let next = if common.chars().count() > typed.chars().count() {
            common
        } else {
            completions[*chosen % completions.len()].clone()
        };
        *fresh = false;
        *text = format!("{next}/");
        *completions = complete_dirs(text, home.as_deref(), MAX_ROWS);
        *chosen = 0;
    }

    fn enter(&mut self) -> Option<Out> {
        match &mut self.view {
            View::List => {
                let action = self
                    .matches()
                    .get(self.selected)
                    .map(|i| i.action.clone())?;
                self.start(action)
            }
            View::Input { ask, text, .. } => {
                let value = text.trim().to_owned();
                if value.is_empty() {
                    return None;
                }
                let command = match ask {
                    Ask::NewSpace => Command::NewSpace {
                        cwd: expand_home(&value, self.home.as_deref()),
                    },
                    Ask::RenameSpace => Command::RenameSpace(value),
                    Ask::RenameTerminal => Command::RenameTerminal(value),
                };
                Some(Out::Run(command))
            }
            View::Issues {
                all,
                modes,
                query,
                selected,
            } => {
                let at = *Self::issue_matches(all, query).get(*selected)?;
                let issue = Issue {
                    nr: all[at].number,
                    title: all[at].title.clone(),
                    url: all[at].url.clone(),
                };
                let modes = modes.clone();
                match modes.as_slice() {
                    [] => None,
                    [only] => Some(Out::Run(Command::StartTask {
                        issue,
                        mode: only.clone(),
                    })),
                    _ => {
                        self.view = View::Modes {
                            issue,
                            modes,
                            selected: 0,
                        };
                        None
                    }
                }
            }
            View::Modes {
                issue,
                modes,
                selected,
            } => Some(Out::Run(Command::StartTask {
                issue: issue.clone(),
                mode: modes.get(*selected)?.clone(),
            })),
            _ => None,
        }
    }

    fn start(&mut self, action: Action) -> Option<Out> {
        let input = |ask, text: String| View::Input {
            ask,
            text,
            completions: Vec::new(),
            chosen: 0,
            fresh: ask == Ask::NewSpace,
        };
        match action {
            Action::NewSpace => {
                let start = if self.space_cwd.is_empty() {
                    "~/".to_owned()
                } else {
                    let parent = Path::new(&self.space_cwd)
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    format!("{}/", parent.trim_end_matches('/'))
                };
                self.view = input(Ask::NewSpace, start);
                self.refresh_completions();
                None
            }
            Action::RenameSpace => {
                self.view = input(Ask::RenameSpace, self.space_name.clone());
                None
            }
            Action::RenameTerminal => {
                self.view = input(Ask::RenameTerminal, self.terminal_name.clone());
                None
            }
            Action::CloseSpace => {
                self.view = View::Confirm {
                    text: format!(
                        "Close space \"{}\" and all its terminals? [y/N]",
                        self.space_name
                    ),
                    then: Command::CloseSpace,
                };
                None
            }
            Action::CloseTerminal => {
                self.view = View::Confirm {
                    text: format!("Close terminal \"{}\"? [y/N]", self.terminal_name),
                    then: Command::CloseTerminal,
                };
                None
            }
            Action::NewTab => Some(Out::Run(Command::NewTab)),
            Action::Project(entry) => Some(Out::Run(Command::Project(entry))),
            Action::StartTask => {
                self.view = View::Busy("Reading the open issues…".into());
                Some(Out::Run(Command::LoadIssues))
            }
        }
    }
}

/// The longest common prefix of `names`.
fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut end = first.len();
    for n in &names[1..] {
        end = end.min(
            first
                .char_indices()
                .zip(n.chars())
                .take_while(|((_, a), b)| a == b)
                .last()
                .map(|((i, a), _)| i + a.len_utf8())
                .unwrap_or(0),
        );
    }
    first[..end].to_owned()
}

/// `~` and `~/…` expanded against `home`.
pub fn expand_home(input: &str, home: Option<&Path>) -> String {
    match (input, home) {
        ("~", Some(h)) => h.display().to_string(),
        (i, Some(h)) if i.starts_with("~/") => format!("{}/{}", h.display(), &i[2..]),
        _ => input.to_owned(),
    }
}

/// Directories matching a typed path, like the app's `list_dirs`: the subdirectories of the typed
/// directory whose name starts with the typed rest (`/a/b` lists the entries of `/a` that start
/// with `b`; `/a/` lists all of `/a`). Hidden ones only show when the rest starts with a dot.
/// Sorted, at most `limit`; empty for a relative path.
pub fn complete_dirs(input: &str, home: Option<&Path>, limit: usize) -> Vec<String> {
    let expanded = expand_home(input, home);
    if !Path::new(&expanded).is_absolute() {
        return Vec::new();
    }
    let Some(slash) = expanded.rfind(['/', std::path::MAIN_SEPARATOR]) else {
        return Vec::new();
    };
    let (parent, prefix) = (&expanded[..=slash], &expanded[slash + 1..]);
    let Ok(read) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
        .map(|n| format!("{parent}{n}"))
        .collect();
    dirs.sort();
    dirs.truncate(limit);
    dirs
}

/// The name of a space created for `cwd`: its last path segment.
pub fn space_name_of(cwd: &str) -> String {
    Path::new(cwd.trim_end_matches('/'))
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_owned())
}

/// The lines the dialog shows: a title line, an input line (empty when there is none) and the
/// rows with the selected one's index.
pub struct Lines {
    pub title: String,
    pub input: Option<String>,
    pub rows: Vec<String>,
    pub selected: usize,
}

impl Palette {
    pub fn lines(&self) -> Lines {
        let rows = |v: Vec<String>, selected: usize| (v, selected);
        let (title, input, (rows, selected)) = match &self.view {
            View::List => (
                "Command palette".to_owned(),
                Some(self.query.clone()),
                rows(
                    self.matches().iter().map(|i| i.label.clone()).collect(),
                    self.selected,
                ),
            ),
            View::Input {
                ask,
                text,
                completions,
                chosen,
                ..
            } => (
                match ask {
                    Ask::NewSpace => "New space: directory (Tab completes)",
                    Ask::RenameSpace => "Rename space",
                    Ask::RenameTerminal => "Rename terminal",
                }
                .to_owned(),
                Some(text.clone()),
                rows(completions.clone(), *chosen),
            ),
            View::Confirm { text, .. } => (text.clone(), None, rows(Vec::new(), 0)),
            View::Trust(p) => {
                let mut v = vec![format!("hash {}", p.hash)];
                v.extend(p.commands.iter().cloned());
                (
                    format!(
                        "Trust {}? It runs these commands. [t] trust  [w] without commands  [Esc] cancel",
                        p.root
                    ),
                    None,
                    rows(v, usize::MAX),
                )
            }
            View::Busy(text) => (text.clone(), None, rows(Vec::new(), 0)),
            View::Issues {
                all,
                query,
                selected,
                ..
            } => {
                let found = Self::issue_matches(all, query);
                let list = found
                    .iter()
                    .map(|&i| {
                        let it = &all[i];
                        match &it.level {
                            Some(l) => format!("#{} [{l}] {}", it.number, it.title),
                            None => format!("#{} {}", it.number, it.title),
                        }
                    })
                    .collect();
                (
                    format!("Start task: issue ({} open)", all.len()),
                    Some(query.clone()),
                    rows(list, *selected),
                )
            }
            View::Modes {
                issue,
                modes,
                selected,
            } => (
                format!("Start task #{}: mode", issue.nr),
                None,
                rows(modes.clone(), *selected),
            ),
        };
        Lines {
            title,
            input,
            rows,
            selected,
        }
    }
}

/// Paints the dialog over `area` (the whole window).
pub fn draw(f: &mut Frame, area: Rect, palette: &Palette) {
    let lines = palette.lines();
    let width = area.width.saturating_sub(4).clamp(1, 90);
    let shown = lines.rows.len().min(MAX_ROWS);
    let body = u16::try_from(shown).unwrap_or(0) + u16::from(lines.input.is_some());
    let height = (body + 3).min(area.height);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 4,
        width.min(area.width),
        height,
    );
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(lines.title.clone());
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let buf = f.buffer_mut();
    let mut y = inner.y;
    if let Some(input) = &lines.input {
        buf.set_stringn(
            inner.x,
            y,
            format!("> {input}"),
            usize::from(inner.width),
            Style::default(),
        );
        f.set_cursor_position((
            inner
                .x
                .saturating_add(u16::try_from(input.chars().count() + 2).unwrap_or(u16::MAX))
                .min(inner.right().saturating_sub(1)),
            y,
        ));
        y += 1;
    }
    let buf = f.buffer_mut();
    // Keep the selected row in view.
    let first = lines.selected.saturating_sub(shown.saturating_sub(1));
    let first = if lines.selected == usize::MAX {
        0
    } else {
        first
    };
    for (i, row) in lines.rows.iter().enumerate().skip(first).take(shown) {
        if y >= inner.bottom() {
            break;
        }
        let style = if i == lines.selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
        buf.set_stringn(inner.x, y, row, usize::from(inner.width), style);
        y += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_core::project::CommandInfo;

    fn ctx() -> Context {
        Context {
            space: true,
            terminal: true,
            project: Entries {
                commands: vec![
                    CommandInfo {
                        name: "test".into(),
                        target: Target::Terminal,
                    },
                    CommandInfo {
                        name: "review".into(),
                        target: Target::Agent,
                    },
                ],
                presets: vec!["duo".into()],
                locked: true,
            },
        }
    }

    fn press(p: &mut Palette, code: KeyCode) -> Option<Out> {
        p.key(&KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(p: &mut Palette, text: &str) {
        for c in text.chars() {
            press(p, KeyCode::Char(c));
        }
    }

    fn open(ctx: &Context) -> Palette {
        Palette::open(ctx, None, ("/work/app", "app"), "shell")
    }

    fn labels(p: &Palette) -> Vec<String> {
        p.matches().iter().map(|i| i.label.clone()).collect()
    }

    fn issue(n: u64, title: &str) -> OpenIssue {
        OpenIssue {
            number: n,
            title: title.into(),
            level: Some("pbi".into()),
            labels: vec!["pbi".into()],
            assignees: vec![],
            url: format!("https://github.com/o/r/issues/{n}"),
        }
    }

    #[test]
    fn the_list_has_the_space_terminal_and_project_entries() {
        assert_eq!(
            labels(&open(&ctx())),
            [
                "Space: New",
                "Space: Rename",
                "Space: Close",
                "Tab: New",
                "Terminal: Rename",
                "Terminal: Close",
                "Task: Start from issue",
                "Project: test",
                "Project: review (agent)",
                "Preset: duo",
            ]
        );
    }

    #[test]
    fn without_a_space_only_a_new_space_is_offered() {
        assert_eq!(labels(&open(&Context::default())), ["Space: New"]);
    }

    #[test]
    fn filtering_matches_every_word_in_any_case_and_keeps_the_order() {
        let mut p = open(&ctx());
        type_text(&mut p, "CLOSE sp");
        assert_eq!(labels(&p), ["Space: Close"]);
        let mut p = open(&ctx());
        type_text(&mut p, "pro");
        assert_eq!(labels(&p), ["Project: test", "Project: review (agent)"]);
        let mut p = open(&ctx());
        type_text(&mut p, "zzz");
        assert!(p.matches().is_empty());
        assert_eq!(press(&mut p, KeyCode::Enter), None);
    }

    #[test]
    fn backspace_and_ctrl_u_widen_the_filter_again() {
        let mut p = open(&ctx());
        type_text(&mut p, "preset");
        assert_eq!(labels(&p), ["Preset: duo"]);
        press(&mut p, KeyCode::Backspace);
        assert_eq!(p.query(), "prese");
        p.key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(labels(&p).len(), 10);
    }

    #[test]
    fn arrows_move_the_selection_and_wrap() {
        let mut p = open(&ctx());
        // Up from the first entry wraps to the last: the preset.
        press(&mut p, KeyCode::Up);
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::Project(Entry::Preset {
                name: "duo".into()
            })))
        );
        // Down moves on from the first entry.
        let mut p = open(&ctx());
        press(&mut p, KeyCode::Down);
        press(&mut p, KeyCode::Enter);
        assert_eq!(p.lines().title, "Rename space");
    }

    #[test]
    fn escape_closes() {
        let mut p = open(&ctx());
        assert_eq!(press(&mut p, KeyCode::Esc), Some(Out::Close));
    }

    #[test]
    fn a_project_entry_runs_the_entry() {
        let mut p = open(&ctx());
        type_text(&mut p, "review");
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::Project(Entry::Command {
                name: "review".into()
            })))
        );
    }

    #[test]
    fn rename_asks_for_the_name_prefilled_and_sends_it() {
        let mut p = open(&ctx());
        type_text(&mut p, "rename sp");
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        assert_eq!(p.lines().input.as_deref(), Some("app"));
        type_text(&mut p, "2");
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::RenameSpace("app2".into())))
        );
        // An empty name is not sent.
        let mut p = open(&ctx());
        type_text(&mut p, "rename te");
        press(&mut p, KeyCode::Enter);
        for _ in 0..5 {
            press(&mut p, KeyCode::Backspace);
        }
        assert_eq!(press(&mut p, KeyCode::Enter), None);
    }

    #[test]
    fn closing_asks_first() {
        let mut p = open(&ctx());
        type_text(&mut p, "close sp");
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        assert!(p.lines().title.contains("Close space \"app\""));
        assert_eq!(press(&mut p, KeyCode::Char('x')), None);
        assert_eq!(
            press(&mut p, KeyCode::Char('y')),
            Some(Out::Run(Command::CloseSpace))
        );
        let mut p = open(&ctx());
        type_text(&mut p, "close te");
        press(&mut p, KeyCode::Enter);
        assert_eq!(press(&mut p, KeyCode::Char('n')), Some(Out::Close));
    }

    struct Dirs(PathBuf);
    impl Dirs {
        fn new(name: &str) -> Self {
            let d =
                std::env::temp_dir().join(format!("poimen-palette-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            for sub in ["alpha", "alps", "beta", ".hidden"] {
                std::fs::create_dir_all(d.join(sub)).unwrap();
            }
            std::fs::write(d.join("alfile"), "").unwrap();
            Self(d)
        }
    }
    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn directory_completion_lists_matching_visible_directories_only() {
        let d = Dirs::new("list");
        let base = d.0.display().to_string();
        assert_eq!(
            complete_dirs(&format!("{base}/al"), None, 10),
            [format!("{base}/alpha"), format!("{base}/alps")]
        );
        assert_eq!(
            complete_dirs(&format!("{base}/"), None, 10),
            [
                format!("{base}/alpha"),
                format!("{base}/alps"),
                format!("{base}/beta")
            ]
        );
        assert_eq!(
            complete_dirs(&format!("{base}/."), None, 10),
            [format!("{base}/.hidden")]
        );
        assert_eq!(complete_dirs(&format!("{base}/"), None, 2).len(), 2);
        assert!(complete_dirs("relative/x", None, 10).is_empty());
        assert!(complete_dirs(&format!("{base}/nope/"), None, 10).is_empty());
    }

    #[test]
    fn home_is_expanded_in_completion_and_in_the_result() {
        let d = Dirs::new("home");
        let home = d.0.clone();
        assert_eq!(
            complete_dirs("~/be", Some(&home), 10),
            [format!("{}/beta", home.display())]
        );
        assert_eq!(expand_home("~", Some(&home)), home.display().to_string());
        assert_eq!(expand_home("~x", Some(&home)), "~x");
        assert_eq!(expand_home("~/a", None), "~/a");
    }

    #[test]
    fn new_space_completes_with_tab_and_sends_the_directory() {
        let d = Dirs::new("newspace");
        let base = d.0.display().to_string();
        let mut p = Palette::open(&ctx(), None, (&format!("{base}/beta"), "beta"), "sh");
        type_text(&mut p, "new sp");
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        // Starts in the parent of the focused space.
        assert_eq!(p.lines().input, Some(format!("{base}/")));
        // The suggestion is replaced by the first typed character.
        type_text(&mut p, &format!("{base}/alp"));
        assert_eq!(p.lines().rows.len(), 2);
        // `alpha` and `alps` share `alp`: Tab adds nothing, so it takes the chosen one.
        press(&mut p, KeyCode::Down);
        press(&mut p, KeyCode::Tab);
        assert_eq!(p.lines().input, Some(format!("{base}/alps/")));
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::NewSpace {
                cwd: format!("{base}/alps/")
            }))
        );
        assert_eq!(space_name_of(&format!("{base}/alps/")), "alps");
    }

    #[test]
    fn tab_extends_to_the_common_prefix() {
        let d = Dirs::new("common");
        let base = d.0.display().to_string();
        let mut p = Palette::open(&ctx(), None, (&format!("{base}/beta"), "beta"), "sh");
        type_text(&mut p, "new sp");
        press(&mut p, KeyCode::Enter);
        type_text(&mut p, &format!("{base}/a"));
        press(&mut p, KeyCode::Tab);
        assert_eq!(p.lines().input, Some(format!("{base}/alp/")));
    }

    #[test]
    fn common_prefix_handles_empty_one_and_diverging_names() {
        let s = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(common_prefix(&[]), "");
        assert_eq!(common_prefix(&s(&["abc"])), "abc");
        assert_eq!(common_prefix(&s(&["abc", "abd", "ab"])), "ab");
        assert_eq!(common_prefix(&s(&["x", "y"])), "");
    }

    #[test]
    fn the_trust_question_answers_with_t_w_or_cancels() {
        let prompt = TrustPrompt {
            root: "/work/app".into(),
            hash: "ab12".into(),
            commands: vec!["setup: make deps".into()],
        };
        let ask = || {
            let mut p = open(&ctx());
            p.show_trust(prompt.clone());
            p
        };
        let mut p = ask();
        let lines = p.lines();
        assert!(lines.title.contains("/work/app"));
        assert_eq!(lines.rows, ["hash ab12", "setup: make deps"]);
        assert_eq!(press(&mut p, KeyCode::Char('x')), None);
        assert_eq!(
            press(&mut p, KeyCode::Char('t')),
            Some(Out::Run(Command::Answer(TrustChoice::Trust)))
        );
        assert_eq!(
            press(&mut ask(), KeyCode::Char('w')),
            Some(Out::Run(Command::Answer(TrustChoice::Without)))
        );
        assert_eq!(press(&mut ask(), KeyCode::Esc), Some(Out::Close));
        // Enter is not an answer: trusting needs a deliberate key.
        assert_eq!(press(&mut ask(), KeyCode::Enter), None);
    }

    #[test]
    fn start_task_loads_issues_filters_them_and_picks_a_mode() {
        let mut p = open(&ctx());
        type_text(&mut p, "task");
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::LoadIssues))
        );
        assert!(p.is_busy());
        p.show_issues(
            vec![
                issue(12, "Add picker"),
                issue(40, "Fix the bug"),
                issue(120, "Docs"),
            ],
            vec!["agent".into(), "shell".into()],
        );
        assert_eq!(p.lines().rows.len(), 3);
        type_text(&mut p, "#12");
        // A number matches by prefix: 12 and 120.
        assert_eq!(p.lines().rows.len(), 2);
        press(&mut p, KeyCode::Down);
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        assert_eq!(p.lines().rows, ["agent", "shell"]);
        press(&mut p, KeyCode::Down);
        assert_eq!(
            press(&mut p, KeyCode::Enter),
            Some(Out::Run(Command::StartTask {
                issue: Issue {
                    nr: 120,
                    title: "Docs".into(),
                    url: "https://github.com/o/r/issues/120".into()
                },
                mode: "shell".into()
            }))
        );
    }

    #[test]
    fn one_mode_is_not_asked_and_words_filter_the_titles() {
        let mut p = open(&ctx());
        type_text(&mut p, "task");
        press(&mut p, KeyCode::Enter);
        p.show_issues(
            vec![issue(12, "Add picker"), issue(40, "Fix the bug")],
            vec!["agent".into()],
        );
        type_text(&mut p, "BUG");
        assert_eq!(p.lines().rows, ["#40 [pbi] Fix the bug"]);
        let out = press(&mut p, KeyCode::Enter);
        assert!(matches!(
            out,
            Some(Out::Run(Command::StartTask { ref mode, .. })) if mode == "agent"
        ));
    }

    #[test]
    fn no_matching_issue_and_no_mode_do_nothing() {
        let mut p = open(&ctx());
        p.show_issues(vec![issue(1, "a")], vec![]);
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        let mut p = open(&ctx());
        p.show_issues(vec![issue(1, "a")], vec!["agent".into()]);
        type_text(&mut p, "zzz");
        assert_eq!(press(&mut p, KeyCode::Enter), None);
    }

    #[test]
    fn while_busy_only_escape_works() {
        let mut p = open(&ctx());
        p.show_busy("Creating the worktree…");
        assert_eq!(press(&mut p, KeyCode::Char('a')), None);
        assert_eq!(press(&mut p, KeyCode::Enter), None);
        assert_eq!(press(&mut p, KeyCode::Esc), Some(Out::Close));
    }

    #[test]
    fn paste_goes_into_the_filter_as_one_line() {
        let mut p = open(&ctx());
        p.paste("close sp\nignored");
        assert_eq!(p.query(), "close sp");
    }
}
