//! The pane's terminal state: an `alacritty_terminal` grid fed with the attach snapshot and the
//! stream bytes, drawn as ratatui cells. Colors come from the user's scheme, or without one stay
//! the local terminal's own (default foreground and background, ANSI palette indices).
//!
//! As the controller the TUI is the terminal the program talks to, so it answers the program's
//! queries (cursor position, device attributes, colors) from this grid ([`Grid::take_replies`]);
//! the server only answers them while no controller is attached.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AColor, NamedColor, Processor, Rgb, StdSyncHandler};
use poimen_core::schemes::Scheme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::input::Modes;

/// Scrollback lines the TUI keeps per pane (the snapshot brings the server's).
pub const SCROLLBACK: usize = 10_000;

/// The colors cells are drawn with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Palette {
    /// `None`: the local terminal's default.
    pub foreground: Option<(u8, u8, u8)>,
    pub background: Option<(u8, u8, u8)>,
    pub cursor: Option<(u8, u8, u8)>,
    /// `None`: ANSI indices, colored by the local terminal.
    pub ansi: Option<[(u8, u8, u8); 16]>,
}

fn hex(text: &str) -> Option<(u8, u8, u8)> {
    let h = text.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

impl Palette {
    pub fn from_scheme(scheme: Option<&Scheme>) -> Self {
        let Some(s) = scheme else {
            return Self::default();
        };
        let ansi: Vec<_> = s.ansi.iter().filter_map(|c| hex(c)).collect();
        Self {
            foreground: hex(&s.foreground),
            background: hex(&s.background),
            cursor: hex(&s.cursor),
            ansi: <[(u8, u8, u8); 16]>::try_from(ansi).ok(),
        }
    }

    fn rgb((r, g, b): (u8, u8, u8)) -> Color {
        Color::Rgb(r, g, b)
    }

    fn ansi(&self, i: u8) -> Color {
        match &self.ansi {
            Some(a) if i < 16 => Self::rgb(a[usize::from(i)]),
            _ => Color::Indexed(i),
        }
    }

    /// A cell color; `dim` is set for the dim named colors.
    fn color(&self, c: AColor, dim: &mut bool) -> Color {
        match c {
            AColor::Spec(Rgb { r, g, b }) => Color::Rgb(r, g, b),
            AColor::Indexed(i) => self.ansi(i),
            AColor::Named(n) => {
                let i = n as usize;
                match n {
                    NamedColor::Foreground | NamedColor::BrightForeground => {
                        self.foreground.map_or(Color::Reset, Self::rgb)
                    }
                    NamedColor::DimForeground => {
                        *dim = true;
                        self.foreground.map_or(Color::Reset, Self::rgb)
                    }
                    NamedColor::Background => self.background.map_or(Color::Reset, Self::rgb),
                    NamedColor::Cursor => self.cursor.map_or(Color::Reset, Self::rgb),
                    _ if i < 16 => self.ansi(i as u8),
                    _ => {
                        // DimBlack..=DimWhite follow the cursor color in the enum.
                        *dim = true;
                        let base = i - NamedColor::DimBlack as usize;
                        self.ansi(u8::try_from(base.min(7)).unwrap_or(7))
                    }
                }
            }
        }
    }

    /// The color a query for `index` is answered with (alacritty's numbering: 0..=255 palette,
    /// 256 foreground, 257 background, 258 cursor).
    fn answer(&self, index: usize) -> Option<Rgb> {
        let rgb = |(r, g, b): (u8, u8, u8)| Rgb { r, g, b };
        let fg = self.foreground.unwrap_or(DEFAULT_FG);
        match index {
            0..=15 => Some(rgb(self.ansi.unwrap_or(DEFAULT_ANSI)[index])),
            16..=231 => {
                let i = u8::try_from(index - 16).ok()?;
                let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
                Some(rgb((level(i / 36), level(i / 6 % 6), level(i % 6))))
            }
            232..=255 => {
                let v = 8 + 10 * u8::try_from(index - 232).ok()?;
                Some(rgb((v, v, v)))
            }
            256 => Some(rgb(fg)),
            257 => Some(rgb(self.background.unwrap_or(DEFAULT_BG))),
            258 => Some(rgb(self.cursor.unwrap_or(fg))),
            _ => None,
        }
    }
}

/// Answers to color queries without a scheme: a dark scheme (the server's default too).
const DEFAULT_FG: (u8, u8, u8) = (0xd8, 0xd8, 0xd8);
const DEFAULT_BG: (u8, u8, u8) = (0x18, 0x18, 0x18);
const DEFAULT_ANSI: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x00, 0x00),
    (0x00, 0xcd, 0x00),
    (0xcd, 0xcd, 0x00),
    (0x00, 0x00, 0xee),
    (0xcd, 0x00, 0xcd),
    (0x00, 0xcd, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x7f, 0x7f, 0x7f),
    (0xff, 0x00, 0x00),
    (0x00, 0xff, 0x00),
    (0xff, 0xff, 0x00),
    (0x5c, 0x5c, 0xff),
    (0xff, 0x00, 0xff),
    (0x00, 0xff, 0xff),
    (0xff, 0xff, 0xff),
];

type Formatter = Arc<dyn Fn(Rgb) -> String + Send + Sync>;

#[derive(Default)]
struct Shared {
    replies: Vec<String>,
    color_requests: Vec<(usize, Formatter)>,
}

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

pub struct Grid {
    term: Term<Listener>,
    processor: Processor<StdSyncHandler>,
    listener: Listener,
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let listener = Listener::default();
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let term = Term::new(
            config,
            &Size {
                cols: usize::from(cols.max(1)),
                rows: usize::from(rows.max(1)),
            },
            listener.clone(),
        );
        Self {
            term,
            processor: Processor::new(),
            listener,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    /// Columns and rows of the terminal (the PTY's size, not the pane's).
    pub fn size(&self) -> (u16, u16) {
        (
            u16::try_from(self.term.columns()).unwrap_or(u16::MAX),
            u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX),
        )
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if self.size() != (cols.max(1), rows.max(1)) {
            self.term.resize(Size {
                cols: usize::from(cols.max(1)),
                rows: usize::from(rows.max(1)),
            });
        }
    }

    /// Scrolls the view by `lines` (positive: back into the scrollback).
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines the view is scrolled back.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn modes(&self) -> Modes {
        let m = *self.term.mode();
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            mouse_report: m.intersects(TermMode::MOUSE_MODE),
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
        }
    }

    /// The cursor on the screen (column, row), when it is shown and the view is not scrolled.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        if !self.term.mode().contains(TermMode::SHOW_CURSOR) || self.display_offset() > 0 {
            return None;
        }
        let p = self.term.grid().cursor.point;
        Some((
            u16::try_from(p.column.0).ok()?,
            u16::try_from(p.line.0).ok()?,
        ))
    }

    /// The answers to the queries fed so far, as bytes for the PTY; taken once.
    pub fn take_replies(&mut self, palette: &Palette) -> Vec<u8> {
        let (replies, requests) = {
            let mut shared = self.listener.lock();
            (
                std::mem::take(&mut shared.replies),
                std::mem::take(&mut shared.color_requests),
            )
        };
        let mut out = replies.concat().into_bytes();
        for (index, format) in requests {
            let own = self.term.colors()[index];
            if let Some(rgb) = own.or_else(|| palette.answer(index)) {
                out.extend(format(rgb).into_bytes());
            }
        }
        out
    }

    /// Draws the visible screen into `area` of `buf`: clipped when the terminal is larger than
    /// the area, padded with blank cells when it is smaller.
    pub fn render(&self, area: Rect, buf: &mut Buffer, palette: &Palette) {
        let blank = Style::default()
            .fg(palette.foreground.map_or(Color::Reset, Palette::rgb))
            .bg(palette.background.map_or(Color::Reset, Palette::rgb));
        buf.set_style(area, blank);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
            }
        }
        let grid = self.term.grid();
        let offset = i32::try_from(grid.display_offset()).unwrap_or(0);
        let rows = self.term.screen_lines().min(usize::from(area.height));
        let cols = self.term.columns().min(usize::from(area.width));
        for row in 0..rows {
            let line = &grid[Line(row as i32 - offset)];
            let y = area.y + row as u16;
            for col in 0..cols {
                let cell = &line[Column(col)];
                let x = area.x + col as u16;
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    // Covered by the wide character left of it.
                    continue;
                }
                let mut dim = cell.flags.contains(Flags::DIM);
                let mut fg = palette.color(cell.fg, &mut dim);
                let mut bg = palette.color(cell.bg, &mut false);
                // With a scheme both colors are known and are swapped here; without one the local
                // terminal swaps its own defaults (`REVERSED`).
                let themed = palette.foreground.is_some() && palette.background.is_some();
                if cell.flags.contains(Flags::INVERSE) && themed {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let mut mods = Modifier::empty();
                let f = cell.flags;
                if f.contains(Flags::BOLD) {
                    mods |= Modifier::BOLD;
                }
                if dim {
                    mods |= Modifier::DIM;
                }
                if f.contains(Flags::ITALIC) {
                    mods |= Modifier::ITALIC;
                }
                if f.intersects(Flags::ALL_UNDERLINES) {
                    mods |= Modifier::UNDERLINED;
                }
                if f.contains(Flags::HIDDEN) {
                    mods |= Modifier::HIDDEN;
                }
                if f.contains(Flags::STRIKEOUT) {
                    mods |= Modifier::CROSSED_OUT;
                }
                if f.contains(Flags::INVERSE) && !themed {
                    mods |= Modifier::REVERSED;
                }
                let wide = f.contains(Flags::WIDE_CHAR);
                let mut symbol = String::new();
                if (cell.c == '\0' || cell.c == ' ' || f.contains(Flags::LEADING_WIDE_CHAR_SPACER))
                    || (wide && col + 1 >= cols)
                {
                    symbol.push(' ');
                } else {
                    symbol.push(cell.c);
                    for z in cell.zerowidth().unwrap_or(&[]) {
                        symbol.push(*z);
                    }
                }
                buf[(x, y)]
                    .set_symbol(&symbol)
                    .set_style(Style::default().fg(fg).bg(bg).add_modifier(mods));
                if wide && col + 1 < cols {
                    buf[(x + 1, y)].reset();
                    buf[(x + 1, y)].set_style(Style::default().bg(bg));
                    buf[(x + 1, y)].set_symbol("");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(buf: &Buffer) -> Vec<String> {
        let a = buf.area;
        (a.top()..a.bottom())
            .map(|y| {
                (a.left()..a.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn modes_follow_the_program() {
        let mut g = Grid::new(20, 5);
        let m = g.modes();
        assert!(!m.app_cursor && !m.bracketed_paste && !m.mouse_report && !m.alt_screen);
        // On by default, as in xterm: the wheel scrolls a full-screen program.
        assert!(m.alternate_scroll);
        g.feed(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1049h");
        let m = g.modes();
        assert!(m.app_cursor && m.bracketed_paste && m.mouse_report && m.sgr_mouse);
        assert!(m.alt_screen);
    }

    #[test]
    fn cursor_is_hidden_when_scrolled_or_turned_off() {
        let mut g = Grid::new(10, 3);
        g.feed(b"ab");
        assert_eq!(g.cursor(), Some((2, 0)));
        g.feed(b"\x1b[?25l");
        assert_eq!(g.cursor(), None);
        g.feed(b"\x1b[?25h");
        for i in 0..10 {
            g.feed(format!("\r\nline{i}").as_bytes());
        }
        g.scroll(2);
        assert_eq!(g.display_offset(), 2);
        assert_eq!(g.cursor(), None);
        g.scroll_to_bottom();
        assert!(g.cursor().is_some());
    }

    #[test]
    fn scrolling_shows_the_scrollback() {
        let mut g = Grid::new(10, 2);
        g.feed(b"one\r\ntwo\r\nthree");
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        g.render(area, &mut buf, &Palette::default());
        assert_eq!(text(&buf), ["two", "three"]);
        g.scroll(1);
        g.render(area, &mut buf, &Palette::default());
        assert_eq!(text(&buf), ["one", "two"]);
    }

    #[test]
    fn device_queries_are_answered_once() {
        let mut g = Grid::new(10, 3);
        g.feed(b"ab\x1b[6n");
        assert_eq!(g.take_replies(&Palette::default()), b"\x1b[1;3R");
        assert!(g.take_replies(&Palette::default()).is_empty());
        g.feed(b"\x1b]11;?\x07");
        let bg = String::from_utf8(g.take_replies(&Palette::default())).unwrap();
        assert!(bg.contains("rgb:1818/1818/1818"), "{bg:?}");
    }

    #[test]
    fn scheme_colors_replace_the_ansi_indices() {
        let mut p = Palette {
            ansi: Some([(1, 2, 3); 16]),
            foreground: Some((9, 9, 9)),
            ..Palette::default()
        };
        p.background = Some((0, 0, 0));
        let mut dim = false;
        assert_eq!(
            p.color(AColor::Named(NamedColor::Red), &mut dim),
            Color::Rgb(1, 2, 3)
        );
        assert_eq!(p.color(AColor::Indexed(200), &mut dim), Color::Indexed(200));
        assert!(!dim);
        assert_eq!(
            p.color(AColor::Named(NamedColor::DimGreen), &mut dim),
            Color::Rgb(1, 2, 3)
        );
        assert!(dim);
        let none = Palette::default();
        assert_eq!(
            none.color(AColor::Named(NamedColor::Foreground), &mut dim),
            Color::Reset
        );
        assert_eq!(
            none.color(AColor::Named(NamedColor::Blue), &mut dim),
            Color::Indexed(4)
        );
    }

    #[test]
    fn palette_from_a_scheme() {
        assert_eq!(Palette::from_scheme(None), Palette::default());
        let s = Scheme {
            name: "x".into(),
            label: "x".into(),
            background: "#000000".into(),
            foreground: "#ffffff".into(),
            cursor: "#ff0000".into(),
            selection_background: "#ffffff".into(),
            selection_foreground: "#000000".into(),
            ansi: vec!["#010203".into(); 16],
        };
        let p = Palette::from_scheme(Some(&s));
        assert_eq!(p.foreground, Some((255, 255, 255)));
        assert_eq!(p.cursor, Some((255, 0, 0)));
        assert_eq!(p.ansi.unwrap()[15], (1, 2, 3));
    }
}
