//! System notifications of the TUI through `notify-rust` (macOS, Linux and Windows toasts). A
//! failure is ignored: a missing notification daemon must not disturb the terminal.

use super::attention::Sink;

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSink;

impl Sink for SystemSink {
    fn send(&self, title: &str, body: &str) {
        let _ = notify_rust::Notification::new()
            .appname("poimen")
            .summary(title)
            .body(body)
            .show();
    }
}
