//! Helpers of the TUI tests.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;

/// The mirrored screen as lines of text.
pub fn screen_text(screen: &Arc<Mutex<Buffer>>) -> String {
    let buf = screen.lock().unwrap();
    let a = buf.area;
    (a.top()..a.bottom())
        .map(|y| {
            (a.left()..a.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Joins the TUI thread with a deadline: a TUI that does not end fails the test with its screen
/// instead of hanging it (#1355).
pub fn join_within<T>(handle: JoinHandle<T>, screen: &Arc<Mutex<Buffer>>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !handle.is_finished() {
        assert!(
            Instant::now() < deadline,
            "the TUI did not end:\n{}",
            screen_text(screen)
        );
        thread::yield_now();
    }
    handle.join().unwrap()
}
