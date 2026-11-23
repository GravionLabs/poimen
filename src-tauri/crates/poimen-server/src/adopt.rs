//! Adopting the hosts of a terminal list a previous server wrote.

use super::*;

pub(crate) mod rebuild;

/// Adopts the hosts of the terminal list a previous server wrote (ADR 0009, "Server update"):
/// connects to every one at once, then rebuilds each grid from its ring and restores name, size,
/// agent status and tokens. A terminal keeps its id, so tabs and splits still point to it. A host
/// that does not answer (or a terminal whose space is gone) is dropped with a line on stderr.
pub(super) fn adopt_terminals(
    shared: &Arc<Shared>,
    inner: &mut Inner,
    stored: Vec<store::StoredTerminal>,
) {
    let reached: Vec<_> = thread::scope(|scope| {
        let pending: Vec<_> = stored
            .into_iter()
            .map(|t| {
                let (endpoint, seq) = (t.endpoint.clone(), t.seq);
                let reach = scope.spawn(move || host::reach(&endpoint, seq, host::ADOPT_TIMEOUT));
                (t, reach)
            })
            .collect();
        pending
            .into_iter()
            .map(|(t, reach)| {
                let reached = reach
                    .join()
                    .unwrap_or_else(|_| Err("the connect failed".into()));
                (t, reached)
            })
            .collect()
    });
    for (t, reached) in reached {
        let dropped = |why: &str| {
            shared.log(
                "server",
                &format!(
                    "dropping terminal {} ({}): {why}; host log kept in {}",
                    t.id,
                    t.endpoint.display(),
                    log::host_path(&t.endpoint).display()
                ),
            );
        };
        if inner.terminals.iter().any(|e| e.info.id == t.id) {
            dropped("listed twice");
            continue;
        }
        let reached = match reached {
            Ok(reached) => reached,
            Err(e) => {
                // The host is gone (a reboot): what the log holds stays to look at (#1195).
                if inner.layout.space(&t.space_id).is_ok()
                    && let Some(ended) = ended(shared, &t)
                {
                    shared.log(
                        "server",
                        &format!(
                            "terminal {} ({}): its host does not answer ({e}); kept as an ended terminal with its history",
                            t.id,
                            t.endpoint.display()
                        ),
                    );
                    inner.terminals.push(ended);
                    continue;
                }
                dropped(&format!("its host does not answer: {e}"));
                continue;
            }
        };
        if inner.layout.space(&t.space_id).is_err() {
            dropped(&format!("its space {} is gone", t.space_id));
            reached.kill();
            continue;
        }
        shared.log(
            "server",
            &format!(
                "adopted terminal {} ({}), host version {}",
                t.id,
                t.endpoint.display(),
                reached.version()
            ),
        );
        inner.terminals.push(adopted(shared, t, reached));
    }
}

/// The entry of a terminal whose host is gone but whose history log holds something (#1195):
/// status `exited`, no process, the grid rebuilt from the log and read-only to every client.
/// `None` without a usable log, then the terminal is dropped.
fn ended(shared: &Arc<Shared>, t: &store::StoredTerminal) -> Option<TerminalEntry> {
    let (cols, rows) = (t.cols.max(1), t.rows.max(1));
    let rebuilt = rebuild::rebuild(&shared.history.path(&t.id), (cols, rows), shared.scrollback)?;
    let stream = new_stream(shared, &t.id, cols, rows);
    let (cols, rows) = rebuilt.emulator.size();
    stream.ended_from_history(rebuilt.emulator);
    // The process is gone; only a program's own `idle` or `done` stays, as after an exit.
    let agent = t
        .agent
        .clone()
        .filter(|a| matches!(a.status, AgentStatus::Idle | AgentStatus::Done));
    let hibernated = t.hibernated.clone().or_else(|| resumable(shared, t));
    Some(TerminalEntry {
        info: Terminal {
            id: t.id.clone(),
            space_id: t.space_id.clone(),
            name: t.name.clone(),
            cwd: t.cwd.clone(),
            cols,
            rows,
            status: TerminalStatus::Exited,
            exit_code: None,
            program: t.program.clone(),
            args: t.args.clone(),
            agent,
            tokens: Default::default(),
            survives_restart: false,
            host_version: None,
            host_bundled_version: None,
            records: Vec::new(),
            hibernated,
            started_by: t.started_by.clone(),
            report: t.report.clone(),
            agent_source: None,
            memory_bytes: None,
            status_history: Vec::new(),
            from_history: true,
        },
        host: None,
        ended_endpoint: Some(t.endpoint.clone()),
        stream,
        agent_seq: t.agent_seq,
        agent_persisted: None,
        osc: None,
        hook_agent: None,
        records: records::Records::default(),
        tokens: Vec::new(),
        resuming: false,
        last_input_ms: None,
        last_agent: t.last_agent.clone(),
    })
}

/// The agent session an ended terminal can resume (#1232): the last one the terminal had (or
/// the stored status's), when its harness has `resumeArgs`. `None` keeps the terminal read-only.
fn resumable(shared: &Shared, t: &store::StoredTerminal) -> Option<poimen_protocol::Hibernated> {
    let (agent, session_id) = match (&t.last_agent, &t.agent) {
        (Some(last), _) => (last.agent.clone(), last.session_id.clone()),
        (None, Some(a)) => (a.agent.clone()?, a.session_id.clone()?),
        (None, None) => return None,
    };
    let harnesses = hibernate::harnesses_of(shared);
    let h = harnesses.iter().find(|h| h.id == agent)?;
    if h.resume_args.is_empty() || session_id.is_empty() {
        return None;
    }
    Some(poimen_protocol::Hibernated {
        agent,
        session_id,
        since: now_millis(),
    })
}

/// The entry of an adopted terminal: the stored state over a grid fed from the host's ring.
fn adopted(
    shared: &Arc<Shared>,
    t: store::StoredTerminal,
    reached: host::Reached,
) -> TerminalEntry {
    let (cols, rows) = (reached.size.cols.max(1), reached.size.rows.max(1));
    let stream = new_stream(shared, &t.id, cols, rows);
    // The grid comes from the history log when it reaches the host's ring (#1194); then the
    // ring is read only after what the log covers, and a `gap` is marked only for what neither
    // holds. Without a usable log the grid is built from the ring as before and a new log starts.
    let read_from = match rebuilt(shared, &t.id, (cols, rows), reached.seq) {
        Some(rebuilt) => {
            let seq = rebuilt.seq;
            stream.restore(rebuilt.emulator, seq);
            seq
        }
        None => {
            stream.start_log();
            0
        }
    };
    let handle = host::adopt(
        reached,
        &log::path(&shared.state_file),
        read_from,
        output_to(&stream),
        exit_to(shared, &t.id),
    );
    stream.set_writer(handle.writer());
    let (now, now_ms) = (Instant::now(), now_millis());
    let records = records::Records::from(t.records.clone());
    let view = records.view(t.agent.as_ref());
    let mut visible = std::collections::BTreeMap::new();
    let mut tokens = Vec::new();
    for token in t.tokens {
        let expires = match token.expires_at_ms {
            None => None,
            Some(at) if at > now_ms => Some(now + Duration::from_millis(at - now_ms)),
            // Its time ran out while no server was there.
            Some(_) => continue,
        };
        visible.insert(token.name.clone(), token.value);
        tokens.push(TokenEntry {
            name: token.name,
            source: token.source,
            expires,
        });
    }
    TerminalEntry {
        info: Terminal {
            id: t.id,
            space_id: t.space_id,
            name: t.name,
            cwd: t.cwd,
            cols,
            rows,
            status: TerminalStatus::Running,
            exit_code: None,
            program: t.program,
            args: t.args,
            agent: t.agent.clone(),
            tokens: visible,
            survives_restart: true,
            host_version: Some(handle.version().to_string()),
            host_bundled_version: Some(host_bundled_version()),
            records: view,
            hibernated: t.hibernated,
            started_by: t.started_by,
            report: t.report,
            agent_source: t.agent.as_ref().map(|a| {
                if t.osc {
                    "osc7501".to_string()
                } else {
                    a.agent.clone().unwrap_or_default()
                }
            }),
            memory_bytes: None,
            status_history: Vec::new(),
            from_history: false,
        },
        host: Some(handle),
        ended_endpoint: None,
        stream,
        agent_seq: t.agent_seq,
        agent_persisted: None,
        osc: t.osc.then(|| t.agent.clone()).flatten(),
        hook_agent: t.hook_agent.clone(),
        records,
        tokens,
        resuming: false,
        last_input_ms: None,
        last_agent: t.last_agent,
    }
}

/// The grid of terminal `id` rebuilt from its history log, `None` when there is no log, it holds
/// nothing, or it does not belong to this host (it covers records the host never had).
fn rebuilt(shared: &Shared, id: &str, size: (u16, u16), host_seq: u64) -> Option<rebuild::Rebuilt> {
    let path = shared.history.path(id);
    let rebuilt = rebuild::rebuild(&path, size, shared.scrollback)?;
    if rebuilt.seq == 0 || rebuilt.seq > host_seq {
        return None;
    }
    shared.log(
        "server",
        &format!(
            "terminal {id}: grid rebuilt from its history log up to host record {}",
            rebuilt.seq
        ),
    );
    Some(rebuilt)
}

/// The screen of a new terminal, with a new history log.
pub(crate) fn fresh_stream(
    shared: &Arc<Shared>,
    id: &str,
    cols: u16,
    rows: u16,
) -> Arc<TermStream> {
    let stream = new_stream(shared, id, cols, rows);
    stream.start_log();
    stream
}

/// The screen and attachments of a new or adopted terminal.
pub(crate) fn new_stream(shared: &Arc<Shared>, id: &str, cols: u16, rows: u16) -> Arc<TermStream> {
    let stream = Arc::new(TermStream::new(
        id,
        cols,
        rows,
        shared.scrollback,
        Arc::clone(&shared.colors),
        {
            let weak = Arc::downgrade(shared);
            Arc::new(move |event| {
                if let Some(shared) = weak.upgrade() {
                    shared.publish(event);
                }
            })
        },
    ));
    stream.set_history(Arc::clone(&shared.history));
    let (weak, id) = (Arc::downgrade(shared), id.to_string());
    stream.set_signal_sink(Arc::new(move |signals| {
        if let Some(shared) = weak.upgrade() {
            shared.output_signals(&id, signals);
        }
    }));
    stream
}

/// Where a host's output goes: the grid and the clients; a replay answers no query.
pub(crate) fn output_to(stream: &Arc<TermStream>) -> impl FnMut(&[u8], bool, u64) + Send + 'static {
    let stream = Arc::clone(stream);
    move |bytes, live, seq| {
        if live {
            stream.output_at(bytes, seq);
        } else {
            stream.replay_at(bytes, seq);
        }
    }
}

/// The host's reader reports the exit through the shared state.
pub(crate) fn exit_to(shared: &Arc<Shared>, id: &str) -> impl FnOnce(Option<u32>) + Send + 'static {
    let id = id.to_string();
    let shared = Arc::clone(shared);
    move |code| shared.terminal_exited(&id, code)
}
