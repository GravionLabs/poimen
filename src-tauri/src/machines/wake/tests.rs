//! A resume and a network return wake the machines that are not connected (#1011), with a fake
//! `Link` and a `Pause` that only ends when woken (so a retry can only come from the trigger).

use super::super::*;
use super::{Trigger, Watch, reconnect_all};
use std::net::IpAddr;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, atomic::AtomicU32};

/// Fails every connect and tells the test about it.
struct Failing {
    attempts: AtomicU32,
    tx: Mutex<Sender<u32>>,
}

impl Link for Failing {
    fn connect(&self) -> Result<(PingResult, Option<Session>), String> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = lock(&self.tx).send(n);
        Err("no route to host".into())
    }
}

/// Waits until woken, whatever the delay: the backoff would be 1 s, then 2 s, ...
struct UntilWoken;

impl Pause for UntilWoken {
    fn pause(&self, shared: &Shared, _delay: Option<Duration>) -> bool {
        shared.wait(None)
    }
}

struct NoConnector;

impl Connector for NoConnector {
    fn connect(&self) -> io::Result<Pipe> {
        Err(not_connected())
    }
}

fn config() -> MachineConfig {
    MachineConfig {
        name: "box".into(),
        ssh: "box".into(),
        repos_dir: "~/Source/{owner}/{repo}".into(),
        server_path: None,
    }
}

/// A machine that has failed its first attempt and now sits in its pause.
fn offline_machine() -> (Machines, Receiver<u32>) {
    let (tx, rx) = channel();
    let link = Mutex::new(Some(Failing {
        attempts: AtomicU32::new(0),
        tx: Mutex::new(tx),
    }));
    let machines = Machines::default();
    machines.sync(&[config()], |c| {
        Machine::start_with(
            c.clone(),
            Arc::new(NoConnector),
            Arc::new(UntilWoken),
            |_| {},
            |_| {},
            |_| Box::new(lock(&link).take().unwrap()),
        )
    });
    // The failed attempt is the first signal; the second only comes from a wake.
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(1));
    (machines, rx)
}

fn ip(last: u8) -> Option<IpAddr> {
    Some(IpAddr::from([10, 0, 0, last]))
}

fn retries_after(trigger: Option<Trigger>, expected: Trigger) {
    let (machines, rx) = offline_machine();
    assert_eq!(trigger, Some(expected));
    reconnect_all(&machines);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Ok(2),
        "the machine tries again at once"
    );
    // And again: the loop never ends for good while the machine is configured.
    reconnect_all(&machines);
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(3));
}

#[test]
fn a_resume_from_sleep_retries_an_offline_machine_at_once() {
    let mut watch = Watch::default();
    watch.observe(Duration::from_secs(100), ip(2));
    let trigger = watch.observe(Duration::from_secs(5000), ip(2));
    retries_after(trigger, Trigger::Resumed);
}

#[test]
fn the_network_coming_back_retries_an_offline_machine_at_once() {
    let mut watch = Watch::default();
    watch.observe(Duration::from_secs(100), ip(2));
    watch.observe(Duration::from_secs(103), None);
    let trigger = watch.observe(Duration::from_secs(106), ip(2));
    retries_after(trigger, Trigger::NetworkChanged);
}

#[test]
fn the_ssh_keep_alive_ends_a_half_open_connection_within_a_minute() {
    let args = ssh_args("box", "poimen-server");
    let value = |key: &str| -> u32 {
        let prefix = format!("{key}=");
        args.iter()
            .find_map(|a| a.strip_prefix(&prefix))
            .unwrap()
            .parse()
            .unwrap()
    };
    assert!(value("ServerAliveInterval") * value("ServerAliveCountMax") <= 60);
}
