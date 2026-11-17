//! Install or update `poimen-server` on a Linux machine after a confirmation (ADR 0013, #865).
//!
//! Two phases, split by the user's confirmation:
//!
//! - [`prepare`] changes nothing on the machine: the read-only probe (`uname -sm`, `command -v`),
//!   and, here on this Mac, the download of the release archive of the app's own version and its
//!   check against `SHA256SUMS`. The result is a [`Plan`] with the exact commands.
//! - [`run`] (only after the confirmation) streams the archive through the machine's ssh into a
//!   staging directory, verifies the staged binaries by running `--version`, and only then moves
//!   them into `~/.local/bin`. A failing step stops, names ssh's output and removes the staging
//!   directory; the old binaries stay. No process on the machine is stopped or signalled.
//!
//! Everything outside the process (ssh, the download) goes through [`Remote`] and [`Fetch`], which
//! the tests fake: no test runs ssh or touches the network.

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Stdio;
use std::sync::Mutex;
use std::thread;

use poimen_client::protocol::{CHANNEL, PROTOCOL_VERSION};
use poimen_core::gui_path;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::machines::{MachineConfig, check_target};

/// The repository whose releases carry the server archives.
const RELEASES: &str = "https://github.com/GravionLabs/poimen/releases/download";

/// What ssh's output may fill in an error.
const MAX_OUTPUT: usize = 4096;

/// A bound on a downloaded asset (the archives are a few MiB).
const MAX_DOWNLOAD: usize = 128 * 1024 * 1024;

/// The staging directory of an install, inside the target directory (same file system for `mv`).
const STAGE: &str = "\"$HOME/.local/bin/.poimen-install\"";
const DIR: &str = "\"$HOME/.local/bin\"";

/// The result of a command on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub ok: bool,
    /// stdout and stderr, trimmed.
    pub text: String,
}

/// Runs one command on a machine over ssh, optionally feeding `input` to its stdin.
pub trait Remote: Send + Sync {
    fn run(&self, target: &str, command: &str, input: Option<&[u8]>) -> io::Result<Out>;
}

/// Downloads a public release asset.
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// ssh in batch mode: never a prompt, bounded connect.
pub struct Ssh;

impl Remote for Ssh {
    fn run(&self, target: &str, command: &str, input: Option<&[u8]>) -> io::Result<Out> {
        check_target(target).map_err(io::Error::other)?;
        let program = gui_path::resolve_program("ssh", &gui_path::child_path())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ssh is not on the PATH"))?;
        let mut child = gui_path::command(&program)
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=15",
                "--",
                target,
                command,
            ])
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take();
        let data = input.map(<[u8]>::to_vec);
        // Writing and reading run apart, so a full pipe in either direction cannot stall.
        let writer = thread::spawn(move || {
            if let (Some(stdin), Some(data)) = (stdin.as_mut(), data) {
                let _ = stdin.write_all(&data);
            }
            drop(stdin);
        });
        let output = child.wait_with_output()?;
        let _ = writer.join();
        Ok(Out {
            ok: output.status.success(),
            text: join_output(&output.stdout, &output.stderr),
        })
    }
}

fn join_output(stdout: &[u8], stderr: &[u8]) -> String {
    let text = [stdout, stderr]
        .iter()
        .map(|b| String::from_utf8_lossy(b).trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    text.chars().take(MAX_OUTPUT).collect()
}

/// `curl` from the login-shell `PATH`: https only, redirects followed, no credential.
pub struct Curl;

impl Fetch for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let program = gui_path::resolve_program("curl", &gui_path::child_path())
            .ok_or("curl is not on the PATH")?;
        let out = gui_path::command(&program)
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-time",
                "300",
                "--max-filesize",
                &MAX_DOWNLOAD.to_string(),
                "--",
                url,
            ])
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            return Err(format!("download of {url} failed: {why}"));
        }
        Ok(out.stdout)
    }
}

/// One step of the install, as the dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanStep {
    pub id: &'static str,
    pub label: &'static str,
    /// What runs on the machine, as shell text (run through `sh -c`).
    pub command: String,
}

/// What the confirmation shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub machine: String,
    pub ssh: String,
    pub version: String,
    /// `stable` or `rc`.
    pub channel: &'static str,
    /// `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl`.
    pub target: &'static str,
    pub archive: String,
    pub origin: String,
    pub sha256: String,
    pub size: usize,
    /// The directory on the machine, as shown.
    pub dir: &'static str,
    pub server: String,
    pub host: String,
    /// The `poimen-server` already on the machine's `PATH`, when there is one.
    pub existing: Option<String>,
    pub steps: Vec<PlanStep>,
}

/// A plan with the verified archive; the archive never leaves this process.
pub struct Prepared {
    pub plan: Plan,
    ssh: String,
    version: String,
    archive: Vec<u8>,
}

/// `Some(reason)` when this build has nothing to install from: a beta has no release.
pub fn unavailable(version: &str) -> Option<String> {
    if version.contains("-beta") {
        return Some(format!(
            "poimen {version} is a beta build without a release, so there is no server archive to install"
        ));
    }
    None
}

/// The release target of `uname -sm`; any other machine is named in the error.
pub fn target_of(uname: &str) -> Result<&'static str, String> {
    match uname.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["Linux", "x86_64"] => Ok("x86_64-unknown-linux-musl"),
        ["Linux", "aarch64" | "arm64"] => Ok("aarch64-unknown-linux-musl"),
        _ => Err(format!(
            "poimen installs the server only on Linux x86_64 and aarch64; this machine reports {:?}",
            uname.trim()
        )),
    }
}

/// The hex digest of `name` in the text of a `SHA256SUMS` file (`<hex>  <name>`, `*` allowed).
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// `'text'` for `sh`.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The steps with their exact commands for this channel's binaries.
fn steps(server: &str, host: &str) -> Vec<PlanStep> {
    vec![
        PlanStep {
            id: "mkdir",
            label: "Create the target directory",
            command: format!("mkdir -p {DIR} && rm -rf {STAGE} && mkdir {STAGE}"),
        },
        PlanStep {
            id: "copy",
            label: "Copy the archive and unpack it into a staging directory",
            command: format!("tar -xzf - -C {STAGE}"),
        },
        PlanStep {
            id: "verify",
            label: "Check the staged binaries (--version)",
            command: format!(
                "cd {STAGE} && chmod 755 {server} {host} && ./{server} --version && ./{host} --version"
            ),
        },
        PlanStep {
            id: "move",
            label: "Move them into place",
            command: format!(
                "mv -f {STAGE}/{host} {DIR}/{host} && mv -f {STAGE}/{server} {DIR}/{server} && rmdir {STAGE}"
            ),
        },
        PlanStep {
            id: "path",
            label: "Look for the server on the PATH of a non-interactive ssh",
            command: format!("command -v {server}"),
        },
    ]
}

/// Probes the machine, downloads and verifies the archive and returns the plan. Nothing is
/// written on the machine.
pub fn prepare(
    remote: &dyn Remote,
    fetch: &dyn Fetch,
    machine: &MachineConfig,
    version: &str,
) -> Result<Prepared, String> {
    if let Some(why) = unavailable(version) {
        return Err(why);
    }
    let server = CHANNEL.named("poimen-server");
    let host = CHANNEL.named("poimen-pty-host");
    let probe = format!("uname -sm; command -v {server} || true");
    let out = remote
        .run(&machine.ssh, &format!("sh -c {}", quote(&probe)), None)
        .map_err(|e| e.to_string())?;
    if !out.ok {
        return Err(format!("ssh to {} failed: {}", machine.ssh, out.text));
    }
    let mut lines = out.text.lines();
    let target = target_of(lines.next().unwrap_or(""))?;
    let existing = lines
        .map(str::trim)
        .find(|l| l.starts_with('/'))
        .map(str::to_owned);
    let archive = format!("poimen-server-{version}-{target}.tar.gz");
    let base = format!("{RELEASES}/v{version}");
    let sums = fetch.get(&format!("{base}/SHA256SUMS"))?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = expected_sum(&sums, &archive)
        .ok_or_else(|| format!("SHA256SUMS of v{version} lists no {archive}"))?;
    let bytes = fetch.get(&format!("{base}/{archive}"))?;
    let sha256 = hex(&Sha256::digest(&bytes));
    if sha256 != expected {
        return Err(format!(
            "{archive} does not match SHA256SUMS (expected {expected}, got {sha256}); nothing was sent to {}",
            machine.name
        ));
    }
    let plan = Plan {
        machine: machine.name.clone(),
        ssh: machine.ssh.clone(),
        version: version.to_owned(),
        channel: if CHANNEL.suffix().is_empty() {
            "stable"
        } else {
            "rc"
        },
        target,
        origin: format!("{base}/{archive}"),
        archive,
        sha256,
        size: bytes.len(),
        dir: "~/.local/bin",
        steps: steps(&server, &host),
        server,
        host,
        existing,
    };
    Ok(Prepared {
        plan,
        ssh: machine.ssh.clone(),
        version: version.to_owned(),
        archive: bytes,
    })
}

/// Progress of one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepEvent {
    pub machine: String,
    pub step: &'static str,
    /// `running`, `ok` or `failed`.
    pub status: &'static str,
    /// ssh's output of a failed step, else the command's output.
    pub output: String,
}

/// The server is reachable by name on a non-interactive `PATH`, or only by its absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// `None`: found by name. `Some(path)`: the connector must call this path.
    pub server_path: Option<String>,
}

fn sh(script: &str) -> String {
    format!("sh -c {}", quote(script))
}

/// Runs the confirmed plan. A failed step stops, removes the staging directory (nothing is
/// moved before the staged binaries ran) and returns the step and its output.
pub fn run(
    remote: &dyn Remote,
    prepared: &Prepared,
    on_step: &dyn Fn(&StepEvent),
) -> Result<Installed, String> {
    let plan = &prepared.plan;
    let event = |step: &'static str, status: &'static str, output: &str| {
        on_step(&StepEvent {
            machine: plan.machine.clone(),
            step,
            status,
            output: output.to_owned(),
        });
    };
    let cleanup = || {
        let _ = remote.run(&prepared.ssh, &sh(&format!("rm -rf {STAGE}")), None);
    };
    for step in &plan.steps {
        event(step.id, "running", "");
        let input = (step.id == "copy").then_some(prepared.archive.as_slice());
        let result = remote.run(&prepared.ssh, &sh(&step.command), input);
        let (ok, text) = match result {
            Ok(Out { ok, text }) => (ok, text),
            Err(e) => (false, e.to_string()),
        };
        // `path` is a question, not a requirement: a missing binary on the PATH is an answer.
        if step.id == "path" {
            event(step.id, "ok", &text);
            let found = ok && text.trim().ends_with(&plan.server);
            return Ok(Installed {
                server_path: (!found).then(|| format!("$HOME/.local/bin/{}", plan.server)),
            });
        }
        let verified = if ok && step.id == "verify" {
            check_versions(&text, plan, &prepared.version)
        } else {
            Ok(())
        };
        match (ok, verified) {
            (true, Ok(())) => event(step.id, "ok", &text),
            (_, verified) => {
                let why = verified.err().unwrap_or_default();
                let output = [text.as_str(), why.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n");
                event(step.id, "failed", &output);
                if step.id != "move" {
                    cleanup();
                }
                return Err(format!("{} failed: {output}", step.label));
            }
        }
    }
    Ok(Installed { server_path: None })
}

/// The staged server must report the app's version and protocol, the host its version.
fn check_versions(text: &str, plan: &Plan, version: &str) -> Result<(), String> {
    let mut lines = text.lines();
    let server = lines.next().unwrap_or("");
    let host = lines.next().unwrap_or("");
    let protocol = format!("(protocol {PROTOCOL_VERSION})");
    if !server.contains(version) || !server.contains(&protocol) {
        return Err(format!(
            "{} reports {server:?}, expected version {version} {protocol}; the old files stay",
            plan.server
        ));
    }
    if !host.contains(version) {
        return Err(format!(
            "{} reports {host:?}, expected version {version}; the old files stay",
            plan.host
        ));
    }
    Ok(())
}

/// The plans waiting for the confirmation, one per machine; a new plan replaces the old.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, Prepared>>);

impl Pending {
    pub fn put(&self, prepared: Prepared) {
        self.lock().insert(prepared.plan.machine.clone(), prepared);
    }

    /// Takes the plan: a confirmed plan runs once.
    pub fn take(&self, machine: &str) -> Option<Prepared> {
        self.lock().remove(machine)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Prepared>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Reads the archive only to hand it to tests that need its length.
#[cfg(test)]
impl Prepared {
    fn archive_len(&self) -> usize {
        self.archive.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn machine() -> MachineConfig {
        MachineConfig {
            name: "box".into(),
            ssh: "me@box".into(),
            repos_dir: "~/Source/{owner}/{repo}".into(),
            server_path: None,
        }
    }

    struct FakeFetch {
        archive: Vec<u8>,
        sums: String,
        urls: Mutex<Vec<String>>,
    }

    impl FakeFetch {
        fn new(archive: &[u8], version: &str, target: &str, hash: Option<String>) -> Self {
            let hash = hash.unwrap_or_else(|| hex(&Sha256::digest(archive)));
            Self {
                archive: archive.to_vec(),
                sums: format!(
                    "{hash}  poimen-server-{version}-{target}.tar.gz\n{} other.dmg\n",
                    "0".repeat(64)
                ),
                urls: Mutex::default(),
            }
        }
    }

    impl Fetch for FakeFetch {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.urls.lock().unwrap().push(url.to_owned());
            if url.ends_with("SHA256SUMS") {
                Ok(self.sums.clone().into_bytes())
            } else {
                Ok(self.archive.clone())
            }
        }
    }

    /// Answers by the first matching marker; records every call.
    struct FakeRemote {
        calls: Mutex<Vec<(String, Option<usize>)>>,
        answers: Vec<(&'static str, Out)>,
    }

    impl FakeRemote {
        fn new(answers: Vec<(&'static str, Out)>) -> Self {
            Self {
                calls: Mutex::default(),
                answers,
            }
        }
        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.0.clone())
                .collect()
        }
    }

    impl Remote for FakeRemote {
        fn run(&self, target: &str, command: &str, input: Option<&[u8]>) -> io::Result<Out> {
            assert_eq!(target, "me@box");
            self.calls
                .lock()
                .unwrap()
                .push((command.to_owned(), input.map(<[u8]>::len)));
            for (marker, out) in &self.answers {
                if command.contains(marker) {
                    return Ok(out.clone());
                }
            }
            Ok(ok(""))
        }
    }

    fn ok(text: &str) -> Out {
        Out {
            ok: true,
            text: text.into(),
        }
    }

    fn fail(text: &str) -> Out {
        Out {
            ok: false,
            text: text.into(),
        }
    }

    const VERSION: &str = "0.18.0";

    fn versions() -> String {
        format!(
            "{} {VERSION} (protocol {PROTOCOL_VERSION})\n{} {VERSION}",
            CHANNEL.named("poimen-server"),
            CHANNEL.named("poimen-pty-host")
        )
    }

    fn prepared(remote: &FakeRemote) -> Prepared {
        let fetch = FakeFetch::new(b"archive", VERSION, "x86_64-unknown-linux-musl", None);
        prepare(remote, &fetch, &machine(), VERSION).unwrap()
    }

    fn probe(uname: &str) -> FakeRemote {
        FakeRemote::new(vec![("uname", ok(uname))])
    }

    #[test]
    fn maps_uname_to_the_release_target_and_refuses_the_rest() {
        assert_eq!(target_of("Linux x86_64"), Ok("x86_64-unknown-linux-musl"));
        assert_eq!(
            target_of("Linux aarch64\n"),
            Ok("aarch64-unknown-linux-musl")
        );
        assert_eq!(target_of("Linux arm64"), Ok("aarch64-unknown-linux-musl"));
        for other in ["Darwin arm64", "Linux riscv64", "", "FreeBSD amd64"] {
            let e = target_of(other).unwrap_err();
            assert!(e.contains("only on Linux"), "{e}");
            assert!(e.contains(other.trim()), "{e}");
        }
    }

    #[test]
    fn a_beta_has_nothing_to_install() {
        assert!(
            unavailable("0.18.0-beta012")
                .unwrap()
                .contains("beta build")
        );
        assert_eq!(unavailable("0.18.0"), None);
        assert_eq!(unavailable("0.18.0-rc003"), None);
    }

    #[test]
    fn reads_the_digest_of_one_archive_from_sha256sums() {
        let h = "a".repeat(64);
        let text = format!(
            "{h}  one.tar.gz\n{} *two.tar.gz\nbad  three.tar.gz\n",
            "B".repeat(64)
        );
        assert_eq!(expected_sum(&text, "one.tar.gz"), Some(h));
        assert_eq!(expected_sum(&text, "two.tar.gz"), Some("b".repeat(64)));
        assert_eq!(expected_sum(&text, "three.tar.gz"), None);
        assert_eq!(expected_sum(&text, "four.tar.gz"), None);
    }

    #[test]
    fn prepare_probes_read_only_downloads_and_plans_the_exact_commands() {
        let remote = probe("Linux x86_64\n/usr/local/bin/poimen-server");
        let fetch = FakeFetch::new(b"archive", VERSION, "x86_64-unknown-linux-musl", None);
        let p = prepare(&remote, &fetch, &machine(), VERSION).unwrap();
        assert_eq!(remote.calls().len(), 1, "only the probe ran on the machine");
        assert!(remote.calls()[0].contains("uname -sm"));
        assert_eq!(p.plan.target, "x86_64-unknown-linux-musl");
        assert_eq!(
            p.plan.archive,
            "poimen-server-0.18.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            p.plan.origin,
            "https://github.com/GravionLabs/poimen/releases/download/v0.18.0/poimen-server-0.18.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            p.plan.existing.as_deref(),
            Some("/usr/local/bin/poimen-server")
        );
        assert_eq!(p.plan.dir, "~/.local/bin");
        let ids: Vec<_> = p.plan.steps.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["mkdir", "copy", "verify", "move", "path"]);
        let tar = &p.plan.steps[1].command;
        assert!(tar.starts_with("tar -xzf - -C "), "{tar}");
        assert_eq!(p.archive_len(), 7);
        assert_eq!(p.plan.sha256, hex(&Sha256::digest(b"archive")));
    }

    #[test]
    fn a_missing_server_is_a_plan_without_an_existing_one() {
        let remote = probe("Linux aarch64");
        let fetch = FakeFetch::new(b"a", VERSION, "aarch64-unknown-linux-musl", None);
        let p = prepare(&remote, &fetch, &machine(), VERSION).unwrap();
        assert_eq!(p.plan.existing, None);
        assert!(
            p.plan
                .archive
                .ends_with("aarch64-unknown-linux-musl.tar.gz")
        );
    }

    #[test]
    fn a_checksum_mismatch_stops_before_any_byte_reaches_the_machine() {
        let remote = probe("Linux x86_64");
        let fetch = FakeFetch::new(
            b"archive",
            VERSION,
            "x86_64-unknown-linux-musl",
            Some("1".repeat(64)),
        );
        let e = prepare(&remote, &fetch, &machine(), VERSION).err().unwrap();
        assert!(e.contains("does not match SHA256SUMS"), "{e}");
        assert!(remote.calls.lock().unwrap().iter().all(|c| c.1.is_none()));
    }

    #[test]
    fn prepare_reports_a_failing_ssh_and_an_unsupported_machine() {
        let remote = FakeRemote::new(vec![("uname", fail("Permission denied (publickey)."))]);
        let fetch = FakeFetch::new(b"a", VERSION, "x86_64-unknown-linux-musl", None);
        let e = prepare(&remote, &fetch, &machine(), VERSION).err().unwrap();
        assert!(e.contains("Permission denied"), "{e}");
        assert!(fetch.urls.lock().unwrap().is_empty(), "nothing downloaded");

        let remote = probe("Darwin arm64");
        let e = prepare(&remote, &fetch, &machine(), VERSION).err().unwrap();
        assert!(e.contains("only on Linux"), "{e}");
        assert!(fetch.urls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_beta_is_refused_before_ssh_runs() {
        let remote = probe("Linux x86_64");
        let fetch = FakeFetch::new(b"a", "0.18.0-beta001", "x86_64-unknown-linux-musl", None);
        assert!(prepare(&remote, &fetch, &machine(), "0.18.0-beta001").is_err());
        assert!(remote.calls().is_empty());
    }

    #[test]
    fn run_executes_every_step_in_order_and_sends_the_archive_only_to_the_copy() {
        let remote = FakeRemote::new(vec![
            ("uname", ok("Linux x86_64")),
            ("--version", ok(&versions())),
            (
                "command -v poimen",
                ok(&format!(
                    "/home/me/.local/bin/{}",
                    CHANNEL.named("poimen-server")
                )),
            ),
        ]);
        let p = prepared(&remote);
        let events = Mutex::new(Vec::new());
        let done = run(&remote, &p, &|e| {
            events.lock().unwrap().push((e.step, e.status));
        })
        .unwrap();
        assert_eq!(done.server_path, None);
        let calls = remote.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1 + 5, "probe plus five steps");
        let with_input: Vec<_> = calls.iter().filter(|c| c.1.is_some()).collect();
        assert_eq!(with_input.len(), 1);
        assert!(with_input[0].0.contains("tar -xzf"));
        assert_eq!(with_input[0].1, Some(7));
        let move_at = calls.iter().position(|c| c.0.contains("mv -f")).unwrap();
        let verify_at = calls
            .iter()
            .position(|c| c.0.contains("--version"))
            .unwrap();
        assert!(
            verify_at < move_at,
            "the staged binaries run before they are moved"
        );
        let events = events.lock().unwrap();
        assert_eq!(events.first(), Some(&("mkdir", "running")));
        assert_eq!(events.last(), Some(&("path", "ok")));
        assert!(events.iter().all(|e| e.1 != "failed"));
    }

    #[test]
    fn a_server_missing_from_the_non_interactive_path_is_reached_by_its_absolute_path() {
        let remote = FakeRemote::new(vec![
            ("uname", ok("Linux x86_64")),
            ("--version", ok(&versions())),
            ("command -v poimen", fail("")),
        ]);
        let p = prepared(&remote);
        let done = run(&remote, &p, &|_| {}).unwrap();
        assert_eq!(
            done.server_path,
            Some(format!(
                "$HOME/.local/bin/{}",
                CHANNEL.named("poimen-server")
            ))
        );
    }

    #[test]
    fn a_failed_step_shows_ssh_output_stops_and_removes_the_staging_directory() {
        let remote = FakeRemote::new(vec![
            ("uname", ok("Linux x86_64")),
            ("tar -xzf", fail("tar: Child returned status 1")),
        ]);
        let p = prepared(&remote);
        let events = Mutex::new(Vec::new());
        let e = run(&remote, &p, &|ev| {
            events
                .lock()
                .unwrap()
                .push((ev.step, ev.status, ev.output.clone()));
        })
        .unwrap_err();
        assert!(e.contains("tar: Child returned status 1"), "{e}");
        let calls = remote.calls();
        assert!(
            !calls.iter().any(|c| c.contains("mv -f")),
            "nothing is moved"
        );
        assert!(
            calls.last().unwrap().contains("rm -rf"),
            "the staging directory goes"
        );
        let events = events.lock().unwrap();
        assert_eq!(events.last().unwrap().0, "copy");
        assert_eq!(events.last().unwrap().1, "failed");
        assert!(events.last().unwrap().2.contains("tar: Child returned"));
    }

    #[test]
    fn staged_binaries_of_another_version_or_protocol_are_never_moved() {
        for text in [
            format!(
                "poimen-server 0.17.0 (protocol {PROTOCOL_VERSION})\npoimen-pty-host {VERSION}"
            ),
            format!(
                "poimen-server {VERSION} (protocol {})\npoimen-pty-host {VERSION}",
                PROTOCOL_VERSION + 1
            ),
            format!("poimen-server {VERSION} (protocol {PROTOCOL_VERSION})\npoimen-pty-host 0.1.0"),
        ] {
            let remote = FakeRemote::new(vec![
                ("uname", ok("Linux x86_64")),
                ("--version", ok(&text)),
            ]);
            let p = prepared(&remote);
            let e = run(&remote, &p, &|_| {}).unwrap_err();
            assert!(e.contains("the old files stay"), "{e}");
            assert!(!remote.calls().iter().any(|c| c.contains("mv -f")));
        }
    }

    #[test]
    fn quote_survives_single_quotes() {
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn a_pending_plan_is_taken_once() {
        let remote = probe("Linux x86_64");
        let pending = Pending::default();
        pending.put(prepared(&remote));
        assert!(pending.take("box").is_some());
        assert!(pending.take("box").is_none());
    }
}
