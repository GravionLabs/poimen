//! Notices a resume from sleep and the return of the network (#1011), so machines that are not
//! connected try at once instead of after their backoff (up to 60 s).
//!
//! No OS API: a ticker compares the wall clock with its own tick (a suspended process sees a
//! gap) and the local address of the default route (a UDP `connect` sends nothing; it only asks
//! the routing table).

use std::net::{IpAddr, UdpSocket};
use std::sync::Arc;

use super::Machines;
use std::time::Duration;

/// How often the watcher looks.
pub const TICK: Duration = Duration::from_secs(3);
/// A tick that took this much longer than `TICK` means the machine slept.
const SLEEP_GAP: Duration = Duration::from_secs(10);

/// Why machines should retry now.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Trigger {
    Resumed,
    NetworkChanged,
}

/// Compares each observation with the one before.
#[derive(Debug, Default)]
pub struct Watch {
    last: Option<(Duration, Option<IpAddr>)>,
}

impl Watch {
    /// `now` is the wall clock since an epoch, `route` the local address of the default route
    /// (`None`: no network). The first observation only records.
    pub fn observe(&mut self, now: Duration, route: Option<IpAddr>) -> Option<Trigger> {
        let previous = self.last.replace((now, route));
        let (then, before) = previous?;
        if now.saturating_sub(then) >= TICK + SLEEP_GAP {
            return Some(Trigger::Resumed);
        }
        // Only a route that appears or changes is a return; losing it needs no retry.
        (route.is_some() && route != before).then_some(Trigger::NetworkChanged)
    }
}

/// The local address the default route would use, or `None` without a route.
pub fn default_route() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // TEST-NET-1: never answers, but the connect needs a route.
    socket.connect("192.0.2.1:9").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

/// Every machine that is not connected tries at once (`Machine::reconnect` ignores a connected one).
fn reconnect_all(machines: &Machines) {
    for status in machines.list() {
        let _ = machines.reconnect(&status.config.name);
    }
}

/// Starts the watcher thread (resume and network return; #1011): machines retry at once. Gives
/// `machines` back.
pub fn start(machines: Arc<Machines>) -> Arc<Machines> {
    let watched = machines.clone();
    std::thread::spawn(move || run(|_| reconnect_all(&watched)));
    machines
}

/// Runs until the process ends; `on_trigger` gets every trigger.
fn run(on_trigger: impl Fn(Trigger)) {
    let mut watch = Watch::default();
    loop {
        std::thread::sleep(TICK);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        if let Some(trigger) = watch.observe(now, default_route()) {
            on_trigger(trigger);
        }
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    fn ip(last: u8) -> Option<IpAddr> {
        Some(IpAddr::from([192, 168, 1, last]))
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_steady_clock_and_route_trigger_nothing() {
        let mut w = Watch::default();
        assert_eq!(w.observe(secs(100), ip(2)), None);
        assert_eq!(w.observe(secs(103), ip(2)), None);
        assert_eq!(w.observe(secs(106), ip(2)), None);
    }

    #[test]
    fn a_gap_in_the_clock_is_a_resume() {
        let mut w = Watch::default();
        w.observe(secs(100), ip(2));
        assert_eq!(w.observe(secs(900), ip(2)), Some(Trigger::Resumed));
        assert_eq!(w.observe(secs(903), ip(2)), None);
    }

    #[test]
    fn a_route_that_returns_or_changes_is_a_network_change() {
        let mut w = Watch::default();
        w.observe(secs(0), ip(2));
        assert_eq!(w.observe(secs(3), None), None, "losing it needs no retry");
        assert_eq!(w.observe(secs(6), ip(2)), Some(Trigger::NetworkChanged));
        assert_eq!(w.observe(secs(9), ip(7)), Some(Trigger::NetworkChanged));
    }
}

#[cfg(test)]
mod tests;
