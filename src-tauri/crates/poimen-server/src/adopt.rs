//! Adopting the hosts of a terminal list a previous server wrote.

use super::*;

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

/// The entry of an adopted terminal: the stored state over a grid fed from the host's ring.
fn adopted(
    shared: &Arc<Shared>,
    t: store::StoredTerminal,
    reached: host::Reached,
) -> TerminalEntry {
    let (cols, rows) = (reached.size.cols.max(1), reached.size.rows.max(1));
    let stream = new_stream(shared, &t.id, cols, rows);
    let handle = host::adopt(
        reached,
        &log::path(&shared.state_file),
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
        },
        host: Some(handle),
        stream,
        agent_seq: t.agent_seq,
        agent_persisted: None,
        osc: t.osc.then(|| t.agent.clone()).flatten(),
        hook_agent: t.hook_agent.clone(),
        records,
        tokens,
        resuming: false,
    }
}
