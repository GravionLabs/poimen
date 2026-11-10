//! The local terminal while the TUI runs: raw mode, the alternate screen, mouse capture and
//! bracketed paste, switched on by [`ScreenGuard`] and restored when it drops (also on an error
//! or an unwinding panic) and by the panic hook of [`install_panic_hook`] (release builds abort
//! on a panic, so no drop runs there).

use std::io::{self, Write};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};

use crate::tty::{LocalTerminal, Terminal};

/// What the TUI switches on and off in the local terminal.
pub trait Screen {
    fn enter(&mut self) -> io::Result<()>;
    fn leave(&mut self) -> io::Result<()>;
}

/// The process's terminal.
#[derive(Debug, Default)]
pub struct LocalScreen;

impl Screen for LocalScreen {
    fn enter(&mut self) -> io::Result<()> {
        LocalTerminal.enable_raw()?;
        let mut out = io::stdout();
        if let Err(e) = execute!(
            out,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            let _ = restore();
            return Err(e);
        }
        Ok(())
    }

    fn leave(&mut self) -> io::Result<()> {
        restore()
    }
}

/// Puts the local terminal back: every step is tried even when an earlier one fails.
fn restore() -> io::Result<()> {
    let mut out = io::stdout();
    let screen = execute!(
        out,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = out.flush();
    let raw = LocalTerminal.disable_raw();
    screen.and(raw)
}

/// The screen switched on for the life of the guard.
pub struct ScreenGuard<'a, S: Screen> {
    screen: &'a mut S,
}

impl<'a, S: Screen> ScreenGuard<'a, S> {
    pub fn new(screen: &'a mut S) -> io::Result<Self> {
        screen.enter()?;
        Ok(Self { screen })
    }
}

impl<S: Screen> Drop for ScreenGuard<'_, S> {
    fn drop(&mut self) {
        let _ = self.screen.leave();
    }
}

/// Restores the local terminal before the panic message is printed, so it is readable and the
/// shell is usable even when the panic aborts.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        on: bool,
        calls: Vec<&'static str>,
        fail_enter: bool,
    }
    impl Screen for Fake {
        fn enter(&mut self) -> io::Result<()> {
            self.calls.push("enter");
            if self.fail_enter {
                return Err(io::Error::other("not a terminal"));
            }
            self.on = true;
            Ok(())
        }
        fn leave(&mut self) -> io::Result<()> {
            self.calls.push("leave");
            self.on = false;
            Ok(())
        }
    }

    #[test]
    fn guard_restores_on_drop() {
        let mut s = Fake::default();
        {
            let _g = ScreenGuard::new(&mut s).unwrap();
        }
        assert!(!s.on);
        assert_eq!(s.calls, ["enter", "leave"]);
    }

    #[test]
    fn guard_restores_on_an_error_return_and_a_panic() {
        let mut s = Fake::default();
        let run = |s: &mut Fake| -> io::Result<()> {
            let _g = ScreenGuard::new(s)?;
            Err(io::Error::other("server gone"))
        };
        assert!(run(&mut s).is_err());
        assert!(!s.on);

        let mut s = Fake::default();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = ScreenGuard::new(&mut s).unwrap();
            panic!("boom");
        }));
        assert!(r.is_err());
        assert!(!s.on);
        assert_eq!(s.calls, ["enter", "leave"]);
    }

    #[test]
    fn a_failed_enter_is_an_error_without_a_guard() {
        let mut s = Fake {
            fail_enter: true,
            ..Fake::default()
        };
        assert!(ScreenGuard::new(&mut s).is_err());
        assert_eq!(s.calls, ["enter"]);
    }
}
