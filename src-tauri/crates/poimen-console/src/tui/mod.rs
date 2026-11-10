//! The TUI of `poimen` without arguments (ADR 0006): a sidebar with the spaces, tabs and
//! terminals of the server, and the selected terminal drawn from a grid of its own.
//!
//! The server owns every state: the sidebar is read from `space.list`/`terminal.list` and read
//! again on every event of an `events.subscribe` connection (as a `tui` client). The selected
//! terminal is the focused pane of its tab: the tab bar lists the tabs of its space and the tab's
//! split tree is drawn with borders. Every visible pane has an attachment and a grid of its own;
//! only the focused one attaches as controller (when another client holds the control it
//! observes and asks before it takes over), the others observe. Tabs and splits are changed
//! with prefix keys through the server's `tab.*` requests; the view follows the events (a tab
//! changed in the app shows up here at once). Focus is the server's too: a focus change sends
//! `tab.focus`, and a focus change made elsewhere moves the focus here. Detaching (prefix, then
//! `d`) ends the attachments with `terminal.detach`: no terminal is closed, the server is never
//! stopped.
//!
//! `config` reads the prefix and keys, `input` routes keys, `grid` is a pane's emulator,
//! `sidebar` the row model, `tree` the rectangles of a split tree, `screen` the local terminal
//! modes, `view` draws a frame.

pub mod config;
pub mod grid;
pub mod input;
pub mod screen;
pub mod sidebar;
pub mod tree;
pub mod view;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::event::{Event as InputEvent, KeyCode, KeyEventKind};
use poimen_client::protocol::{
    AttachMode, ClientInfo, ClientKind, DetachReason, ErrorCode, Event, SplitDirection, TabCreate,
    TabSplit, TerminalStatus, decode_bytes,
};
use poimen_client::{AttachmentWriter, Client, ClientError};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;

use crate::session::Error;
use config::Config;
use grid::{Grid, Palette};
use input::{Routed, Router, Wheel};
use ratatui::layout::Rect;
use sidebar::{Model, Row};
use tree::Dir;

/// How often the stop flag is looked at while nothing happens.
const TICK: Duration = Duration::from_millis(100);

pub struct Options {
    pub config: Config,
    /// The home directory, shown as `~` in paths.
    pub home: Option<String>,
    /// The terminal the TUI itself runs in (`POIMEN_TERMINAL_ID`): never attached to.
    pub own_terminal: Option<String>,
    /// Receives a copy of every drawn frame (tests read the screen through it).
    pub mirror: Option<Arc<Mutex<Buffer>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The detach key; every terminal keeps running.
    Detached,
    /// `stop` was set (a signal).
    Interrupted,
    /// The event connection to the server ended.
    ServerGone(String),
}

enum Msg {
    Input(InputEvent),
    /// Something changed on the server: read the model again.
    Changed,
    ServerGone(String),
    /// A stream record of the attachment with this generation.
    Record(u64, Box<Result<Event, String>>),
}

/// The attached terminal.
pub struct Pane {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub grid: Grid,
    /// The terminal ended (with its exit code).
    pub exited: Option<Option<u32>>,
    writer: AttachmentWriter,
    generation: u64,
    /// The PTY size last sent as controller.
    sent_size: Option<(u16, u16)>,
    /// Observing because another client has the control (asked about or taken from us): a
    /// refresh does not try to control it again.
    sticky: bool,
}

impl Pane {
    pub fn controls(&self) -> bool {
        self.mode == AttachMode::Control && self.exited.is_none()
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        // `terminal.detach`: the terminal keeps running.
        self.writer.close();
    }
}

/// The state a frame is drawn from.
pub struct App {
    pub config: Config,
    pub palette: Palette,
    pub home: Option<String>,
    pub own_terminal: Option<String>,
    pub model: Model,
    pub rows: Vec<Row>,
    pub selected: Option<String>,
    /// One per visible terminal (the leaves of the selected terminal's tab).
    pub panes: Vec<Pane>,
    pub sidebar: bool,
    /// Asking whether to take over the control of this terminal.
    pub take_over: Option<String>,
    /// A message for the status line (errors, what happened).
    pub notice: Option<String>,
    pub prefix_pending: bool,
    generation: u64,
}

impl App {
    fn new(opts: &Options) -> Self {
        Self {
            palette: Palette::from_scheme(opts.config.scheme.as_ref()),
            config: opts.config.clone(),
            home: opts.home.clone(),
            own_terminal: opts.own_terminal.clone(),
            model: Model::default(),
            rows: Vec::new(),
            selected: None,
            panes: Vec::new(),
            sidebar: true,
            take_over: None,
            notice: opts.config.warnings.first().map(|w| format!("config: {w}")),
            prefix_pending: false,
            generation: 0,
        }
    }

    fn refresh(&mut self, client: &Client) -> Result<(), Error> {
        self.model = Model::load(client)?;
        self.rows = self.model.rows(&self.config.rows, self.home.as_deref());
        Ok(())
    }

    fn selected_name(&self) -> Option<&str> {
        let id = self.selected.as_deref()?;
        self.model.terminal(id).map(|t| t.name.as_str())
    }

    /// The focused pane: the selected terminal's.
    pub fn focused(&self) -> Option<&Pane> {
        let id = self.selected.as_deref()?;
        self.pane(id)
    }

    pub fn pane(&self, terminal_id: &str) -> Option<&Pane> {
        self.panes.iter().find(|p| p.terminal_id == terminal_id)
    }

    fn focused_mut(&mut self) -> Option<&mut Pane> {
        let id = self.selected.clone()?;
        self.panes.iter_mut().find(|p| p.terminal_id == id)
    }

    /// The tab of the selected terminal.
    pub fn current_tab(&self) -> Option<&poimen_client::protocol::Tab> {
        self.model.tab_of(self.selected.as_deref()?)
    }

    /// The space of the selected terminal, else the first space.
    fn current_space(&self) -> Option<String> {
        self.selected
            .as_deref()
            .and_then(|id| self.model.terminal(id))
            .map(|t| t.space_id.clone())
            .or_else(|| self.model.spaces.first().map(|s| s.id.clone()))
    }

    /// The rectangles of the visible panes in a pane area: the selected terminal's tab, or the
    /// terminal alone when it is in no tab.
    pub fn layout(&self, area: Rect) -> tree::Layout {
        if let Some(tab) = self.current_tab() {
            return tree::layout(&tab.root, area);
        }
        match &self.selected {
            Some(id) => tree::layout(
                &poimen_client::protocol::Node::Terminal {
                    terminal_id: id.clone(),
                },
                area,
            ),
            None => tree::Layout::default(),
        }
    }
}

/// Runs the TUI on `terminal` until detach, `stop` or the server going away. `input` yields the
/// local terminal's events.
pub fn run<B: Backend>(
    client: &Client,
    opts: &Options,
    input: Receiver<InputEvent>,
    terminal: &mut Terminal<B>,
    stop: &AtomicBool,
) -> Result<Outcome, Error> {
    let (tx, rx) = mpsc::channel();
    let mut events = client.subscribe_as(Some(ClientInfo {
        kind: ClientKind::Tui,
        pid: std::process::id(),
    }))?;
    {
        let tx = tx.clone();
        thread::spawn(move || {
            for event in events.by_ref() {
                match event {
                    Ok(_) => {
                        if tx.send(Msg::Changed).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::ServerGone(e.to_string()));
                        return;
                    }
                }
            }
            let _ = tx.send(Msg::ServerGone("connection ended".into()));
        });
    }
    {
        let tx = tx.clone();
        thread::spawn(move || {
            for ev in input {
                if tx.send(Msg::Input(ev)).is_err() {
                    return;
                }
            }
        });
    }

    let mut app = App::new(opts);
    let mut router = Router::new(&opts.config);
    app.refresh(client)?;
    let first = app.model.initial(app.own_terminal.as_deref());
    select(&mut app, client, &tx, first, How::Control);

    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(Outcome::Interrupted);
        }
        sync_size(&mut app, terminal)?;
        let completed = terminal
            .draw(|f| view::draw(f, &app))
            .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
        if let Some(mirror) = &opts.mirror {
            *mirror.lock().unwrap_or_else(|e| e.into_inner()) = completed.buffer.clone();
        }

        let first = match rx.recv_timeout(TICK) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Ok(Outcome::ServerGone("connection ended".into()));
            }
        };
        // Everything that is already there is handled before the next frame.
        let mut changed = false;
        for msg in std::iter::once(first).chain(rx.try_iter()) {
            match msg {
                Msg::Changed => changed = true,
                Msg::ServerGone(e) => return Ok(Outcome::ServerGone(e)),
                Msg::Record(generation, rec) => record(&mut app, client, &tx, generation, *rec),
                Msg::Input(ev) => {
                    if let Some(outcome) =
                        on_input(&mut app, &mut router, client, &tx, terminal, &ev)?
                    {
                        return Ok(outcome);
                    }
                }
            }
        }
        if changed {
            follow_server(&mut app, client, &tx);
        }
    }
}

fn window<B: Backend>(terminal: &Terminal<B>) -> Result<Rect, Error> {
    let size = terminal
        .size()
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(Rect::new(0, 0, size.width, size.height))
}

/// The pane area (below the tab bar) for the current window size.
fn pane_area<B: Backend>(app: &App, terminal: &Terminal<B>) -> Result<Rect, Error> {
    Ok(view::layout(window(terminal)?, app.sidebar).pane)
}

/// As controller, sizes the PTY to the focused pane's rectangle.
fn sync_size<B: Backend>(app: &mut App, terminal: &Terminal<B>) -> Result<(), Error> {
    let area = pane_area(app, terminal)?;
    let Some(rect) = app
        .selected
        .as_deref()
        .and_then(|id| app.layout(area).rect_of(id))
    else {
        return Ok(());
    };
    let want = (rect.width.max(1), rect.height.max(1));
    if let Some(pane) = app.focused_mut().filter(|p| p.controls())
        && pane.sent_size != Some(want)
    {
        pane.sent_size = Some(want);
        pane.writer.resize(want.0, want.1)?;
    }
    Ok(())
}

/// How [`select`] attaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    /// As controller; when another client has the control, observe and ask.
    Control,
    /// As controller, replacing the current one (the user said yes).
    TakeOver,
    /// As observer, without asking (the control was just taken over by someone else).
    Observe,
}

/// Focuses `id` (`None`: nothing) and attaches the panes of its tab: `id` as controller and
/// the other leaves as observers. Attachments of terminals that are no longer visible end.
fn select(app: &mut App, client: &Client, tx: &Sender<Msg>, id: Option<String>, how: How) {
    app.take_over = None;
    app.selected = id.clone();
    if let Some(id) = &id {
        // A fresh attachment of the focused terminal, so the mode is the one asked for.
        app.panes.retain(|p| &p.terminal_id != id);
        if how != How::Control {
            attach_pane(app, client, tx, id, how);
        }
    }
    sync_panes(app, client, tx);
}

/// Makes the attachments match the visible panes: one per leaf of the focused terminal's tab
/// (not the terminal the TUI runs in), the focused one as controller and the rest as observers.
fn sync_panes(app: &mut App, client: &Client, tx: &Sender<Msg>) {
    let own = app.own_terminal.clone();
    let desired: Vec<String> = match (&app.selected, app.current_tab()) {
        (Some(_), Some(tab)) => tab
            .root
            .terminal_ids()
            .into_iter()
            .filter(|id| Some(*id) != own.as_deref())
            .map(str::to_owned)
            .collect(),
        (Some(id), None) => vec![id.clone()],
        (None, _) => Vec::new(),
    };
    app.panes.retain(|p| desired.contains(&p.terminal_id));
    if app.take_over != app.selected {
        app.take_over = None;
    }
    // Observers first: a pane that gives up the control does so before the next one takes it.
    let selected = app.selected.clone();
    for id in desired.iter().filter(|id| Some(*id) != selected.as_ref()) {
        if app.pane(id).is_none_or(|p| p.mode == AttachMode::Control) {
            attach_pane(app, client, tx, id, How::Observe);
        }
    }
    if let Some(id) = app.selected.clone().filter(|id| desired.contains(id))
        && app
            .pane(&id)
            .is_none_or(|p| p.mode == AttachMode::Observe && !p.sticky)
    {
        attach_pane(app, client, tx, &id, How::Control);
    }
}

/// Attaches to `id`, replacing its pane.
fn attach_pane(app: &mut App, client: &Client, tx: &Sender<Msg>, id: &str, how: How) {
    app.panes.retain(|p| p.terminal_id != id);
    let focused = app.selected.as_deref() == Some(id);
    let mut sticky = how == How::Observe && focused;
    let attached = match how {
        How::Observe => client.attach(id, AttachMode::Observe, false),
        How::Control | How::TakeOver => {
            match client.attach(id, AttachMode::Control, how == How::TakeOver) {
                Err(ClientError::Server {
                    code: ErrorCode::Conflict,
                    ..
                }) => {
                    if focused {
                        app.take_over = Some(id.to_owned());
                    }
                    sticky = true;
                    client.attach(id, AttachMode::Observe, false)
                }
                other => other,
            }
        }
    };
    match attached.and_then(|a| open(app, tx, id.to_owned(), a)) {
        Ok(mut pane) => {
            pane.sticky = sticky;
            app.panes.push(pane);
        }
        Err(e) => app.notice = Some(format!("cannot attach: {e}")),
    }
}

/// The pane of a fresh attachment: the grid at the terminal's size with the snapshot, and a
/// thread that forwards the stream records.
fn open(
    app: &mut App,
    tx: &Sender<Msg>,
    id: String,
    mut attachment: poimen_client::Attachment,
) -> Result<Pane, ClientError> {
    let writer = attachment.writer()?;
    let snapshot = decode_bytes(&attachment.info.snapshot)
        .map_err(|e| ClientError::Protocol(format!("snapshot: {e}")))?;
    let mut grid = Grid::new(attachment.info.cols, attachment.info.rows);
    grid.feed(&snapshot);
    // The snapshot replays the screen; the queries in it were answered long ago.
    let _ = grid.take_replies(&app.palette);
    app.generation += 1;
    let generation = app.generation;
    let mode = attachment.info.mode;
    let exited = app
        .model
        .terminal(&id)
        .filter(|t| t.status == TerminalStatus::Exited)
        .map(|t| t.exit_code);
    let tx = tx.clone();
    thread::spawn(move || {
        loop {
            let next = attachment.next_record().map_err(|e| e.to_string());
            let failed = next.is_err();
            if tx.send(Msg::Record(generation, Box::new(next))).is_err() || failed {
                return;
            }
        }
    });
    Ok(Pane {
        terminal_id: id,
        mode,
        grid,
        exited,
        writer,
        generation,
        sent_size: None,
        sticky: false,
    })
}

/// Reads the model again after a server event and follows it: panes of a changed tree are
/// attached or dropped, a focus change made elsewhere moves the focus here, and when the
/// focused terminal went away the focus goes to what the server shows next.
fn follow_server(app: &mut App, client: &Client, tx: &Sender<Msg>) {
    let old = app.model.clone();
    if let Err(e) = app.refresh(client) {
        app.notice = Some(format!("cannot read the server: {e}"));
        return;
    }
    let own = app.own_terminal.clone();
    let target = match app.selected.clone() {
        Some(sel) if app.model.terminal(&sel).is_some() => {
            sidebar::remote_focus(&old, &app.model, &sel, own.as_deref())
        }
        Some(sel) => sidebar::after_loss(&old, &app.model, &sel, own.as_deref()),
        None => app.model.initial(own.as_deref()),
    };
    let gone = app
        .selected
        .as_deref()
        .is_some_and(|id| app.model.terminal(id).is_none());
    match target {
        Some(t) => select(app, client, tx, Some(t), How::Control),
        None if gone => select(app, client, tx, None, How::Control),
        None => sync_panes(app, client, tx),
    }
}

/// Focuses `id` here and, when the server's focus differs, there too.
fn focus_terminal(app: &mut App, client: &Client, tx: &Sender<Msg>, id: &str) {
    if app.selected.as_deref() != Some(id) {
        select(app, client, tx, Some(id.to_owned()), How::Control);
    }
    let Some(tab) = app.model.tab_of(id) else {
        return;
    };
    let space_focus = app
        .model
        .spaces
        .iter()
        .find(|s| s.id == tab.space_id)
        .and_then(|s| s.active_tab.as_deref());
    if tab.active_terminal != id || space_focus != Some(tab.tab_id.as_str()) {
        let tab_id = tab.tab_id.clone();
        if let Err(e) = client.focus_tab(&tab_id, Some(id)) {
            app.notice = Some(format!("cannot focus: {e}"));
        }
    }
}

fn reload(app: &mut App, client: &Client) {
    if let Err(e) = app.refresh(client) {
        app.notice = Some(format!("cannot read the server: {e}"));
    }
}

fn record(
    app: &mut App,
    client: &Client,
    tx: &Sender<Msg>,
    generation: u64,
    rec: Result<Event, String>,
) {
    let Some(at) = app.panes.iter().position(|p| p.generation == generation) else {
        return;
    };
    let focused = app.selected.as_deref() == Some(app.panes[at].terminal_id.as_str());
    match rec {
        Ok(Event::TerminalOutput { data, .. }) => {
            let pane = &mut app.panes[at];
            if let Ok(bytes) = decode_bytes(&data) {
                pane.grid.feed(&bytes);
                let replies = pane.grid.take_replies(&app.palette);
                if pane.controls() && !replies.is_empty() {
                    let _ = pane.writer.input(&String::from_utf8_lossy(&replies));
                }
            }
        }
        Ok(Event::TerminalResized { cols, rows, .. }) => app.panes[at].grid.resize(cols, rows),
        Ok(Event::TerminalExited { exit_code, .. }) => app.panes[at].exited = Some(exit_code),
        Ok(Event::TerminalDetached { reason, .. }) => {
            let id = app.panes[at].terminal_id.clone();
            match reason {
                DetachReason::TakenOver => {
                    attach_pane(app, client, tx, &id, How::Observe);
                    if focused {
                        app.notice = Some("another client took over the control; observing".into());
                    }
                }
                DetachReason::Overflow if focused => {
                    select(app, client, tx, Some(id), How::Control);
                }
                DetachReason::Overflow => attach_pane(app, client, tx, &id, How::Observe),
                DetachReason::Closed => {
                    app.panes.remove(at);
                }
            }
        }
        Ok(_) => {}
        Err(e) => {
            app.notice = Some(format!("attachment ended: {e}"));
            app.panes.remove(at);
        }
    }
}

fn on_input<B: Backend>(
    app: &mut App,
    router: &mut Router,
    client: &Client,
    tx: &Sender<Msg>,
    terminal: &Terminal<B>,
    ev: &InputEvent,
) -> Result<Option<Outcome>, Error> {
    // The take-over question answers to y/n/Esc; the prefix keys keep working meanwhile.
    if let (Some(id), InputEvent::Key(key)) = (app.take_over.clone(), ev)
        && key.kind != KeyEventKind::Release
        && !router.pending()
    {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                select(app, client, tx, Some(id), How::TakeOver);
                return Ok(None);
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.take_over = None;
                return Ok(None);
            }
            _ => {}
        }
    }
    let modes = app.focused().map(|p| p.grid.modes()).unwrap_or_default();
    let routed = router.route(ev, modes);
    app.prefix_pending = router.pending();
    let Some(routed) = routed else {
        return Ok(None);
    };
    match routed {
        Routed::Detach => return Ok(Some(Outcome::Detached)),
        Routed::ToggleSidebar => app.sidebar = !app.sidebar,
        Routed::Select(delta) => {
            let next = sidebar::step(
                &app.rows,
                app.selected.as_deref(),
                delta,
                app.own_terminal.as_deref(),
            );
            if let Some(id) = next.filter(|id| Some(id) != app.selected.as_ref()) {
                focus_terminal(app, client, tx, &id);
            }
        }
        Routed::NewTab => new_tab(app, client, tx),
        Routed::SwitchTab(delta) => switch_tab(app, client, tx, delta),
        Routed::CloseTab => {
            if let Some(tab) = app.current_tab().map(|t| t.tab_id.clone())
                && let Err(e) = client.close_tab(&tab)
            {
                app.notice = Some(format!("cannot close the tab: {e}"));
            }
        }
        Routed::Split(direction) => split(app, client, tx, direction),
        Routed::ClosePane => {
            if let Some(id) = app.selected.clone()
                && let Err(e) = client.close_terminal(&id)
            {
                app.notice = Some(format!("cannot close the pane: {e}"));
            }
        }
        Routed::Focus(dir) => {
            let layout = app.layout(pane_area(app, terminal)?);
            let leaves: Vec<_> = layout
                .leaves
                .into_iter()
                .filter(|l| Some(&l.terminal_id) != app.own_terminal.as_ref())
                .collect();
            if let Some(from) = app.selected.as_deref()
                && let Some(to) = tree::neighbor(&leaves, from, dir)
            {
                focus_terminal(app, client, tx, &to);
            }
        }
        Routed::ResizePane(dir) => resize(app, client, dir),
        Routed::Send(bytes) => {
            if let Some(pane) = app.focused_mut().filter(|p| p.controls()) {
                pane.grid.scroll_to_bottom();
                pane.writer.input(&String::from_utf8_lossy(&bytes))?;
            }
        }
        Routed::Wheel { up, column, row } => {
            let layout = app.layout(pane_area(app, terminal)?);
            if let Some(leaf) = layout.leaf_at(column, row)
                && let Some(pane) = app
                    .panes
                    .iter_mut()
                    .find(|p| p.terminal_id == leaf.terminal_id)
            {
                let (x, y) = (column - leaf.rect.x, row - leaf.rect.y);
                match input::wheel(pane.grid.modes(), up, x, y) {
                    Wheel::Scroll(lines) => pane.grid.scroll(lines),
                    Wheel::Send(bytes) if pane.controls() => {
                        pane.writer.input(&String::from_utf8_lossy(&bytes))?;
                    }
                    Wheel::Send(_) => {}
                }
            }
        }
        Routed::Click { column, row } => {
            let areas = view::layout(window(terminal)?, app.sidebar);
            if let Some(side) = areas.sidebar
                && side.contains((column, row).into())
            {
                if let Some(id) = view::row_at(app, side, row - side.y)
                    && Some(&id) != app.selected.as_ref()
                    && Some(&id) != app.own_terminal.as_ref()
                {
                    focus_terminal(app, client, tx, &id);
                }
            } else if let Some(leaf) = app.layout(areas.pane).leaf_at(column, row) {
                let id = leaf.terminal_id.clone();
                if Some(&id) != app.selected.as_ref() && Some(&id) != app.own_terminal.as_ref() {
                    focus_terminal(app, client, tx, &id);
                }
            }
        }
        Routed::Resize => {}
    }
    Ok(None)
}

fn new_tab(app: &mut App, client: &Client, tx: &Sender<Msg>) {
    let Some(space_id) = app.current_space() else {
        app.notice = Some("no space for a new tab".into());
        return;
    };
    let params = TabCreate {
        space_id,
        name: None,
        terminal_id: None,
    };
    match client.create_tab(&params) {
        Ok((_, terminal)) => {
            reload(app, client);
            focus_terminal(app, client, tx, &terminal.id);
        }
        Err(e) => app.notice = Some(format!("cannot create a tab: {e}")),
    }
}

/// Shows the tab `delta` places on in the space (wrapping), skipping tabs that hold only the
/// terminal the TUI runs in.
fn switch_tab(app: &mut App, client: &Client, tx: &Sender<Msg>, delta: isize) {
    let Some(current) = app.current_tab() else {
        return;
    };
    let tabs = app.model.space_tabs(&current.space_id);
    let Some(at) = tabs.iter().position(|t| t.tab_id == current.tab_id) else {
        return;
    };
    let n = tabs.len() as isize;
    let target = (1..n)
        .map(|step| (at as isize + delta.signum() * step).rem_euclid(n) as usize)
        .find_map(|i| sidebar::focus_target(tabs[i], app.own_terminal.as_deref()));
    if let Some(id) = target {
        focus_terminal(app, client, tx, &id);
    }
}

fn split(app: &mut App, client: &Client, tx: &Sender<Msg>, direction: SplitDirection) {
    let Some(id) = app.selected.clone() else {
        return;
    };
    let params = TabSplit {
        terminal_id: id,
        direction,
        ratio: None,
        cwd: None,
        name: None,
        command: None,
    };
    match client.split(&params) {
        Ok((_, terminal)) => {
            reload(app, client);
            focus_terminal(app, client, tx, &terminal.id);
        }
        Err(e) => app.notice = Some(format!("cannot split: {e}")),
    }
}

fn resize(app: &mut App, client: &Client, dir: Dir) {
    let (Some(id), Some(tab)) = (app.selected.as_deref(), app.current_tab()) else {
        return;
    };
    let Some((split_id, ratio)) = tree::resize(&tab.root, id, dir) else {
        return;
    };
    let tab_id = tab.tab_id.clone();
    match client.resize_split(&tab_id, &split_id, ratio) {
        Ok(_) => reload(app, client),
        Err(e) => app.notice = Some(format!("cannot resize: {e}")),
    }
}
