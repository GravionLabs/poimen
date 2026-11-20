//! `terminal.report_done` (#980): the agent of a terminal reports its result once at its end. The
//! server stores it on the terminal (`Terminal.report`, kept in `server.json`) and tells the agent
//! that started the terminal (`Terminal.started_by`) with one line typed into it. The line names
//! the reporting terminal and the outcome, never the summary (the starter reads that with
//! `wait_agent`/`read_agent`). While the starter works or waits for the user the line waits; the
//! sweeper types it once the starter is idle or done. A starter that is gone or has no agent is
//! not told.

use std::thread;
use std::time::Duration;

use poimen_protocol::report::{AgentReport, TerminalReportDone};
use poimen_protocol::{AgentStatus, TerminalStatus};
use serde_json::json;

use crate::{ApiResult, Shared, invalid, lock, now_millis, terminal_mut};

/// How often a waiting line is tried again.
const SWEEP: Duration = Duration::from_secs(1);

const OUTCOMES: [&str; 3] = ["succeeded", "failed", "cancelled"];
const MAX_SUMMARY: usize = 4000;
const MAX_FILES: usize = 100;
const MAX_FILE: usize = 500;

pub(crate) fn terminal_report_done(shared: &Shared, p: TerminalReportDone) -> ApiResult {
    if !OUTCOMES.contains(&p.outcome.as_str()) {
        return Err(invalid("outcome must be succeeded, failed or cancelled"));
    }
    let summary = p.summary.trim();
    if summary.is_empty() || summary.chars().count() > MAX_SUMMARY {
        return Err(invalid(format!(
            "summary must be 1 to {MAX_SUMMARY} characters"
        )));
    }
    if p.files.len() > MAX_FILES || p.files.iter().any(|f| f.chars().count() > MAX_FILE) {
        return Err(invalid(format!(
            "at most {MAX_FILES} files of at most {MAX_FILE} characters"
        )));
    }
    let report = {
        let mut inner = lock(&shared.inner);
        let entry = terminal_mut(&mut inner, &p.terminal_id)?;
        if entry.info.status == TerminalStatus::Exited {
            return Err(invalid("the terminal has ended"));
        }
        let report = AgentReport {
            outcome: p.outcome,
            summary: summary.to_owned(),
            files: p.files,
            at: now_millis(),
            // Nobody started this terminal: nobody to tell.
            notified: entry.info.started_by.is_none(),
        };
        entry.info.report = Some(report.clone());
        shared.persist(&inner);
        report
    };
    shared.deliver_reports();
    Ok(json!(report))
}

/// One line without control characters, so a terminal name cannot type a second command.
fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

impl Shared {
    /// Types the line of every report whose starter is idle or done; marks the reports of a
    /// starter that cannot be told. Called after a report and by the sweeper.
    pub(crate) fn deliver_reports(&self) {
        let mut typing = Vec::new();
        let mut changed = false;
        {
            let mut inner = lock(&self.inner);
            let waiting: Vec<(usize, String, String)> = inner
                .terminals
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let report = t.info.report.as_ref().filter(|r| !r.notified)?;
                    let starter = t.info.started_by.clone()?;
                    let line = format!(
                        "poimen: terminal {} ({}) reported {}. Read it with wait_agent or read_agent.",
                        t.info.id,
                        plain(&t.info.name),
                        report.outcome
                    );
                    Some((i, starter, line))
                })
                .collect();
            for (i, starter, line) in waiting {
                let target = inner.terminals.iter().find(|t| t.info.id == starter);
                let tell = match target {
                    None => None,
                    Some(t) if t.info.status == TerminalStatus::Exited => None,
                    Some(t) => match t.info.agent.as_ref().map(|a| a.status) {
                        None | Some(AgentStatus::Gone) => None,
                        Some(AgentStatus::Idle | AgentStatus::Done) => {
                            Some(t.host.as_ref().map(|h| h.writer()))
                        }
                        // Working, blocked or in error: the line waits.
                        Some(_) => continue,
                    },
                };
                if let Some(report) = inner.terminals[i].info.report.as_mut() {
                    report.notified = true;
                    changed = true;
                }
                if let Some(Some(writer)) = tell {
                    typing.push((writer, format!("{line}\r")));
                }
            }
            if !changed {
                return;
            }
            self.persist(&inner);
        }
        for (writer, text) in typing {
            if let Err(e) = writer.write(text.as_bytes()) {
                self.log("report", &format!("the report line was not typed: {e}"));
            }
        }
    }
}

/// Runs [`Shared::deliver_reports`] on a timer for the life of the server.
pub(crate) fn spawn_sweeper(shared: &std::sync::Arc<Shared>) {
    let shared = std::sync::Arc::clone(shared);
    thread::spawn(move || {
        loop {
            thread::sleep(SWEEP);
            shared.deliver_reports();
        }
    });
}
