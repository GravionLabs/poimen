//! Key routing of the TUI: the prefix key and the keys after it are the TUI's, everything else
//! goes to the focused terminal, encoded as the bytes an xterm sends. Pressing the prefix twice
//! sends it; any other key after the prefix goes through together with it (as in
//! `poimen attach`). The mouse wheel scrolls the scrollback unless the program in the terminal
//! asked for mouse reports or alternate scrolling.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use poimen_client::protocol::SplitDirection;

use super::config::{Config, KeySpec, Keys};
use super::tree::Dir;

/// The modes of the focused terminal that change what a key or the wheel sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    /// DECCKM: cursor keys send `ESC O x`.
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    /// Any mouse reporting mode (1000, 1002, 1003).
    pub mouse_report: bool,
    /// SGR mouse encoding (1006).
    pub sgr_mouse: bool,
    pub alt_screen: bool,
    /// 1007: the wheel sends cursor keys on the alternate screen.
    pub alternate_scroll: bool,
}

/// What an input event turned into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Bytes for the terminal.
    Send(Vec<u8>),
    Detach,
    ToggleSidebar,
    /// Open the notification list.
    Notifications,
    /// Move the sidebar selection by this many terminals.
    Select(isize),
    /// A new tab with a new terminal in the focused space.
    NewTab,
    /// Show the tab this many places on in the space's order (wrapping).
    SwitchTab(isize),
    CloseTab,
    /// Split the focused pane; the new terminal goes right of or below it.
    Split(SplitDirection),
    ClosePane,
    /// Focus the neighboring pane.
    Focus(Dir),
    /// Move the border of the focused pane.
    ResizePane(Dir),
    /// The wheel at a screen position: `up` scrolls back.
    Wheel {
        up: bool,
        column: u16,
        row: u16,
    },
    /// A left click at a screen position.
    Click {
        column: u16,
        row: u16,
    },
    /// The window changed its size.
    Resize,
}

#[derive(Debug)]
pub struct Router {
    prefix: KeySpec,
    keys: Keys,
    pending: bool,
}

impl Router {
    pub fn new(config: &Config) -> Self {
        Self {
            prefix: config.prefix,
            keys: config.keys.clone(),
            pending: false,
        }
    }

    /// Whether the prefix was pressed and the next key is the TUI's.
    pub fn pending(&self) -> bool {
        self.pending
    }

    pub fn route(&mut self, event: &Event, modes: Modes) -> Option<Routed> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(key, modes),
            Event::Paste(text) => Some(Routed::Send(paste(text, modes.bracketed_paste))),
            Event::Mouse(m) => mouse(m),
            Event::Resize(..) => Some(Routed::Resize),
            _ => None,
        }
    }

    fn key(&mut self, key: &KeyEvent, modes: Modes) -> Option<Routed> {
        if !self.pending {
            if self.prefix.matches(key) {
                self.pending = true;
                return None;
            }
            return non_empty(encode_key(key, modes.app_cursor));
        }
        self.pending = false;
        if self.prefix.matches(key) {
            non_empty(encode_key(key, modes.app_cursor))
        } else if let Some(routed) = self.keys.action(key) {
            Some(routed)
        } else {
            let mut bytes = encode_spec(self.prefix, modes.app_cursor);
            bytes.extend(encode_key(key, modes.app_cursor));
            non_empty(bytes)
        }
    }
}

impl Keys {
    /// What the key after the prefix asks for, if it is bound.
    fn action(&self, key: &KeyEvent) -> Option<Routed> {
        let table = [
            (self.detach, Routed::Detach),
            (self.sidebar, Routed::ToggleSidebar),
            (self.notifications, Routed::Notifications),
            (self.next, Routed::Select(1)),
            (self.previous, Routed::Select(-1)),
            (self.new_tab, Routed::NewTab),
            (self.next_tab, Routed::SwitchTab(1)),
            (self.previous_tab, Routed::SwitchTab(-1)),
            (self.close_tab, Routed::CloseTab),
            (self.split_right, Routed::Split(SplitDirection::Right)),
            (self.split_down, Routed::Split(SplitDirection::Down)),
            (self.close_pane, Routed::ClosePane),
            (self.focus_left, Routed::Focus(Dir::Left)),
            (self.focus_down, Routed::Focus(Dir::Down)),
            (self.focus_up, Routed::Focus(Dir::Up)),
            (self.focus_right, Routed::Focus(Dir::Right)),
            (self.resize_left, Routed::ResizePane(Dir::Left)),
            (self.resize_down, Routed::ResizePane(Dir::Down)),
            (self.resize_up, Routed::ResizePane(Dir::Up)),
            (self.resize_right, Routed::ResizePane(Dir::Right)),
        ];
        table
            .into_iter()
            .find(|(spec, _)| spec.matches(key))
            .map(|(_, routed)| routed)
    }
}

fn non_empty(bytes: Vec<u8>) -> Option<Routed> {
    (!bytes.is_empty()).then_some(Routed::Send(bytes))
}

fn mouse(m: &MouseEvent) -> Option<Routed> {
    let (column, row) = (m.column, m.row);
    match m.kind {
        MouseEventKind::ScrollUp => Some(Routed::Wheel {
            up: true,
            column,
            row,
        }),
        MouseEventKind::ScrollDown => Some(Routed::Wheel {
            up: false,
            column,
            row,
        }),
        MouseEventKind::Down(MouseButton::Left) => Some(Routed::Click { column, row }),
        _ => None,
    }
}

fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Line breaks of a paste are carriage returns, as a terminal sends them.
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// What the wheel does over the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wheel {
    /// Scroll the scrollback by lines (positive: back).
    Scroll(i32),
    /// Send to the program (a mouse report, or cursor keys for alternate scrolling).
    Send(Vec<u8>),
}

/// Lines one wheel step scrolls.
pub const WHEEL_LINES: i32 = 3;

/// The wheel at `column`/`row` of the pane (0-based).
pub fn wheel(modes: Modes, up: bool, column: u16, row: u16) -> Wheel {
    if modes.mouse_report {
        let button = if up { 64 } else { 65 };
        let bytes = if modes.sgr_mouse {
            format!("\x1b[<{button};{};{}M", column + 1, row + 1).into_bytes()
        } else {
            let at = |v: u16| u8::try_from(v.saturating_add(33)).unwrap_or(255);
            vec![0x1b, b'[', b'M', 32 + button, at(column), at(row)]
        };
        return Wheel::Send(bytes);
    }
    if modes.alt_screen && modes.alternate_scroll {
        let key = if up { b'A' } else { b'B' };
        let one: &[u8] = if modes.app_cursor {
            &[0x1b, b'O', key]
        } else {
            &[0x1b, b'[', key]
        };
        return Wheel::Send(one.repeat(WHEEL_LINES as usize));
    }
    Wheel::Scroll(if up { WHEEL_LINES } else { -WHEEL_LINES })
}

fn encode_spec(spec: KeySpec, app_cursor: bool) -> Vec<u8> {
    encode_key(&KeyEvent::new(spec.code, spec.mods), app_cursor)
}

/// The xterm modifier parameter: 1 + shift 1 + alt 2 + ctrl 4.
fn modifier_param(mods: KeyModifiers) -> u8 {
    1 + u8::from(mods.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(mods.contains(KeyModifiers::ALT))
        + 4 * u8::from(mods.contains(KeyModifiers::CONTROL))
}

/// The bytes an xterm sends for a key; empty for keys without a sequence.
pub fn encode_key(key: &KeyEvent, app_cursor: bool) -> Vec<u8> {
    let mods = key.modifiers;
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let mut out = Vec::new();
    let with_alt = |mut bytes: Vec<u8>| {
        if alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    let param = modifier_param(mods);
    // `ESC [ 1 ; m x` with modifiers, else `ESC [ x` (or `ESC O x` in application mode).
    let cursor = |x: u8, ss3: bool| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[1;{param}{}", x as char).into_bytes()
        } else if ss3 {
            vec![0x1b, b'O', x]
        } else {
            vec![0x1b, b'[', x]
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[{n};{param}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    match key.code {
        KeyCode::Char(c) if ctrl => {
            let byte = match c.to_ascii_lowercase() {
                l @ 'a'..='z' => Some(l as u8 & 0x1f),
                '@' | ' ' | '2' => Some(0),
                '[' | '3' => Some(0x1b),
                '\\' | '4' => Some(0x1c),
                ']' | '5' => Some(0x1d),
                '^' | '6' => Some(0x1e),
                '_' | '-' | '7' => Some(0x1f),
                '?' | '8' => Some(0x7f),
                _ => None,
            };
            match byte {
                Some(b) => out = with_alt(vec![b]),
                None => out = with_alt(c.to_string().into_bytes()),
            }
        }
        KeyCode::Char(c) => out = with_alt(c.to_string().into_bytes()),
        KeyCode::Enter => out = with_alt(vec![b'\r']),
        KeyCode::Tab => out = with_alt(vec![b'\t']),
        KeyCode::BackTab => out = b"\x1b[Z".to_vec(),
        KeyCode::Backspace => out = with_alt(vec![if ctrl { 0x08 } else { 0x7f }]),
        KeyCode::Esc => out = with_alt(vec![0x1b]),
        KeyCode::Up => out = cursor(b'A', app_cursor),
        KeyCode::Down => out = cursor(b'B', app_cursor),
        KeyCode::Right => out = cursor(b'C', app_cursor),
        KeyCode::Left => out = cursor(b'D', app_cursor),
        KeyCode::Home => out = cursor(b'H', app_cursor),
        KeyCode::End => out = cursor(b'F', app_cursor),
        KeyCode::Insert => out = tilde(2),
        KeyCode::Delete => out = tilde(3),
        KeyCode::PageUp => out = tilde(5),
        KeyCode::PageDown => out = tilde(6),
        KeyCode::F(n @ 1..=4) => out = cursor(b'P' + (n - 1), true),
        KeyCode::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            out = tilde(CODES[usize::from(n - 5)]);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::{self, DEFAULT_PREFIX};
    use serde_json::json;

    fn press(code: KeyCode, mods: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, mods))
    }
    fn ch(c: char) -> Event {
        press(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn prefix() -> Event {
        press(KeyCode::Char('b'), KeyModifiers::CONTROL)
    }
    fn router() -> Router {
        Router::new(&Config::default())
    }
    fn send(bytes: &[u8]) -> Option<Routed> {
        Some(Routed::Send(bytes.to_vec()))
    }

    #[test]
    fn plain_keys_pass_through() {
        let mut r = router();
        let m = Modes::default();
        assert_eq!(r.route(&ch('a'), m), send(b"a"));
        assert_eq!(
            r.route(&press(KeyCode::Enter, KeyModifiers::NONE), m),
            send(b"\r")
        );
        assert_eq!(
            r.route(&press(KeyCode::Char('c'), KeyModifiers::CONTROL), m),
            send(&[0x03])
        );
        assert_eq!(r.route(&ch('é'), m), send("é".as_bytes()));
    }

    #[test]
    fn prefix_then_detach_key_detaches() {
        let mut r = router();
        let m = Modes::default();
        assert_eq!(r.route(&prefix(), m), None);
        assert!(r.pending());
        assert_eq!(r.route(&ch('d'), m), Some(Routed::Detach));
        assert!(!r.pending());
        // Without the prefix, `d` is just a letter.
        assert_eq!(r.route(&ch('d'), m), send(b"d"));
    }

    #[test]
    fn prefix_keys_for_the_sidebar_and_selection() {
        let mut r = router();
        let m = Modes::default();
        r.route(&prefix(), m);
        assert_eq!(r.route(&ch('b'), m), Some(Routed::ToggleSidebar));
        r.route(&prefix(), m);
        assert_eq!(r.route(&ch('N'), m), Some(Routed::Notifications));
        r.route(&prefix(), m);
        assert_eq!(
            r.route(&press(KeyCode::Down, KeyModifiers::NONE), m),
            Some(Routed::Select(1))
        );
        r.route(&prefix(), m);
        assert_eq!(
            r.route(&press(KeyCode::Up, KeyModifiers::NONE), m),
            Some(Routed::Select(-1))
        );
    }

    fn after_prefix(r: &mut Router, e: Event) -> Option<Routed> {
        r.route(&prefix(), Modes::default());
        r.route(&e, Modes::default())
    }

    #[test]
    fn prefix_keys_for_tabs() {
        let mut r = router();
        assert_eq!(after_prefix(&mut r, ch('c')), Some(Routed::NewTab));
        assert_eq!(after_prefix(&mut r, ch('n')), Some(Routed::SwitchTab(1)));
        assert_eq!(after_prefix(&mut r, ch('p')), Some(Routed::SwitchTab(-1)));
        assert_eq!(after_prefix(&mut r, ch('&')), Some(Routed::CloseTab));
        assert!(!r.pending());
        // Without the prefix they are letters for the terminal.
        assert_eq!(r.route(&ch('c'), Modes::default()), send(b"c"));
    }

    #[test]
    fn prefix_keys_for_splits_and_panes() {
        let mut r = router();
        assert_eq!(
            after_prefix(&mut r, ch('%')),
            Some(Routed::Split(SplitDirection::Right))
        );
        assert_eq!(
            after_prefix(&mut r, ch('"')),
            Some(Routed::Split(SplitDirection::Down))
        );
        assert_eq!(after_prefix(&mut r, ch('x')), Some(Routed::ClosePane));
        assert_eq!(
            after_prefix(&mut r, ch('h')),
            Some(Routed::Focus(Dir::Left))
        );
        assert_eq!(
            after_prefix(&mut r, ch('j')),
            Some(Routed::Focus(Dir::Down))
        );
        assert_eq!(after_prefix(&mut r, ch('k')), Some(Routed::Focus(Dir::Up)));
        assert_eq!(
            after_prefix(&mut r, ch('l')),
            Some(Routed::Focus(Dir::Right))
        );
        // Shift gives the resize keys; crossterm reports the capital letter with SHIFT.
        let shifted = |c| press(KeyCode::Char(c), KeyModifiers::SHIFT);
        assert_eq!(
            after_prefix(&mut r, shifted('H')),
            Some(Routed::ResizePane(Dir::Left))
        );
        assert_eq!(
            after_prefix(&mut r, shifted('L')),
            Some(Routed::ResizePane(Dir::Right))
        );
        assert_eq!(
            after_prefix(&mut r, ch('K')),
            Some(Routed::ResizePane(Dir::Up))
        );
        assert_eq!(
            after_prefix(&mut r, ch('J')),
            Some(Routed::ResizePane(Dir::Down))
        );
    }

    #[test]
    fn configured_tab_keys_replace_the_defaults() {
        let c = config::parse(
            &json!({"console": {"keys": {"new_tab": "t", "split_right": "|"}}}),
            &[],
        );
        let mut r = Router::new(&c);
        assert_eq!(after_prefix(&mut r, ch('t')), Some(Routed::NewTab));
        assert_eq!(
            after_prefix(&mut r, ch('|')),
            Some(Routed::Split(SplitDirection::Right))
        );
        // The old key is no action now: it goes through with the prefix.
        assert_eq!(after_prefix(&mut r, ch('c')), send(&[0x02, b'c']));
    }

    #[test]
    fn prefix_twice_sends_the_prefix() {
        let mut r = router();
        let m = Modes::default();
        r.route(&prefix(), m);
        assert_eq!(r.route(&prefix(), m), send(&[0x02]));
        assert!(!r.pending());
    }

    #[test]
    fn other_key_after_prefix_goes_through_with_it() {
        let mut r = router();
        let m = Modes::default();
        r.route(&prefix(), m);
        assert_eq!(r.route(&ch('z'), m), send(&[0x02, b'z']));
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut r = router();
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(r.route(&Event::Key(release), Modes::default()), None);
    }

    #[test]
    fn configured_prefix_and_keys_route() {
        let c = config::parse(
            &json!({"console": {"prefix": "C-a", "keys": {"detach": "q"}}}),
            &[],
        );
        assert_ne!(c.prefix, DEFAULT_PREFIX);
        let mut r = Router::new(&c);
        let m = Modes::default();
        // Ctrl+B is an ordinary key now.
        assert_eq!(r.route(&prefix(), m), send(&[0x02]));
        r.route(&press(KeyCode::Char('a'), KeyModifiers::CONTROL), m);
        assert_eq!(r.route(&ch('q'), m), Some(Routed::Detach));
        r.route(&press(KeyCode::Char('a'), KeyModifiers::CONTROL), m);
        assert_eq!(r.route(&ch('d'), m), send(&[0x01, b'd']));
    }

    #[test]
    fn cursor_keys_follow_application_mode_and_modifiers() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(encode_key(&up, false), b"\x1b[A");
        assert_eq!(encode_key(&up, true), b"\x1bOA");
        let ctrl_right = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(encode_key(&ctrl_right, true), b"\x1b[1;5C");
        let shift_del = KeyEvent::new(KeyCode::Delete, KeyModifiers::SHIFT);
        assert_eq!(encode_key(&shift_del, false), b"\x1b[3;2~");
    }

    #[test]
    fn special_keys_encode_like_xterm() {
        let e = |code, mods| encode_key(&KeyEvent::new(code, mods), false);
        assert_eq!(e(KeyCode::Backspace, KeyModifiers::NONE), [0x7f]);
        assert_eq!(e(KeyCode::Esc, KeyModifiers::NONE), [0x1b]);
        assert_eq!(e(KeyCode::BackTab, KeyModifiers::SHIFT), b"\x1b[Z");
        assert_eq!(e(KeyCode::Char('x'), KeyModifiers::ALT), b"\x1bx");
        assert_eq!(e(KeyCode::F(1), KeyModifiers::NONE), b"\x1bOP");
        assert_eq!(e(KeyCode::F(5), KeyModifiers::NONE), b"\x1b[15~");
        assert_eq!(e(KeyCode::F(12), KeyModifiers::NONE), b"\x1b[24~");
        assert_eq!(e(KeyCode::PageUp, KeyModifiers::NONE), b"\x1b[5~");
        assert_eq!(e(KeyCode::Char(' '), KeyModifiers::CONTROL), [0]);
        assert!(e(KeyCode::CapsLock, KeyModifiers::NONE).is_empty());
    }

    #[test]
    fn paste_is_bracketed_when_the_program_asks() {
        let mut r = router();
        let plain = r.route(&Event::Paste("a\nb".into()), Modes::default());
        assert_eq!(plain, send(b"a\rb"));
        let modes = Modes {
            bracketed_paste: true,
            ..Modes::default()
        };
        assert_eq!(
            r.route(&Event::Paste("x".into()), modes),
            send(b"\x1b[200~x\x1b[201~")
        );
    }

    #[test]
    fn mouse_wheel_and_click_and_resize() {
        let mut r = router();
        let m = Modes::default();
        let ev = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: 4,
                row: 2,
                modifiers: KeyModifiers::NONE,
            })
        };
        assert_eq!(
            r.route(&ev(MouseEventKind::ScrollUp), m),
            Some(Routed::Wheel {
                up: true,
                column: 4,
                row: 2
            })
        );
        assert_eq!(
            r.route(&ev(MouseEventKind::Down(MouseButton::Left)), m),
            Some(Routed::Click { column: 4, row: 2 })
        );
        assert_eq!(r.route(&ev(MouseEventKind::Moved), m), None);
        assert_eq!(r.route(&Event::Resize(80, 24), m), Some(Routed::Resize));
    }

    #[test]
    fn wheel_scrolls_unless_the_program_wants_it() {
        assert_eq!(
            wheel(Modes::default(), true, 0, 0),
            Wheel::Scroll(WHEEL_LINES)
        );
        assert_eq!(
            wheel(Modes::default(), false, 0, 0),
            Wheel::Scroll(-WHEEL_LINES)
        );
        let sgr = Modes {
            mouse_report: true,
            sgr_mouse: true,
            ..Modes::default()
        };
        assert_eq!(
            wheel(sgr, true, 4, 2),
            Wheel::Send(b"\x1b[<64;5;3M".to_vec())
        );
        let legacy = Modes {
            mouse_report: true,
            ..Modes::default()
        };
        assert_eq!(
            wheel(legacy, false, 0, 0),
            Wheel::Send(vec![0x1b, b'[', b'M', 97, 33, 33])
        );
        let alt = Modes {
            alt_screen: true,
            alternate_scroll: true,
            ..Modes::default()
        };
        assert_eq!(
            wheel(alt, true, 0, 0),
            Wheel::Send(b"\x1b[A\x1b[A\x1b[A".to_vec())
        );
    }
}
