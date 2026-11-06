//! The active color scheme for `poimen-server`: while no app is attached to a terminal, the server
//! answers the colors queries of a program itself (`server.set_colors`, see `docs/protocol.md`).
//! The frontend sends the scheme on start and on every change; the last one is kept here and pushed
//! again once the server has been started, so the order of the two does not matter.

use std::sync::Mutex;

use poimen_client::protocol::SetColors;

/// Where the colors go; the server client, or a fake in tests.
pub trait ColorSink {
    fn set_colors(&self, colors: &SetColors) -> Result<(), String>;
}

impl ColorSink for poimen_client::Client {
    fn set_colors(&self, colors: &SetColors) -> Result<(), String> {
        poimen_client::Client::set_colors(self, colors).map_err(|e| e.to_string())
    }
}

/// The last scheme the frontend sent.
#[derive(Clone, Default)]
pub struct TerminalColors(std::sync::Arc<Mutex<Option<SetColors>>>);

impl TerminalColors {
    /// Remembers the scheme and, for the poimen backend (`sink`), sends it. A server that is not
    /// running yet is not an error: `push` sends it after the start.
    pub fn set(&self, colors: SetColors, sink: Option<&dyn ColorSink>) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(colors.clone());
        sink.map_or(Ok(()), |s| s.set_colors(&colors))
    }

    /// Sends the remembered scheme, if any.
    pub fn push(&self, sink: &dyn ColorSink) -> Result<(), String> {
        let colors = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        colors.map_or(Ok(()), |c| sink.set_colors(&c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake(Mutex<Vec<SetColors>>);

    impl ColorSink for Fake {
        fn set_colors(&self, colors: &SetColors) -> Result<(), String> {
            self.0.lock().unwrap().push(colors.clone());
            Ok(())
        }
    }

    fn scheme(bg: &str) -> SetColors {
        SetColors {
            foreground: "#ffffff".into(),
            background: bg.into(),
            cursor: None,
            palette: None,
        }
    }

    #[test]
    fn a_scheme_set_before_the_server_runs_is_pushed_later() {
        let colors = TerminalColors::default();
        colors.set(scheme("#111111"), None).unwrap();
        colors.set(scheme("#222222"), None).unwrap();
        let sink = Fake::default();
        colors.push(&sink).unwrap();
        assert_eq!(*sink.0.lock().unwrap(), vec![scheme("#222222")]);
    }

    #[test]
    fn a_change_is_sent_at_once() {
        let colors = TerminalColors::default();
        let sink = Fake::default();
        colors.set(scheme("#333333"), Some(&sink)).unwrap();
        assert_eq!(sink.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn pushing_without_a_scheme_sends_nothing() {
        let sink = Fake::default();
        TerminalColors::default().push(&sink).unwrap();
        assert!(sink.0.lock().unwrap().is_empty());
    }
}
