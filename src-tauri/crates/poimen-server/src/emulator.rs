//! The headless terminal state of one terminal: an `alacritty_terminal` grid fed with the PTY
//! output, and the attach snapshot made from it.
//!
//! Live output goes to the clients as raw bytes; the grid is only read to build the snapshot (a
//! byte stream a fresh emulator replays to reach the same screen, scrollback and modes).
//!
//! `alacritty_terminal` hides two things the snapshot needs: the scroll region and, while the
//! alternate screen is active, the primary grid. [`Scanner`] watches the output for the escape
//! sequences involved, so the emulator remembers the region and dumps the primary screen right
//! before the alternate screen is entered.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Flags, Hyperlink};

mod snapshot_pen;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color, CursorShape, NamedColor, Processor, Rgb, StdSyncHandler,
};
use snapshot_pen::{Pen, kitty_flags, link_sequence};

/// The scrollback kept per terminal unless configured otherwise.
pub const DEFAULT_SCROLLBACK: usize = 10_000;

/// The colors terminal queries are answered with (OSC 10, 11, 12 and 4): the active scheme,
/// which the app sends with `server.set_colors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// The 16 ANSI colors; 16..=255 are the xterm cube and gray ramp.
    pub ansi: [Rgb; 16],
}

impl Default for Colors {
    /// A dark scheme, used until the app has sent its own.
    fn default() -> Self {
        let c = |r, g, b| Rgb { r, g, b };
        Self {
            foreground: c(0xd8, 0xd8, 0xd8),
            background: c(0x18, 0x18, 0x18),
            cursor: c(0xd8, 0xd8, 0xd8),
            ansi: [
                c(0x00, 0x00, 0x00),
                c(0xcd, 0x00, 0x00),
                c(0x00, 0xcd, 0x00),
                c(0xcd, 0xcd, 0x00),
                c(0x00, 0x00, 0xee),
                c(0xcd, 0x00, 0xcd),
                c(0x00, 0xcd, 0xcd),
                c(0xe5, 0xe5, 0xe5),
                c(0x7f, 0x7f, 0x7f),
                c(0xff, 0x00, 0x00),
                c(0x00, 0xff, 0x00),
                c(0xff, 0xff, 0x00),
                c(0x5c, 0x5c, 0xff),
                c(0xff, 0x00, 0xff),
                c(0x00, 0xff, 0xff),
                c(0xff, 0xff, 0xff),
            ],
        }
    }
}

impl Colors {
    /// The color a query for `index` (alacritty's numbering: 0..=255 palette, 256 foreground,
    /// 257 background, 258 cursor) is answered with; `None` for an index that is not a color.
    fn get(&self, index: usize) -> Option<Rgb> {
        let c = |r: u8, g: u8, b: u8| Rgb { r, g, b };
        match index {
            0..=15 => Some(self.ansi[index]),
            16..=231 => {
                let i = u8::try_from(index - 16).ok()?;
                let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
                Some(c(level(i / 36), level(i / 6 % 6), level(i % 6)))
            }
            232..=255 => {
                let v = 8 + 10 * u8::try_from(index - 232).ok()?;
                Some(c(v, v, v))
            }
            256 => Some(self.foreground),
            257 => Some(self.background),
            258 => Some(self.cursor),
            _ => None,
        }
    }
}

type Formatter = Arc<dyn Fn(Rgb) -> String + Send + Sync>;

#[derive(Default)]
struct Shared {
    title: Option<String>,
    /// Answers the grid wants written to the PTY.
    replies: Vec<String>,
    /// The part of `replies` the server gives itself, also with a controller attached (the OSC 7501
    /// detection answer: xterm.js does not know it).
    own: Vec<String>,
    /// Color queries; the formatter is applied once the grid is up to date.
    color_requests: Vec<(usize, Formatter)>,
}

/// Remembers the window title the emulator reports and collects the answers it wants to give to
/// terminal queries (see [`Emulator::take_replies`]); everything else is ignored.
#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Shared>>);

impl Listener {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut shared = self.lock();
        match event {
            Event::Title(t) => shared.title = Some(t),
            Event::ResetTitle => shared.title = None,
            Event::PtyWrite(text) => shared.replies.push(text),
            Event::ColorRequest(index, format) => shared.color_requests.push((index, format)),
            _ => {}
        }
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// The answers to the queries fed so far.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Answers {
    /// Every answer, in order.
    pub all: Vec<u8>,
    /// The ones the server writes even while a controller is attached.
    pub own: Vec<u8>,
}

/// The answer to `OSC 7501 ; ? ST`: what the server understands, nothing from any report.
pub const DETECTION_REPLY: &str = "\x1b]7501;?:states=idle,working,blocked,done,error,clear:kinds=permission,question,auth:tones=warning\x1b\\";

pub struct Emulator {
    term: Term<Listener>,
    processor: Processor<StdSyncHandler>,
    title: Listener,
    scanner: Scanner,
    /// 0-based inclusive `(top, bottom)`, `None` for the whole screen.
    region: Option<(usize, usize)>,
    /// The primary screen with its scrollback, dumped when the alternate screen was entered.
    primary: Option<Vec<u8>>,
    /// What the scanner saw that concerns the agent status, until [`Emulator::take_signals`].
    signals: Vec<Signal>,
}

impl Emulator {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let title = Listener::default();
        let config = Config {
            scrolling_history: scrollback,
            kitty_keyboard: true,
            ..Config::default()
        };
        let term = Term::new(
            config,
            &Size {
                cols: usize::from(cols.max(1)),
                rows: usize::from(rows.max(1)),
            },
            title.clone(),
        );
        Self {
            term,
            processor: Processor::new(),
            title,
            scanner: Scanner::default(),
            region: None,
            primary: None,
            signals: Vec::new(),
        }
    }

    /// The agent status signals fed since the last call (OSC 7501 reports, new prompts, RIS), in
    /// order.
    pub fn take_signals(&mut self) -> Vec<Signal> {
        std::mem::take(&mut self.signals)
    }

    #[cfg(test)]
    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Whether the alternate screen is active (a full-screen program).
    pub fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn size(&self) -> (u16, u16) {
        (
            u16::try_from(self.term.columns()).unwrap_or(u16::MAX),
            u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX),
        )
    }

    pub fn title(&self) -> Option<String> {
        self.title.lock().title.clone()
    }

    /// The scroll region as 0-based inclusive `(top, bottom)` rows, `None` when it spans the
    /// whole screen.
    #[cfg(test)]
    pub fn scroll_region(&self) -> Option<(usize, usize)> {
        self.region
    }

    /// Cursor position as 0-based `(row, column)` on the visible screen.
    pub fn cursor(&self) -> (usize, usize) {
        let p = self.term.grid().cursor.point;
        (usize::try_from(p.line.0).unwrap_or(0), p.column.0)
    }

    /// The answers to the queries fed so far (DA1/DA2, DSR, OSC 10/11/12/4 color queries), as
    /// bytes for the PTY, in order. Taken once: the next call only returns newer ones. A color
    /// the program set itself (OSC 4, 10, 11) wins over `colors`.
    #[cfg(test)]
    pub fn take_replies(&mut self, colors: &Colors) -> Vec<u8> {
        self.take_answers(colors).all
    }

    /// Like [`take_replies`](Self::take_replies), plus the part of the answers that the server
    /// gives itself even while a controller is attached (the OSC 7501 detection answer).
    pub fn take_answers(&mut self, colors: &Colors) -> Answers {
        let (replies, own, requests) = {
            let mut shared = self.title.lock();
            (
                std::mem::take(&mut shared.replies),
                std::mem::take(&mut shared.own),
                std::mem::take(&mut shared.color_requests),
            )
        };
        let mut all = replies.concat().into_bytes();
        for (index, format) in requests {
            let own = self.term.colors()[index];
            if let Some(rgb) = own.or_else(|| colors.get(index)) {
                all.extend(format(rgb).into_bytes());
            }
        }
        Answers {
            all,
            own: own.concat().into_bytes(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let rows = self.term.screen_lines();
        let mut done = 0;
        for hit in self.scanner.scan(bytes) {
            match hit {
                Hit::EnterAlt(at) => {
                    self.processor.advance(&mut self.term, &bytes[done..at]);
                    done = at;
                    if !self.term.mode().contains(TermMode::ALT_SCREEN) {
                        self.primary = Some(self.dump_screen());
                    }
                }
                Hit::Region(region) => {
                    self.region = region
                        .map(|(t, b)| (t.min(rows - 1), b.min(rows - 1)))
                        .filter(|&(t, b)| t < b && !(t == 0 && b == rows - 1));
                }
                Hit::Reset => self.region = None,
                Hit::Ris => self.signals.push(Signal::Ris),
                Hit::Prompt => self.signals.push(Signal::Prompt),
                Hit::Report(report) => self.signals.push(Signal::Report(report)),
                Hit::Detect(at) => {
                    // The grid answers what came before the query first.
                    self.processor.advance(&mut self.term, &bytes[done..at]);
                    done = at;
                    let mut shared = self.title.lock();
                    shared.replies.push(DETECTION_REPLY.to_string());
                    shared.own.push(DETECTION_REPLY.to_string());
                }
            }
        }
        self.processor.advance(&mut self.term, &bytes[done..]);
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.primary = None;
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(Size {
            cols: usize::from(cols.max(1)),
            rows: usize::from(rows.max(1)),
        });
        // The grid resets its scroll region on a resize.
        self.region = None;
    }

    /// Bytes that bring a fresh emulator of the same size to this state: the screen with its
    /// scrollback, the modes, the cursor and the title.
    pub fn snapshot(&self) -> Vec<u8> {
        let mode = *self.term.mode();
        // RIS first, so the state does not depend on what the client's emulator did before.
        let mut out = b"\x1bc".to_vec();
        if mode.contains(TermMode::ALT_SCREEN) {
            // Primary screen and scrollback, then the alternate screen on top (1049 saves the
            // primary cursor, which the dump left in place).
            if let Some(primary) = &self.primary {
                out.extend_from_slice(primary);
            }
            out.extend_from_slice(b"\x1b[?1049h");
        }
        out.extend(self.dump_screen());
        out.extend(self.modes_and_cursor(mode));
        out
    }

    /// Dump of the active grid including the scrollback, ending with the cursor placed.
    fn dump_screen(&self) -> Vec<u8> {
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let cols = grid.columns();
        let mut out = String::new();
        let mut pen = Pen::default();
        let mut link: Option<Hyperlink> = None;
        let mut line = top;
        while line <= bottom {
            let row = &grid[Line(line)];
            // Trailing cells that look like an empty cell are not written.
            let mut end = cols;
            while end > 0 && is_blank(&row[Column(end - 1)]) {
                end -= 1;
            }
            for c in 0..end {
                let cell = &row[Column(c)];
                let cell_link = cell.hyperlink();
                if cell_link != link {
                    link_sequence(&mut out, cell_link.as_ref());
                    link = cell_link;
                }
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                pen.apply(&mut out, cell.fg, cell.bg, cell.flags);
                out.push(if cell.c == '\0' { ' ' } else { cell.c });
                for z in cell.zerowidth().unwrap_or(&[]) {
                    out.push(*z);
                }
            }
            let wrapped = end == cols && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
            if line < bottom && !wrapped {
                pen.reset(&mut out);
                out.push_str("\r\n");
            }
            line += 1;
        }
        if link.is_some() {
            link_sequence(&mut out, None);
        }
        pen.reset(&mut out);
        let c = grid.cursor.point;
        let _ = write!(out, "\x1b[{};{}H", c.line.0 + 1, c.column.0 + 1);
        out.into_bytes()
    }

    fn modes_and_cursor(&self, mode: TermMode) -> Vec<u8> {
        let mut out = String::new();
        let set = |out: &mut String, code: &str, on: bool| {
            let _ = write!(out, "\x1b[{code}{}", if on { 'h' } else { 'l' });
        };
        if mode.contains(TermMode::APP_KEYPAD) {
            out.push_str("\x1b=");
        }
        set(&mut out, "?1", mode.contains(TermMode::APP_CURSOR));
        set(&mut out, "?7", mode.contains(TermMode::LINE_WRAP));
        set(
            &mut out,
            "?1000",
            mode.contains(TermMode::MOUSE_REPORT_CLICK),
        );
        set(&mut out, "?1002", mode.contains(TermMode::MOUSE_DRAG));
        set(&mut out, "?1003", mode.contains(TermMode::MOUSE_MOTION));
        set(&mut out, "?1004", mode.contains(TermMode::FOCUS_IN_OUT));
        set(&mut out, "?1005", mode.contains(TermMode::UTF8_MOUSE));
        set(&mut out, "?1006", mode.contains(TermMode::SGR_MOUSE));
        set(&mut out, "?1007", mode.contains(TermMode::ALTERNATE_SCROLL));
        set(&mut out, "?2004", mode.contains(TermMode::BRACKETED_PASTE));
        set(&mut out, "4", mode.contains(TermMode::INSERT));
        set(&mut out, "20", mode.contains(TermMode::LINE_FEED_NEW_LINE));
        if let Some(title) = self.title() {
            // Control characters would end the OSC early.
            let clean: String = title.chars().filter(|c| !c.is_control()).collect();
            let _ = write!(out, "\x1b]2;{clean}\x1b\\");
        }
        let origin = mode.contains(TermMode::ORIGIN);
        // DECSTBM and DECOM both home the cursor, so the cursor is placed after them.
        match self.region {
            Some((t, b)) => {
                let _ = write!(out, "\x1b[{};{}r", t + 1, b + 1);
            }
            None => out.push_str("\x1b[r"),
        }
        set(&mut out, "?6", origin);
        let (row, col) = self.cursor();
        let row = if origin {
            row.saturating_sub(self.region.map_or(0, |(t, _)| t))
        } else {
            row
        };
        let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
        // The pen the next character is written with.
        let template = &self.term.grid().cursor.template;
        let mut pen = Pen::default();
        pen.apply(&mut out, template.fg, template.bg, template.flags);
        let style = self.term.cursor_style();
        let code = match (style.shape, style.blinking) {
            (CursorShape::Underline, true) => 3,
            (CursorShape::Underline, false) => 4,
            (CursorShape::Beam, true) => 5,
            (CursorShape::Beam, false) => 6,
            (_, true) => 1,
            (_, false) => 2,
        };
        let _ = write!(out, "\x1b[{code} q");
        set(&mut out, "?25", mode.contains(TermMode::SHOW_CURSOR));
        // The kitty keyboard flags the program pushed (of the active screen's stack).
        let flags = kitty_flags(mode);
        if flags != 0 {
            let _ = write!(out, "\x1b[>{flags}u");
        }
        out.into_bytes()
    }
}

fn is_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    cell.hyperlink().is_none()
        && (cell.c == ' ' || cell.c == '\0')
        && matches!(cell.bg, Color::Named(NamedColor::Background))
        && !cell
            .flags
            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT | Flags::WRAPLINE)
        && cell.zerowidth().is_none()
}

// --- the scanner -------------------------------------------------------------------------------

/// What the output said about the agent status of the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// An OSC 7501 report.
    Report(Report),
    /// OSC 133 `A`: the shell shows a new prompt.
    Prompt,
    /// RIS (`ESC c`).
    Ris,
}

/// The state of an OSC 7501 report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportState {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
    Clear,
}

/// A parsed OSC 7501 report (`ESC ] 7501 ; key=value:… ST`). Everything in it is untrusted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub state: Option<ReportState>,
    /// Absent for the root record.
    pub id: Option<String>,
    pub app: Option<String>,
    pub msg: Option<String>,
    pub title: Option<String>,
    /// 0 to 100 (`progress`).
    pub progress: Option<u8>,
    /// `permission`, `question` or `auth` (`kind`).
    pub kind: Option<String>,
    /// `warning` (`tone`).
    pub tone: Option<String>,
}

/// Longest OSC 7501 sequence read, in bytes after `ESC ]`.
const MAX_REPORT: usize = 4096;
const MAX_MSG: usize = 2048;
const MAX_TITLE: usize = 192;
const MAX_APP: usize = 32;
const MAX_KEY: usize = 16;

fn value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
}

/// Standard base64 of UTF-8 without control characters, padding optional, at most `max` decoded
/// bytes.
fn text_of(value: &str, max: usize) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    // Bound the work before decoding: 4 characters give 3 bytes.
    if value.len() > max.div_ceil(3) * 4 + 4 {
        return None;
    }
    let engine = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let bytes = engine.decode(value).ok()?;
    if bytes.len() > max {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// Parses the body of an OSC 7501 (after `7501;`). `None` drops the whole report.
fn parse_report(body: &[u8]) -> Option<Report> {
    let body = std::str::from_utf8(body).ok()?;
    let mut state = None;
    let mut report = Report::default();
    for pair in body.split(':') {
        let pair = pair.trim();
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if key.len() > MAX_KEY {
            return None;
        }
        let clean = value.bytes().all(value_byte);
        match key {
            "state" if clean => {
                state = Some(match value {
                    "idle" => ReportState::Idle,
                    "working" => ReportState::Working,
                    "blocked" => ReportState::Blocked,
                    "done" => ReportState::Done,
                    "error" => ReportState::Error,
                    "clear" => ReportState::Clear,
                    _ => return None,
                });
            }
            "id" => {
                if !clean || !poimen_protocol::valid_record_id(value) {
                    return None;
                }
                report.id = Some(value.to_string());
            }
            "app" if clean => {
                if value.len() > MAX_APP {
                    return None;
                }
                report.app = (!value.is_empty()).then(|| value.to_string());
            }
            "msg" if clean => report.msg = Some(text_of(value, MAX_MSG)?),
            "title" if clean => report.title = Some(text_of(value, MAX_TITLE)?),
            // A value outside the spec is ignored like an unknown key; the state still counts.
            "progress" if clean => {
                report.progress = value.parse::<u8>().ok().filter(|p| *p <= 100);
            }
            "kind" if clean => {
                report.kind = poimen_protocol::BLOCK_KINDS
                    .contains(&value)
                    .then(|| value.to_string());
            }
            "tone" if clean => {
                report.tone = poimen_protocol::TONES
                    .contains(&value)
                    .then(|| value.to_string());
            }
            _ => {}
        }
    }
    report.state = Some(state?);
    Some(report)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// The final byte of a sequence that enters the alternate screen is at this index.
    EnterAlt(usize),
    /// DECSTBM: 0-based `(top, bottom)`, `None` for the default (whole screen).
    Region(Option<(usize, usize)>),
    /// RIS or DECSTR.
    Reset,
    /// RIS only.
    Ris,
    /// OSC 133 `A`.
    Prompt,
    /// A complete, valid OSC 7501 report.
    Report(Report),
    /// `OSC 7501 ; ? ST` ended at this index (exclusive).
    Detect(usize),
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi,
    /// Inside an OSC/DCS/APC/PM/SOS string up to its terminator.
    Str,
    StrEsc,
}

/// A tiny CSI scanner that carries its state across calls, for the few sequences the grid does
/// not expose (see the module doc).
#[derive(Default)]
struct Scanner {
    state: State,
    params: Vec<u8>,
    /// The string in `Str` is an OSC and its body is collected in `body` (the start of an OSC 7501
    /// or an OSC 133 is wanted, nothing else).
    collect: bool,
    body: Vec<u8>,
    /// More bytes than the limit of the sequence came, or it was cut off: nothing is reported.
    spoiled: bool,
}

impl Scanner {
    fn scan(&mut self, bytes: &[u8]) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    }
                }
                State::Esc => self.state = self.esc(b, &mut hits),
                State::Csi => match b {
                    0x20..=0x3f => {
                        if self.params.len() < 64 {
                            self.params.push(b);
                        }
                    }
                    0x40..=0x7e => {
                        self.csi(b, i, &mut hits);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Esc,
                    // Other C0 controls are executed inside a sequence; the sequence goes on.
                    _ => {}
                },
                State::Str => match b {
                    0x07 => {
                        self.finish(i, &mut hits);
                        self.state = State::Ground;
                    }
                    0x18 | 0x1a => self.state = State::Ground,
                    0x1b => self.state = State::StrEsc,
                    _ => self.collect_byte(b),
                },
                State::StrEsc => {
                    if b == b'\\' {
                        self.finish(i, &mut hits);
                        self.state = State::Ground;
                    } else {
                        // ESC inside a string aborts it and starts a new sequence.
                        self.state = self.esc(b, &mut hits);
                    }
                }
            }
        }
        hits
    }

    /// The byte after ESC: the state it leads to.
    fn esc(&mut self, b: u8, hits: &mut Vec<Hit>) -> State {
        match b {
            b'[' => {
                self.params.clear();
                State::Csi
            }
            b']' | b'P' | b'_' | b'^' | b'X' => {
                self.collect = b == b']';
                self.spoiled = false;
                self.body.clear();
                State::Str
            }
            b'c' => {
                hits.push(Hit::Reset);
                hits.push(Hit::Ris);
                State::Ground
            }
            0x1b => State::Esc,
            _ => State::Ground,
        }
    }

    /// Keeps the start of an OSC string: 7501 up to its limit, anything else only as far as it
    /// takes to tell it is not wanted (the buffer never grows past `MAX_REPORT`).
    fn collect_byte(&mut self, b: u8) {
        if !self.collect || self.spoiled {
            return;
        }
        let limit = if self.body.starts_with(b"7501;") {
            MAX_REPORT
        } else {
            32
        };
        if self.body.len() >= limit {
            // Too long for a report; an OSC 133 never needs this much either.
            self.spoiled = true;
            self.body.clear();
            return;
        }
        self.body.push(b);
        let near = |p: &[u8]| p.starts_with(&self.body) || self.body.starts_with(p);
        let wanted = near(b"7501;") || near(b"133;");
        if !wanted {
            self.spoiled = true;
            self.body.clear();
        }
    }

    /// The OSC string ended with a terminator.
    fn finish(&mut self, at: usize, hits: &mut Vec<Hit>) {
        if !self.collect || self.spoiled {
            return;
        }
        let body = std::mem::take(&mut self.body);
        if let Some(rest) = body.strip_prefix(b"7501;") {
            if rest == b"?" {
                hits.push(Hit::Detect(at + 1));
            } else if let Some(report) = parse_report(rest) {
                hits.push(Hit::Report(report));
            }
        } else if body == b"133;A" || body.starts_with(b"133;A;") {
            hits.push(Hit::Prompt);
        }
        self.body = body;
        self.body.clear();
    }

    fn csi(&self, fin: u8, at: usize, hits: &mut Vec<Hit>) {
        let params = std::str::from_utf8(&self.params).unwrap_or("");
        match fin {
            b'h' if params.starts_with('?')
                && params[1..]
                    .split(';')
                    .any(|p| matches!(p, "47" | "1047" | "1049")) =>
            {
                hits.push(Hit::EnterAlt(at));
            }
            b'r' if !params.starts_with('?') && !params.contains(' ') => {
                let mut it = params.split(';').map(|p| p.parse::<usize>().unwrap_or(0));
                let (top, bottom) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
                hits.push(Hit::Region(if bottom == 0 && top <= 1 {
                    None
                } else {
                    Some((top.max(1) - 1, bottom.max(1) - 1))
                }));
            }
            b'p' if params == "!" => hits.push(Hit::Reset),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "emulator_tests.rs"]
mod tests;
