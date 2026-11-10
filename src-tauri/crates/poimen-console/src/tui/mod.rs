//! The TUI of `poimen` without arguments (ADR 0006): a sidebar with the spaces, tabs and
//! terminals of the server, and the selected terminal drawn from a grid of its own.
//!
//! The server owns every state: the sidebar is read from `space.list`/`terminal.list` and read
//! again on every event of an `events.subscribe` connection (as a `tui` client). The pane attaches
//! to the selected terminal as controller; when another client holds the control the pane
//! observes and asks before it takes over. Moving the selection detaches and attaches to the
//! next terminal. Detaching (prefix, then `d`) ends the attachment with `terminal.detach`: no
//! terminal is closed, the server is never stopped.
//!
//! `config` reads the prefix and keys, `input` routes keys, `grid` is the pane's emulator,
//! `sidebar` the row model, `screen` the local terminal modes, `view` draws a frame.

pub mod config;
pub mod grid;
pub mod input;
pub mod screen;
pub mod sidebar;
pub mod view;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::event::{Event as InputEvent, KeyCode, KeyEventKind};
use poimen_client::protocol::{
    AttachMode, ClientInfo, ClientKind, DetachReason, ErrorCode, Event, TerminalStatus,
    decode_bytes,
};
use poimen_client::{AttachmentWriter, Client, ClientError};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;

use crate::session::Error;
use config::Config;
use grid::{Grid, Palette};
use input::{Routed, Router, Wheel};
use sidebar::{Model, Row};

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
    pub pane: Option<Pane>,
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
            pane: None,
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
            if let Err(e) = app.refresh(client) {
                app.notice = Some(format!("cannot read the server: {e}"));
            }
            let gone = app
                .selected
                .as_deref()
                .is_none_or(|id| app.model.terminal(id).is_none());
            if gone {
                let next = app.model.initial(app.own_terminal.as_deref());
                select(&mut app, client, &tx, next, How::Control);
            }
        }
    }
}

/// The pane's area for the current window size.
fn pane_area<B: Backend>(
    app: &App,
    terminal: &Terminal<B>,
) -> Result<ratatui::layout::Rect, Error> {
    let size = terminal
        .size()
        .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(view::layout(
        ratatui::layout::Rect::new(0, 0, size.width, size.height),
        app.sidebar,
    )
    .pane)
}

/// As controller, sizes the PTY to the pane.
fn sync_size<B: Backend>(app: &mut App, terminal: &Terminal<B>) -> Result<(), Error> {
    let area = pane_area(app, terminal)?;
    let want = (area.width.max(1), area.height.max(1));
    if let Some(pane) = app.pane.as_mut().filter(|p| p.controls())
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

/// Shows `id` in the pane; `None` empties it. The previous attachment ends first.
fn select(app: &mut App, client: &Client, tx: &Sender<Msg>, id: Option<String>, how: How) {
    app.pane = None;
    app.take_over = None;
    app.selected = id.clone();
    let Some(id) = id else {
        return;
    };
    let attached = match how {
        How::Observe => client.attach(&id, AttachMode::Observe, false),
        How::Control | How::TakeOver => {
            match client.attach(&id, AttachMode::Control, how == How::TakeOver) {
                Err(ClientError::Server {
                    code: ErrorCode::Conflict,
                    ..
                }) => {
                    app.take_over = Some(id.clone());
                    client.attach(&id, AttachMode::Observe, false)
                }
                other => other,
            }
        }
    };
    match attached.and_then(|a| open(app, tx, id, a)) {
        Ok(pane) => app.pane = Some(pane),
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
    })
}

fn record(
    app: &mut App,
    client: &Client,
    tx: &Sender<Msg>,
    generation: u64,
    rec: Result<Event, String>,
) {
    let Some(pane) = app.pane.as_mut().filter(|p| p.generation == generation) else {
        return;
    };
    match rec {
        Ok(Event::TerminalOutput { data, .. }) => {
            if let Ok(bytes) = decode_bytes(&data) {
                pane.grid.feed(&bytes);
                let replies = pane.grid.take_replies(&app.palette);
                if pane.controls() && !replies.is_empty() {
                    let _ = pane.writer.input(&String::from_utf8_lossy(&replies));
                }
            }
        }
        Ok(Event::TerminalResized { cols, rows, .. }) => pane.grid.resize(cols, rows),
        Ok(Event::TerminalExited { exit_code, .. }) => pane.exited = Some(exit_code),
        Ok(Event::TerminalDetached { reason, .. }) => {
            let id = Some(pane.terminal_id.clone());
            match reason {
                DetachReason::TakenOver => {
                    select(app, client, tx, id, How::Observe);
                    app.notice = Some("another client took over the control; observing".into());
                }
                DetachReason::Overflow => select(app, client, tx, id, How::Control),
                DetachReason::Closed => app.pane = None,
            }
        }
        Ok(_) => {}
        Err(e) => {
            app.notice = Some(format!("attachment ended: {e}"));
            app.pane = None;
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
    let modes = app
        .pane
        .as_ref()
        .map(|p| p.grid.modes())
        .unwrap_or_default();
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
            if next != app.selected {
                select(app, client, tx, next, How::Control);
            }
        }
        Routed::Send(bytes) => {
            if let Some(pane) = app.pane.as_mut().filter(|p| p.controls()) {
                pane.grid.scroll_to_bottom();
                pane.writer.input(&String::from_utf8_lossy(&bytes))?;
            }
        }
        Routed::Wheel { up, column, row } => {
            let area = pane_area(app, terminal)?;
            if let Some(pane) = app.pane.as_mut()
                && area.contains((column, row).into())
            {
                match input::wheel(pane.grid.modes(), up, column - area.x, row - area.y) {
                    Wheel::Scroll(lines) => pane.grid.scroll(lines),
                    Wheel::Send(bytes) if pane.controls() => {
                        pane.writer.input(&String::from_utf8_lossy(&bytes))?;
                    }
                    Wheel::Send(_) => {}
                }
            }
        }
        Routed::Click { column, row } => {
            let size = terminal
                .size()
                .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
            let areas = view::layout(
                ratatui::layout::Rect::new(0, 0, size.width, size.height),
                app.sidebar,
            );
            if let Some(side) = areas.sidebar
                && side.contains((column, row).into())
                && let Some(id) = view::row_at(app, side, row - side.y)
                && Some(&id) != app.selected.as_ref()
                && Some(&id) != app.own_terminal.as_ref()
            {
                select(app, client, tx, Some(id), How::Control);
            }
        }
        Routed::Resize => {}
    }
    Ok(None)
}
