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
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color, CursorShape, NamedColor, Processor, Rgb, StdSyncHandler,
};

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
pub const DETECTION_REPLY: &str =
    "\x1b]7501;?:states=idle,working,blocked,done,error,clear:kinds=permission,question,auth\x1b\\";

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
        out.into_bytes()
    }
}

fn is_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    (cell.c == ' ' || cell.c == '\0')
        && matches!(cell.bg, Color::Named(NamedColor::Background))
        && !cell
            .flags
            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT | Flags::WRAPLINE)
        && cell.zerowidth().is_none()
}

/// The SGR state of the stream being written, so only changes are emitted.
#[derive(Default)]
struct Pen {
    state: Option<(Color, Color, Flags)>,
}

const DEFAULT_PEN: (Color, Color, Flags) = (
    Color::Named(NamedColor::Foreground),
    Color::Named(NamedColor::Background),
    Flags::empty(),
);

impl Pen {
    fn apply(&mut self, out: &mut String, fg: Color, bg: Color, flags: Flags) {
        let flags = flags
            & (Flags::INVERSE
                | Flags::BOLD
                | Flags::ITALIC
                | Flags::DIM
                | Flags::HIDDEN
                | Flags::STRIKEOUT
                | Flags::ALL_UNDERLINES);
        let wanted = (fg, bg, flags);
        if self.state.unwrap_or(DEFAULT_PEN) == wanted {
            return;
        }
        out.push_str("\x1b[0");
        if flags.contains(Flags::BOLD) {
            out.push_str(";1");
        }
        if flags.contains(Flags::DIM) {
            out.push_str(";2");
        }
        if flags.contains(Flags::ITALIC) {
            out.push_str(";3");
        }
        if flags.contains(Flags::DOUBLE_UNDERLINE) {
            out.push_str(";21");
        } else if flags.contains(Flags::UNDERCURL) {
            out.push_str(";4:3");
        } else if flags.intersects(Flags::ALL_UNDERLINES) {
            out.push_str(";4");
        }
        if flags.contains(Flags::INVERSE) {
            out.push_str(";7");
        }
        if flags.contains(Flags::HIDDEN) {
            out.push_str(";8");
        }
        if flags.contains(Flags::STRIKEOUT) {
            out.push_str(";9");
        }
        color(out, fg, 30, 90, 38);
        color(out, bg, 40, 100, 48);
        out.push('m');
        self.state = Some(wanted);
    }

    fn reset(&mut self, out: &mut String) {
        if self.state.is_some_and(|s| s != DEFAULT_PEN) {
            out.push_str("\x1b[0m");
        }
        self.state = None;
    }
}

fn color(out: &mut String, c: Color, base: u8, bright: u8, ext: u8) {
    let n = match c {
        Color::Spec(Rgb { r, g, b }) => {
            let _ = write!(out, ";{ext};2;{r};{g};{b}");
            return;
        }
        Color::Indexed(i) => usize::from(i),
        Color::Named(n) => match n as usize {
            i @ 0..=15 => i,
            // The dim variants are the plain colors; DIM carries the rest.
            i @ 259..=266 => i - 259,
            _ => return,
        },
    };
    match n {
        0..=7 => {
            let _ = write!(out, ";{}", usize::from(base) + n);
        }
        8..=15 => {
            let _ = write!(out, ";{}", usize::from(bright) + n - 8);
        }
        _ => {
            let _ = write!(out, ";{ext};5;{n}");
        }
    }
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
}

/// Longest OSC 7501 sequence read, in bytes after `ESC ]`.
const MAX_REPORT: usize = 4096;
const MAX_MSG: usize = 2048;
const MAX_TITLE: usize = 192;
const MAX_APP: usize = 32;
const MAX_KEY: usize = 16;
const MAX_ID: usize = 64;

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
                if !clean || value.is_empty() || value.len() > MAX_ID {
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
mod tests {
    use super::*;

    fn fresh(cols: u16, rows: u16) -> Emulator {
        Emulator::new(cols, rows, 1000)
    }

    fn replay(e: &Emulator) -> Emulator {
        let (cols, rows) = e.size();
        let mut copy = Emulator::new(cols, rows, 1000);
        copy.feed(&e.snapshot());
        copy
    }

    fn row_text(e: &Emulator, line: i32) -> String {
        let grid = e.term.grid();
        (0..grid.columns())
            .map(|c| grid[Line(line)][Column(c)].c)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn snapshot_restores_alt_screen_mouse_and_paste_modes() {
        let mut e = fresh(40, 10);
        e.feed(b"\x1b[?1049h\x1b[?1006h\x1b[?2004h");
        let copy = replay(&e);
        for flag in [
            TermMode::ALT_SCREEN,
            TermMode::SGR_MOUSE,
            TermMode::BRACKETED_PASTE,
        ] {
            assert!(copy.mode().contains(flag), "{flag:?}");
        }
        assert!(!copy.mode().contains(TermMode::MOUSE_MODE));
    }

    #[test]
    fn snapshot_restores_each_mouse_mode_and_the_other_input_modes() {
        let cases: [(&[u8], TermMode); 4] = [
            (
                b"\x1b[?1000h\x1b[?1006h",
                TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE,
            ),
            (b"\x1b[?1002h", TermMode::MOUSE_DRAG),
            (
                b"\x1b[?1003h\x1b[?1004h",
                TermMode::MOUSE_MOTION | TermMode::FOCUS_IN_OUT,
            ),
            (
                b"\x1b[?1h\x1b=\x1b[?25l",
                TermMode::APP_CURSOR | TermMode::APP_KEYPAD,
            ),
        ];
        for (input, expected) in cases {
            let mut e = fresh(40, 10);
            e.feed(input);
            assert!(e.mode().contains(expected), "setup {input:?}");
            let copy = replay(&e);
            assert_eq!(copy.mode(), e.mode(), "{input:?}");
        }
    }

    #[test]
    fn snapshot_restores_cells_cursor_style_title_and_region() {
        let mut e = fresh(20, 8);
        e.feed(b"\x1b]2;my title\x07\x1b[2;6r\x1b[3 q\x1b[1;31mred\x1b[0m\r\nline2\x1b[4;5H");
        let copy = replay(&e);
        assert_eq!(row_text(&copy, 0), "red");
        assert_eq!(row_text(&copy, 1), "line2");
        assert_eq!(copy.title().as_deref(), Some("my title"));
        assert_eq!(copy.scroll_region(), Some((1, 5)));
        assert_eq!(copy.cursor(), (3, 4));
        let style = copy.term.cursor_style();
        assert_eq!(
            (style.shape, style.blinking),
            (CursorShape::Underline, true)
        );
        let cell = &copy.term.grid()[Line(0)][Column(0)];
        assert_eq!(cell.fg, Color::Named(NamedColor::Red));
        assert!(cell.flags.contains(Flags::BOLD));
    }

    #[test]
    fn snapshot_includes_scrollback() {
        let mut e = fresh(10, 3);
        for i in 0..8 {
            e.feed(format!("row{i}\r\n").as_bytes());
        }
        let copy = replay(&e);
        let grid = copy.term.grid();
        assert_eq!(grid.history_size(), e.term.grid().history_size());
        assert_eq!(
            row_text(&copy, i32::try_from(-(grid.history_size() as i64)).unwrap()),
            "row0"
        );
    }

    #[test]
    fn scrollback_limit_is_respected() {
        let mut e = Emulator::new(10, 3, 5);
        for i in 0..50 {
            e.feed(format!("row{i}\r\n").as_bytes());
        }
        assert_eq!(e.term.grid().history_size(), 5);
    }

    #[test]
    fn alt_screen_keeps_the_primary_screen_for_when_it_is_left() {
        let mut e = fresh(20, 5);
        e.feed(b"shell prompt\r\n\x1b[?1049h\x1b[Halt content");
        let mut copy = replay(&e);
        assert!(copy.mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(row_text(&copy, 0), "alt content");
        copy.feed(b"\x1b[?1049l");
        assert!(!copy.mode().contains(TermMode::ALT_SCREEN));
        assert_eq!(row_text(&copy, 0), "shell prompt");
    }

    #[test]
    fn a_sequence_split_across_feeds_is_still_seen() {
        let mut e = fresh(20, 5);
        e.feed(b"primary\r\n\x1b[?10");
        e.feed(b"49h\x1b[2;4");
        e.feed(b"r");
        let mut copy = replay(&e);
        assert_eq!(e.scroll_region(), Some((1, 3)));
        copy.feed(b"\x1b[?1049l");
        assert_eq!(row_text(&copy, 0), "primary");
    }

    #[test]
    fn wrapped_lines_stay_one_logical_line() {
        let mut e = fresh(5, 4);
        e.feed(b"abcdefgh\r\nnext");
        let copy = replay(&e);
        assert_eq!(row_text(&copy, 0), "abcde");
        assert_eq!(row_text(&copy, 1), "fgh");
        assert!(
            copy.term.grid()[Line(0)][Column(4)]
                .flags
                .contains(Flags::WRAPLINE)
        );
    }

    #[test]
    fn resize_resets_the_region_and_changes_the_size() {
        let mut e = fresh(20, 8);
        e.feed(b"\x1b[2;5r");
        e.resize(30, 12);
        assert_eq!(e.size(), (30, 12));
        assert_eq!(e.scroll_region(), None);
    }

    #[test]
    fn full_reset_clears_the_region() {
        let mut e = fresh(20, 8);
        e.feed(b"\x1b[2;5r\x1bc");
        assert_eq!(e.scroll_region(), None);
    }

    #[test]
    fn scanner_ignores_text_inside_strings() {
        let mut s = Scanner::default();
        assert!(s.scan(b"\x1b]0;title ?1049h r\x07").is_empty());
        assert_eq!(s.scan(b"\x1b[?1049h"), vec![Hit::EnterAlt(7)]);
    }

    fn replies(input: &[u8]) -> String {
        let mut e = fresh(20, 5);
        e.feed(input);
        String::from_utf8(e.take_replies(&Colors::default())).unwrap()
    }

    #[test]
    fn cursor_position_report_is_one_based() {
        assert_eq!(replies(b"\x1b[3;7H\x1b[6n"), "\x1b[3;7R");
    }

    #[test]
    fn device_attributes_are_answered() {
        assert_eq!(replies(b"\x1b[c"), "\x1b[?6c");
        assert!(replies(b"\x1b[>c").starts_with("\x1b[>0;"));
    }

    #[test]
    fn color_queries_come_from_the_scheme_and_a_program_setting_wins() {
        let colors = Colors {
            foreground: Rgb { r: 1, g: 2, b: 3 },
            background: Rgb {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
            },
            ..Colors::default()
        };
        let mut e = fresh(20, 5);
        e.feed(b"\x1b]10;?\x1b\\\x1b]11;?\x07\x1b]4;1;?\x07\x1b]4;232;?\x07");
        let out = String::from_utf8(e.take_replies(&colors)).unwrap();
        assert_eq!(
            out,
            "\x1b]10;rgb:0101/0202/0303\x1b\\\x1b]11;rgb:aaaa/bbbb/cccc\x07\
             \x1b]4;1;rgb:cdcd/0000/0000\x07\x1b]4;232;rgb:0808/0808/0808\x07"
        );
        e.feed(b"\x1b]11;rgb:10/20/30\x07\x1b]11;?\x07");
        assert_eq!(
            String::from_utf8(e.take_replies(&colors)).unwrap(),
            "\x1b]11;rgb:1010/2020/3030\x07"
        );
    }

    #[test]
    fn the_detection_query_is_answered_ahead_of_the_following_da1() {
        assert_eq!(
            replies(b"\x1b]7501;?\x1b\\\x1b[c"),
            format!("{DETECTION_REPLY}\x1b[?6c")
        );
        assert_eq!(replies(b"\x1b]7501;?\x07"), DETECTION_REPLY);
        // What came before the query is answered before it, also in one chunk.
        assert_eq!(
            replies(b"\x1b[c\x1b]7501;?\x07\x1b[c"),
            format!("\x1b[?6c{DETECTION_REPLY}\x1b[?6c")
        );
        // Split over feeds.
        let mut e = fresh(10, 3);
        e.feed(b"\x1b]7501;");
        e.feed(b"?\x1b");
        e.feed(b"\\\x1b[c");
        assert_eq!(
            String::from_utf8(e.take_replies(&Colors::default())).unwrap(),
            format!("{DETECTION_REPLY}\x1b[?6c")
        );
    }

    #[test]
    fn the_detection_answer_is_also_given_separately_and_carries_no_report() {
        let mut e = fresh(10, 3);
        e.feed(b"\x1b]7501;state=blocked:msg=aGk\x07\x1b[c\x1b]7501;?\x07");
        let a = e.take_answers(&Colors::default());
        assert_eq!(a.own, DETECTION_REPLY.as_bytes());
        assert_eq!(a.all, format!("\x1b[?6c{DETECTION_REPLY}").into_bytes());
        assert!(!a.own.windows(3).any(|w| w == b"msg"));
        // A report is not a query and a query with more is not either.
        assert!(replies(b"\x1b]7501;state=idle\x07\x1b]7501;?x\x07").is_empty());
        assert!(e.take_answers(&Colors::default()).own.is_empty());
    }

    #[test]
    fn replies_are_taken_once() {
        let mut e = fresh(20, 5);
        e.feed(b"\x1b[c");
        assert_eq!(e.take_replies(&Colors::default()), b"\x1b[?6c");
        assert!(e.take_replies(&Colors::default()).is_empty());
    }

    /// The input of `src/app/terminal-view/unicode.spec.ts` (#502): wide characters without
    /// their spacer cells, a row that relies on autowrap, the cursor last. A change here must
    /// be copied there, so xterm.js stays tested against what the server sends.
    #[test]
    fn snapshot_of_wide_characters_and_a_wrapped_row() {
        let mut e = fresh(8, 4);
        e.feed("✅⚠🚀ab\r\n0123456789\x1b[1;6H".as_bytes());
        assert_eq!(
            String::from_utf8(e.dump_screen()).unwrap(),
            "✅⚠🚀ab\r\n0123456789\r\n\x1b[1;6H"
        );
        let copy = replay(&e);
        assert_eq!(row_text(&copy, 0), "✅ ⚠🚀 ab");
        assert_eq!(row_text(&copy, 1), "01234567");
        assert_eq!(row_text(&copy, 2), "89");
        assert_eq!(copy.cursor(), (0, 5));
    }

    // --- OSC 7501 -------------------------------------------------------------------------

    fn reports(bytes: &[u8]) -> Vec<Report> {
        let mut s = Scanner::default();
        s.scan(bytes)
            .into_iter()
            .filter_map(|h| match h {
                Hit::Report(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn b64(text: &str) -> String {
        poimen_protocol::encode_bytes(text.as_bytes())
    }

    #[test]
    fn a_report_is_parsed_with_either_terminator() {
        for end in ["\x07", "\x1b\\"] {
            let got = reports(format!("a\x1b]7501;state=working:app=codex{end}b").as_bytes());
            assert_eq!(got.len(), 1, "{end:?}");
            assert_eq!(got[0].state, Some(ReportState::Working));
            assert_eq!(got[0].app.as_deref(), Some("codex"));
            assert_eq!(got[0].id, None);
        }
    }

    #[test]
    fn a_report_split_at_every_byte_is_the_same_report() {
        let text = format!(
            "x\x1b]7501;state=blocked:app=aider:msg={}\x1b\\y",
            b64("Allow? é")
        );
        let whole = reports(text.as_bytes());
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].msg.as_deref(), Some("Allow? é"));
        for cut in 1..text.len() {
            let mut s = Scanner::default();
            let (a, b) = text.as_bytes().split_at(cut);
            let mut hits = s.scan(a);
            hits.extend(s.scan(b));
            assert_eq!(hits, vec![Hit::Report(whole[0].clone())], "cut at {cut}");
        }
        let mut s = Scanner::default();
        let mut hits = Vec::new();
        for byte in text.as_bytes() {
            hits.extend(s.scan(std::slice::from_ref(byte)));
        }
        assert_eq!(hits, vec![Hit::Report(whole[0].clone())]);
    }

    #[test]
    fn pairs_follow_the_spec() {
        let got = reports(
            b"\x1b]7501; state = idle :junk:=x:Bad=1:nokey:foo=bar:app=a:app=b:title=aGk\x07",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, Some(ReportState::Idle));
        assert_eq!(got[0].app.as_deref(), Some("b"));
        // Padding is optional.
        assert_eq!(got[0].title.as_deref(), Some("hi"));
        let padded = reports(b"\x1b]7501;state=idle:title=aGk=\x07");
        assert_eq!(padded[0].title.as_deref(), Some("hi"));
        // The last duplicate wins, also for the state.
        let dup = reports(b"\x1b]7501;state=idle:state=done\x07");
        assert_eq!(dup[0].state, Some(ReportState::Done));
        // A value with characters outside the set makes a malformed pair: skipped.
        let bad = reports(b"\x1b]7501;state=idle:app=a b\x07");
        assert_eq!(bad[0].app, None);
        let rich = reports(b"\x1b]7501;state=blocked:progress=40:kind=permission\x07");
        assert_eq!(rich[0].progress, Some(40));
        assert_eq!(rich[0].kind.as_deref(), Some("permission"));
        // Out of range, not a number or an unknown kind: ignored, the state still counts.
        for body in [
            "progress=101",
            "progress=-1",
            "progress=4.5",
            "progress=",
            "kind=other",
        ] {
            let got = reports(format!("\x1b]7501;state=working:{body}\x07").as_bytes());
            assert_eq!(got[0].state, Some(ReportState::Working), "{body}");
            assert_eq!(
                (got[0].progress, got[0].kind.as_deref()),
                (None, None),
                "{body}"
            );
        }
        let child = reports(b"\x1b]7501;state=working:id=sub-1\x07");
        assert_eq!(child[0].id.as_deref(), Some("sub-1"));
    }

    #[test]
    fn invalid_reports_are_dropped_whole() {
        let long_msg = b64(&"a".repeat(2049));
        let long_title = b64(&"a".repeat(193));
        let cases = [
            "state=nope".to_string(),
            "app=codex".to_string(),
            "state=idle:id=".to_string(),
            "state=idle:id=a b".to_string(),
            "state=idle:msg=ab-_".to_string(),
            format!("state=idle:msg={}", b64("a\nb")),
            format!("state=idle:msg={}", b64("a\u{1b}b")),
            "state=idle:msg=/w==".to_string(),
            format!("state=idle:msg={long_msg}"),
            format!("state=idle:title={long_title}"),
            format!("state=idle:app={}", "a".repeat(33)),
            format!("state=idle:{}=1", "a".repeat(17)),
            format!("state=idle:app={}", "a".repeat(5000)),
        ];
        for body in cases {
            let text = format!("\x1b]7501;{body}\x07");
            assert!(reports(text.as_bytes()).is_empty(), "{body:.60}");
        }
        // The limits themselves are fine.
        let ok = format!(
            "\x1b]7501;state=idle:app={}:msg={}:title={}\x07",
            "a".repeat(32),
            b64(&"a".repeat(2048)),
            b64(&"b".repeat(192))
        );
        assert_eq!(reports(ok.as_bytes()).len(), 1);
    }

    #[test]
    fn a_sequence_over_4096_bytes_is_dropped_and_the_next_one_read() {
        let pad = "a".repeat(4096);
        let text = format!("\x1b]7501;state=idle:x={pad}\x07\x1b]7501;state=done\x07");
        let got = reports(text.as_bytes());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, Some(ReportState::Done));
        // Exactly 4096 bytes is still read.
        let fill = "a".repeat(4096 - "7501;state=idle:x=".len());
        let exact = format!("\x1b]7501;state=idle:x={fill}\x07");
        assert_eq!(reports(exact.as_bytes()).len(), 1);
    }

    #[test]
    fn other_strings_and_aborted_sequences_report_nothing() {
        assert!(reports(b"\x1bP7501;state=idle\x07").is_empty());
        assert!(reports(b"\x1b]7500;state=idle\x07").is_empty());
        assert!(reports(b"\x1b]7501;state=idle\x18").is_empty());
        assert!(reports(b"\x1b]7501;state=idle\x1bxy\x07").is_empty());
        // An aborted report does not swallow the next one.
        assert_eq!(
            reports(b"\x1b]7501;state=idle\x1b]7501;state=done\x07").len(),
            1
        );
        assert!(reports(b"\x1b]7501;state=idle").is_empty());
    }

    #[test]
    fn prompts_and_ris_are_signals_decstr_is_not() {
        let mut e = fresh(20, 5);
        e.feed(b"\x1b]133;A\x07\x1b]133;A;k=1\x1b\\\x1b]133;B\x07\x1b]133;C\x07");
        assert_eq!(e.take_signals(), vec![Signal::Prompt, Signal::Prompt]);
        e.feed(b"\x1b[!p");
        assert_eq!(e.take_signals(), vec![]);
        e.feed(b"\x1b]7501;state=working\x07\x1bc");
        let signals = e.take_signals();
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[1], Signal::Ris);
    }

    #[test]
    fn a_report_does_not_change_the_grid() {
        let mut e = fresh(20, 3);
        e.feed(b"ab\x1b]7501;state=working\x07cd");
        let mut plain = fresh(20, 3);
        plain.feed(b"abcd");
        assert_eq!(e.snapshot(), plain.snapshot());
    }
}
