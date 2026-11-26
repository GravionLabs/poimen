//! A new shell for an ended terminal whose agent can be resumed (#1232, ADR 0014).
//!
//! A terminal whose host is gone (a reboot) is its history, read-only (#1195). When its last
//! agent session is known and its harness has `resumeArgs`, adoption marks it `hibernated`; the
//! first control attach then starts a host under the same terminal id (same leaf, same log) with
//! the program and arguments it had, in the cwd the agent ran in. The resume command is typed by
//! `hibernate.rs::resume_hibernated`, as for #904. An observer starts nothing.

use super::*;

impl TerminalEntry {
    /// Keeps the latest agent session a report names: it survives `/clear` and `gone`.
    pub(crate) fn note_session(&mut self, p: &TerminalReportAgent) {
        let (Some(agent), Some(session)) = (
            p.agent.as_deref().filter(|a| !a.is_empty()),
            p.session_id.as_deref().filter(|s| !s.is_empty()),
        ) else {
            return;
        };
        let last = store::LastAgent {
            agent: agent.to_owned(),
            session_id: session.to_owned(),
            cwd: self.info.cwd.clone(),
        };
        if p.status != AgentStatus::Gone && self.last_agent.as_ref() != Some(&last) {
            self.last_agent = Some(last);
            // Written even when the report changes nothing else.
            self.agent_persisted = None;
        }
    }
}

impl Shared {
    /// For a control attach: starts a shell for `terminal_id` when it is an ended terminal with a session to resume;
    /// does nothing for any other terminal. A failed start publishes `terminal.resume_failed`
    /// and leaves the terminal as it was.
    pub(crate) fn revive_ended(self: &Arc<Self>, terminal_id: &str, mode: AttachMode) {
        if mode != AttachMode::Control {
            return;
        }
        let mut inner = lock(&self.inner);
        let Some(index) = inner.terminals.iter().position(|t| {
            t.info.id == terminal_id
                && t.info.from_history
                && t.host.is_none()
                && t.info.hibernated.is_some()
        }) else {
            return;
        };
        let (space_id, program, args, cols, rows, own_cwd) = {
            let info = &inner.terminals[index].info;
            (
                info.space_id.clone(),
                info.program.clone(),
                info.args.clone(),
                info.cols,
                info.rows,
                info.cwd.clone(),
            )
        };
        // The agent's cwd while it still exists, else the terminal's own.
        let cwd = inner.terminals[index]
            .last_agent
            .as_ref()
            .map(|a| a.cwd.clone())
            .filter(|c| Path::new(c).is_dir())
            .unwrap_or(own_cwd);
        let mut env = std::collections::BTreeMap::new();
        if let Ok(space) = inner.layout.space(&space_id) {
            let dir = if space.cwd.is_empty() {
                &cwd
            } else {
                &space.cwd
            };
            env.extend(poimen_core::project::dir_env(dir, &space.name));
        }
        let mut command = vec![program.unwrap_or_else(pty::default_shell)];
        command.extend(args);
        let term = std::env::var("TERM").ok();
        let entry = &mut inner.terminals[index];
        // Before the host starts: its first output must find the stream revived.
        entry.stream.revive();
        let spawned = host::start(
            &self.pty_host,
            &host::Spec {
                terminal_id,
                log: &log::path(&self.state_file),
                socket: &self.socket,
                command: &command,
                cwd: &cwd,
                cols,
                rows,
                term: term.as_deref(),
                locale_set: host::locale_set(|name| std::env::var(name).ok()),
                env: &env,
            },
            output_to(&entry.stream),
            exit_to(self, terminal_id),
        );
        let spawned = match spawned {
            Ok(spawned) => spawned,
            Err(e) => {
                entry.stream.unrevive();
                // Not retried on every attach: the terminal stays read-only history.
                entry.info.hibernated = None;
                self.publish(Event::TerminalHibernatedChanged {
                    terminal_id: terminal_id.to_owned(),
                    hibernated: None,
                });
                self.persist(&inner);
                self.publish(Event::TerminalResumeFailed {
                    terminal_id: terminal_id.to_owned(),
                    reason: format!("no new shell could be started: {e}"),
                });
                return;
            }
        };
        entry.stream.set_writer(spawned.writer());
        entry.info.status = TerminalStatus::Running;
        entry.info.exit_code = None;
        entry.info.from_history = false;
        entry.info.survives_restart = true;
        entry.info.cwd = cwd;
        entry.info.host_version = Some(spawned.version().to_string());
        entry.info.host_bundled_version = Some(host_bundled_version());
        // The old agent's state is not the new shell's: the resume waits for `gone`.
        entry.info.agent = None;
        entry.hook_agent = None;
        entry.osc = None;
        entry.host = Some(spawned);
        entry.ended_endpoint = None;
        let hibernated = entry.info.hibernated.clone();
        self.log(
            "server",
            &format!("terminal {terminal_id}: a new shell started to resume its agent session"),
        );
        self.publish(Event::TerminalHibernatedChanged {
            terminal_id: terminal_id.to_owned(),
            hibernated,
        });
        self.persist(&inner);
    }
}

impl Shared {
    pub(crate) fn terminal_exited(&self, terminal_id: &str, exit_code: Option<u32>) {
        let mut inner = lock(&self.inner);
        let Some(entry) = inner
            .terminals
            .iter_mut()
            .find(|t| t.info.id == terminal_id)
        else {
            return;
        };
        if entry.info.status == TerminalStatus::Exited {
            return;
        }
        entry.info.status = TerminalStatus::Exited;
        entry.info.exit_code = exit_code;
        self.log(
            "server",
            &format!(
                "terminal {terminal_id} ended, exit code {}",
                exit_code.map_or("unknown".to_string(), |c| c.to_string())
            ),
        );
        // Nothing of it is left for a restart to keep.
        entry.info.survives_restart = false;
        entry.info.host_version = None;
        entry.info.host_bundled_version = None;
        entry.host = None;
        // A program's own `idle` or `done` stays; the rest of the status goes with the process.
        entry.hook_agent = None;
        entry.info.agent = entry
            .osc
            .take()
            .filter(|a| matches!(a.status, AgentStatus::Idle | AgentStatus::Done));
        if !entry.info.tokens.is_empty() {
            entry.info.tokens.clear();
            entry.tokens.clear();
            self.publish(Event::TerminalTokensChanged {
                terminal_id: terminal_id.to_string(),
                tokens: Default::default(),
            });
        }
        entry.records.exited();
        if entry.sync_records() {
            self.publish(records_event(entry));
        }
        entry.stream.exited(exit_code);
        self.publish(Event::TerminalExited {
            terminal_id: terminal_id.to_string(),
            exit_code,
        });
        // An exited terminal leaves the tree and is not kept for a restart. The one of a
        // scheduled run (#1305) stays in its tab until it is closed, so its output can be read.
        let now = self.clock_ms();
        if !inner.schedules.ended(terminal_id, exit_code, now) {
            let events = inner.layout.remove_terminal(terminal_id);
            publish_all(self, events);
        }
        self.persist(&inner);
    }
}
