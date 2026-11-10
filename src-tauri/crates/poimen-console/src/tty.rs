//! The local terminal: raw mode and size, behind a trait so the session logic is testable.

use std::io;

pub trait Terminal {
    fn enable_raw(&mut self) -> io::Result<()>;
    fn disable_raw(&mut self) -> io::Result<()>;
    /// Columns and rows of the window.
    fn size(&self) -> (u16, u16);
}

/// The process's terminal, through crossterm (termios on Unix, console modes on Windows).
pub struct LocalTerminal;

impl Terminal for LocalTerminal {
    fn enable_raw(&mut self) -> io::Result<()> {
        crossterm::terminal::enable_raw_mode()
    }
    fn disable_raw(&mut self) -> io::Result<()> {
        crossterm::terminal::disable_raw_mode()
    }
    fn size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }
}

/// Raw mode for the life of the guard: restored on drop, so also on an error or a panic.
pub struct RawGuard<'a, T: Terminal> {
    tty: &'a mut T,
}

impl<'a, T: Terminal> RawGuard<'a, T> {
    pub fn new(tty: &'a mut T) -> io::Result<Self> {
        tty.enable_raw()?;
        Ok(Self { tty })
    }

    pub fn size(&self) -> (u16, u16) {
        self.tty.size()
    }
}

impl<T: Terminal> Drop for RawGuard<'_, T> {
    fn drop(&mut self) {
        let _ = self.tty.disable_raw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        raw: bool,
        calls: Vec<&'static str>,
    }
    impl Terminal for Fake {
        fn enable_raw(&mut self) -> io::Result<()> {
            self.raw = true;
            self.calls.push("on");
            Ok(())
        }
        fn disable_raw(&mut self) -> io::Result<()> {
            self.raw = false;
            self.calls.push("off");
            Ok(())
        }
        fn size(&self) -> (u16, u16) {
            (80, 24)
        }
    }

    #[test]
    fn guard_restores_on_drop_and_on_panic() {
        let mut t = Fake::default();
        {
            let g = RawGuard::new(&mut t).unwrap();
            assert_eq!(g.size(), (80, 24));
        }
        assert!(!t.raw);
        assert_eq!(t.calls, ["on", "off"]);

        let mut t = Fake::default();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = RawGuard::new(&mut t).unwrap();
            panic!("boom");
        }));
        assert!(r.is_err());
        assert!(!t.raw);
    }
}
