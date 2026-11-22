//! What the attach snapshot writes besides the cells: the SGR pen, the kitty keyboard flags and
//! OSC 8 hyperlinks (split from `emulator.rs`).

use std::fmt::Write as _;

use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Flags, Hyperlink};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// The kitty keyboard protocol flags of `mode` as the number of `CSI > flags u`.
pub(crate) fn kitty_flags(mode: TermMode) -> u8 {
    [
        (TermMode::DISAMBIGUATE_ESC_CODES, 1),
        (TermMode::REPORT_EVENT_TYPES, 2),
        (TermMode::REPORT_ALTERNATE_KEYS, 4),
        (TermMode::REPORT_ALL_KEYS_AS_ESC, 8),
        (TermMode::REPORT_ASSOCIATED_TEXT, 16),
    ]
    .into_iter()
    .filter(|(flag, _)| mode.contains(*flag))
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// An OSC 8 sequence that opens `link` or, with `None`, closes the open one.
pub(crate) fn link_sequence(out: &mut String, link: Option<&Hyperlink>) {
    let clean = |text: &str| text.chars().filter(|c| !c.is_control()).collect::<String>();
    match link {
        Some(link) => {
            let id = clean(link.id());
            let id = if id.contains([';', ':']) {
                String::new()
            } else {
                format!("id={id}")
            };
            let _ = write!(out, "\x1b]8;{id};{}\x1b\\", clean(link.uri()));
        }
        None => out.push_str("\x1b]8;;\x1b\\"),
    }
}

/// The SGR state of the stream being written, so only changes are emitted.
#[derive(Default)]
pub(crate) struct Pen {
    state: Option<(Color, Color, Flags)>,
}

const DEFAULT_PEN: (Color, Color, Flags) = (
    Color::Named(NamedColor::Foreground),
    Color::Named(NamedColor::Background),
    Flags::empty(),
);

impl Pen {
    pub(crate) fn apply(&mut self, out: &mut String, fg: Color, bg: Color, flags: Flags) {
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

    pub(crate) fn reset(&mut self, out: &mut String) {
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
