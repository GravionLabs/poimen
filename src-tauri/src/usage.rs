//! Claude (and Copilot) usage for the status bar, read from tamias (`tamias today --json`,
//! parsed with `tamias-core`). poimen has no usage source of its own. The report is passed on as
//! it is (snake_case wire of tamias); the frontend derives the figures (`core/usage.ts`).

use std::io::{self, Read};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use tamias_core::report::Report;

use crate::gui_path;

/// How often the report is read while the app runs.
pub const POLL_INTERVAL: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(15);
/// Exit code of `tamias today` when a provider is stale; the output is still complete.
const EXIT_STALE: i32 = 3;

/// Runs `tamias today --json`; faked in tests.
pub trait Runner {
    /// `ErrorKind::NotFound` means tamias is not installed.
    fn today(&self) -> io::Result<Output>;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn today(&self) -> io::Result<Output> {
        let path = gui_path::child_path();
        let program = gui_path::resolve_program("tamias", &path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "tamias not found"))?;
        let mut child = gui_path::command(program)
            .args(["today", "--json"])
            .env("PATH", &path)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "tamias timed out"));
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        Ok(Output {
            status,
            stdout: reader.join().unwrap_or_default(),
            stderr: Vec::new(),
        })
    }
}

/// Whether a report has something to show: at least one provider with a window.
fn has_data(report: &Report) -> bool {
    report.providers.iter().any(|p| !p.windows.is_empty())
}

/// The current report, or `None` (hidden item) without tamias, with a failing run, an unknown
/// document or only unavailable providers. Never an error: usage is optional.
pub fn read<R: Runner>(runner: &R) -> Option<Report> {
    let output = runner.today().ok()?;
    // 0 ok, 3 stale (output complete); 1 and 2 print nothing.
    match output.status.code() {
        Some(0) | Some(EXIT_STALE) => {}
        _ => return None,
    }
    let report = Report::parse(std::str::from_utf8(&output.stdout).ok()?).ok()?;
    has_data(&report).then_some(report)
}

/// Polls every `interval` and calls `emit` with the report whenever it changed (also the first
/// read, and a change to `None`).
pub fn watch(interval: Duration, emit: impl Fn(&Option<Report>)) {
    let mut last: Option<Option<Report>> = None;
    loop {
        let now = read(&SystemRunner);
        if last.as_ref() != Some(&now) {
            emit(&now);
            last = Some(now);
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tamias_core::report::Provider as ProviderReport;
    use tamias_core::snapshot::{Provider, Window};

    struct Fake(io::Result<Output>);

    impl Runner for Fake {
        fn today(&self) -> io::Result<Output> {
            match &self.0 {
                Ok(o) => Ok(o.clone()),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
    }

    #[cfg(unix)]
    fn status(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }
    #[cfg(windows)]
    fn status(code: i32) -> std::process::ExitStatus {
        std::os::windows::process::ExitStatusExt::from_raw(code as u32)
    }

    fn run(code: i32, stdout: &str) -> Fake {
        Fake(Ok(Output {
            status: status(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }))
    }

    fn provider(report: &Report, which: Provider) -> &ProviderReport {
        report
            .providers
            .iter()
            .find(|p| p.provider == which)
            .unwrap()
    }

    #[test]
    fn reads_the_claude_and_copilot_windows() {
        let report = read(&run(0, include_str!("../fixtures/tamias-today.json"))).unwrap();
        let claude = provider(&report, Provider::Claude);
        let seven = claude
            .windows
            .iter()
            .find(|w| w.window == Window::SevenDay)
            .unwrap();
        assert_eq!(seven.used, 54.0);
        assert_eq!(seven.limit, 100.0);
        assert_eq!(seven.today_usage, Some(4.0));
        assert_eq!(seven.budget, Some(10.0));
        assert_eq!(seven.ratio, Some(0.4));
        assert_eq!(seven.month_usage, Some(4.0));
        let five = claude
            .windows
            .iter()
            .find(|w| w.window == Window::FiveHour)
            .unwrap();
        assert_eq!(five.used, 12.0);
        assert_eq!(five.month_usage, None);
        let copilot = provider(&report, Provider::Copilot);
        assert_eq!(copilot.windows[0].window, Window::Monthly);
        assert_eq!(copilot.windows[0].limit, 300.0);
    }

    #[test]
    fn a_stale_exit_code_is_still_parsed() {
        let report = read(&run(3, include_str!("../fixtures/tamias-today-stale.json"))).unwrap();
        let claude = provider(&report, Provider::Claude);
        assert!(claude.stale_since.is_some());
        assert_eq!(claude.windows.len(), 1);
    }

    #[test]
    fn a_missing_month_usage_is_absent() {
        let report = read(&run(
            0,
            include_str!("../fixtures/tamias-today-no-month.json"),
        ))
        .unwrap();
        let seven = &provider(&report, Provider::Claude).windows[0];
        assert_eq!(seven.month_usage, None);
        assert_eq!(seven.today_usage, Some(4.0));
    }

    #[test]
    fn unavailable_providers_hide_the_item() {
        let json = include_str!("../fixtures/tamias-today-unavailable.json");
        assert!(read(&run(0, json)).is_none());
    }

    #[test]
    fn a_missing_tamias_is_none() {
        let fake = Fake(Err(io::Error::new(io::ErrorKind::NotFound, "no tamias")));
        assert!(read(&fake).is_none());
    }

    #[test]
    fn failures_and_unknown_documents_are_none() {
        let json = include_str!("../fixtures/tamias-today.json");
        assert!(read(&run(1, "")).is_none());
        assert!(read(&run(2, json)).is_none());
        assert!(read(&run(0, "not json")).is_none());
        let v2 = json.replace("\"schema_version\": 1", "\"schema_version\": 2");
        assert!(read(&run(0, &v2)).is_none());
    }

    #[test]
    fn the_report_serializes_with_tamias_field_names() {
        let report = read(&run(0, include_str!("../fixtures/tamias-today.json"))).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["providers"][0]["provider"], "claude");
        assert_eq!(value["providers"][0]["windows"][0]["window"], "seven_day");
        assert_eq!(value["providers"][0]["windows"][0]["month_usage"], 4.0);
    }
}
