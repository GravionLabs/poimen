//! `record_run` and `read_run` (#1212, ADR 0018): the state of a lead run lives in an
//! agent-events run `lead-<uuid>` of the repository of the lead's space (in the user's shared
//! event directory, `origin` names the repository), not in the session. The lead writes the plan, its members, the grant and its decisions; a new lead session reads them
//! back and resumes. Core events (`run.*`, `task.*`, `attention*`, `progress`) show the run in the
//! Runs view; the records of the profile `orchestrate@1` (`poimen_protocol::lead`) carry the rest.

use std::collections::BTreeSet;
use std::path::PathBuf;

use poimen_client::Client;
use poimen_core::agent_events::{self, RepoRef};
use poimen_protocol::files::{now_secs, utc};
use poimen_protocol::lead::{LEAD_PROFILE, LEAD_RUN_PREFIX, LeadRecord};
use serde_json::{Value, json};

use super::{Env, Fail, run_files, run_writer, runs, server_error, text_arg};

const PLAN_MAX_BYTES: usize = 64 * 1024;
const OUTCOMES: [&str; 4] = ["succeeded", "failed", "cancelled", "skipped"];
const KEYS: [&str; 16] = [
    "event",
    "repository",
    "target",
    "run",
    "title",
    "plan",
    "task",
    "blocked_by",
    "space",
    "terminal",
    "machine",
    "deliver_run",
    "text",
    "outcome",
    "id",
    "grant",
];

/// The tool definition of `record_run`.
pub(super) fn record_tool() -> Value {
    json!({
        "name": "record_run",
        "description": "Records the state of YOUR lead run outside your session, so a new lead session can resume it (read_run). It appends to an agent-events run `lead-<uuid>` of the repository of your space, which the Runs view shows. Write after every step: `start` (title = the user's request; optional `plan`) once, then `plan` (the approved plan: repositories with path, milestone, order, blockers; the newest replaces older ones), `member` (a started loop: `task` `<repo>#<n>`, `target` = its repository directory, `space`, `terminal`, `deliver_run`, `machine`), `member_done` (`task`, `outcome`, `text`), `dependency` (`task`, `blocked_by`), `grant` (what request_grant gave: `grant` id, `spaces`, `tools`, `max_agents`), `decision` (what you answered to a blocked session: `text`, `task`), `attention` / `resolved` (a blocked session: `id`, `task`, `terminal`, `text`), `note` (progress, `text`; write one at least every 20 minutes while you wait so the run is not shown abandoned), `resume` (after the user confirmed taking over a run from another terminal; `run`) and `finish` (`outcome`, `text`). Never record a secret. A write to your own run file: no confirmation.",
        "inputSchema": {"type": "object", "properties": {
            "event": {"type": "string", "enum": ["start", "plan", "member", "member_done", "dependency", "grant", "decision", "attention", "resolved", "note", "resume", "finish"]},
            "repository": {"type": "string", "description": "A directory in the repository that holds the run; default: the directory of your terminal."},
            "run": {"type": "string", "description": "The lead run id; default: your open run."},
            "title": {"type": "string"},
            "plan": {"description": "The plan as structured data."},
            "target": {"type": "string", "description": "`member`: the directory of the repository the loop works in."},
            "task": {"type": "string"},
            "blocked_by": {"type": "string"},
            "space": {"type": "string"},
            "terminal": {"type": "string"},
            "machine": {"type": "string"},
            "deliver_run": {"type": "string"},
            "text": {"type": "string"},
            "outcome": {"type": "string", "enum": ["succeeded", "failed", "cancelled", "skipped"]},
            "id": {"type": "string", "description": "The attention id."},
            "grant": {"type": "object", "description": "{id, spaces[], tools[], max_agents, expires?}"}
        }, "required": ["event"], "additionalProperties": false},
    })
}

/// The tool definition of `read_run`.
pub(super) fn read_tool() -> Value {
    json!({
        "name": "read_run",
        "description": "Reads the recorded state of a lead run (record_run): the run as the Runs view folds it (open tasks are the members still to watch, attentions), the newest plan, the members, dependencies, grant and decisions, and the lead terminal. Without `run` it reads your own open run; when you have none it lists the open lead runs of the repository so you can show them to the user and resume one. The record may lag: re-check terminals with list_terminals and get_events of each member before you act. A read tool: no confirmation.",
        "inputSchema": {"type": "object", "properties": {
            "repository": {"type": "string", "description": "A directory in the repository; default: the directory of your terminal."},
            "run": {"type": "string", "description": "A lead run id."}
        }, "additionalProperties": false},
    })
}

/// What the lines of a lead run file say about its writer.
#[derive(Default)]
struct Scan {
    last_seq: i64,
    terminal: Option<String>,
    finished: bool,
    tasks: BTreeSet<String>,
    records: Vec<LeadRecord>,
    title: Option<String>,
    /// `ts` of `run.started`.
    started: String,
}

fn scan(events: &[Value]) -> Scan {
    let mut s = Scan::default();
    for e in events {
        s.last_seq = s.last_seq.max(e["seq"].as_i64().unwrap_or(0));
        match e["type"].as_str() {
            Some("run.started") => {
                s.started = e["ts"].as_str().unwrap_or_default().to_owned();
                s.terminal = e["host"]["poimen.terminal"].as_str().map(str::to_owned);
                s.title = e["subject"]["title"].as_str().map(str::to_owned);
            }
            Some("run.finished") => s.finished = true,
            Some("task.started") => {
                if let Some(t) = e["task"].as_str() {
                    s.tasks.insert(t.to_owned());
                }
            }
            _ => {}
        }
        if let Some(r) = LeadRecord::from_line(&e.to_string()) {
            if let LeadRecord::Resume { terminal } = &r {
                s.terminal = Some(terminal.clone());
            }
            s.records.push(r);
        }
    }
    s
}

/// One lead run of the repository: its merged events and what they say.
struct Lead {
    run: String,
    events: Vec<Value>,
    scan: Scan,
}

/// The lead runs of the repository, newest first.
fn lead_runs(at: &Where) -> Vec<Lead> {
    let mut attribution = run_files::Attribution::default();
    let mut v: Vec<Lead> =
        run_files::of_repository(at.shared.as_deref(), &at.repo, &mut attribution)
            .into_iter()
            .filter(|(run, _)| run.starts_with(LEAD_RUN_PREFIX))
            .map(|(run, files)| {
                let events = run_files::events(&files);
                let scan = scan(&events);
                Lead { run, events, scan }
            })
            .collect();
    v.sort_by(|a, b| (&b.scan.started, &b.run).cmp(&(&a.scan.started, &a.run)));
    v
}

struct Where {
    shared: Option<PathBuf>,
    repo: RepoRef,
    repository: String,
}

fn terminal_dir(client: &Client, id: &str) -> Result<String, Fail> {
    client
        .list_terminals(None)
        .map_err(server_error)?
        .into_iter()
        .find(|t| t.id == id)
        .map(|t| t.cwd)
        .ok_or_else(|| Fail::Tool(format!("terminal {id} is not known to the server")))
}

fn locate(client: &Client, env: &Env, args: &Value) -> Result<Where, Fail> {
    let repository = match text_arg(args, "repository") {
        Some(r) => r.to_owned(),
        None => terminal_dir(client, caller(env)?)?,
    };
    let repo = run_files::repository(&repository).map_err(Fail::Tool)?;
    Ok(Where {
        shared: env.events_dir.clone(),
        repo,
        repository,
    })
}

fn caller<'a>(env: &Env<'a>) -> Result<&'a str, Fail> {
    env.terminal_id.filter(|i| !i.is_empty()).ok_or_else(|| {
        Fail::Tool(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set)"
                .into(),
        )
    })
}

fn need<'a>(args: &'a Value, key: &str, event: &str) -> Result<&'a str, Fail> {
    text_arg(args, key).ok_or_else(|| Fail::Tool(format!("`{event}` needs the argument `{key}`")))
}

fn opt(args: &Value, key: &str) -> Option<String> {
    text_arg(args, key).map(str::to_owned)
}

fn valid_run(run: &str) -> bool {
    run.starts_with(LEAD_RUN_PREFIX) && !run.contains(['/', '\\', '.'])
}

/// `record_run {event, ...}`.
pub(super) fn record_run(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &KEYS)?;
    let me = caller(env)?;
    let event = need(args, "event", "record_run")?;
    let at = locate(client, env, args)?;
    // A write goes only to the repository of the caller's own terminal (no confirmation needed).
    let own_dir = terminal_dir(client, me)?;
    let own_repo = run_files::repository(&own_dir).map_err(Fail::Tool)?;
    if own_repo.common != at.repo.common {
        return Err(Fail::Tool(
            "refused: a lead run is recorded in the repository of your own terminal".into(),
        ));
    }
    let shared = at
        .shared
        .clone()
        .ok_or_else(|| Fail::Tool(run_files::NO_SHARED.into()))?;
    if args
        .get("plan")
        .is_some_and(|p| p.to_string().len() > PLAN_MAX_BYTES)
    {
        return Err(Fail::Tool(format!("`plan` is over {PLAN_MAX_BYTES} bytes")));
    }
    let mut known = lead_runs(&at);
    let own = known
        .iter()
        .find(|l| !l.scan.finished && l.scan.terminal.as_deref() == Some(me))
        .map(|l| l.run.clone());
    let origin = run_writer::origin(&own_dir, Some(&own_repo));

    let (run, mut seq, scan, before, mut out) = if event == "start" {
        if let Some(run) = own {
            return Err(Fail::Tool(format!(
                "refused: you already have the open run `{run}`; read_run it, or finish it first"
            )));
        }
        let title = need(args, "title", "start")?;
        let terminal = client
            .list_terminals(None)
            .map_err(server_error)?
            .into_iter()
            .find(|t| t.id == me)
            .ok_or_else(|| Fail::Tool(format!("terminal {me} is not known to the server")))?;
        let run = run_writer::new_run_id(LEAD_RUN_PREFIX);
        let started = json!({
            "v": 1, "seq": 1, "ts": utc(now_secs()), "run": run, "type": "run.started",
            "tone": "neutral", "host": {"poimen.terminal": me}, "origin": origin,
            "agent": {"harness": terminal.program},
            "subject": {"kind": "poimen.lead", "id": me, "title": title},
        });
        (run, 1, Scan::default(), Vec::new(), vec![started])
    } else {
        let run = match text_arg(args, "run") {
            Some(r) if valid_run(r) => r.to_owned(),
            Some(r) => return Err(Fail::Tool(format!("`{r}` is not a lead run id"))),
            None if event == "resume" => {
                return Err(Fail::Tool("`resume` needs the argument `run`".into()));
            }
            None => own.ok_or_else(|| {
                Fail::Tool(
                    "you have no open lead run: `start` one, or read_run to list them".into(),
                )
            })?,
        };
        let found = known
            .iter()
            .position(|l| l.run == run)
            .map(|i| known.swap_remove(i))
            .ok_or_else(|| Fail::Tool(format!("unknown run `{run}` in `{}`", at.repository)))?;
        let s = found.scan;
        if s.finished {
            return Err(Fail::Tool(format!("run `{run}` is finished")));
        }
        if event != "resume" && s.terminal.as_deref() != Some(me) {
            return Err(Fail::Tool(format!(
                "refused: run `{run}` belongs to another lead terminal; `resume` it after the user confirmed"
            )));
        }
        let seq = s.last_seq;
        (run, seq, s, found.events, Vec::new())
    };

    let mut push = |mut e: Value, custom: bool| {
        seq += 1;
        e["v"] = json!(1);
        e["seq"] = json!(seq);
        e["ts"] = json!(utc(now_secs()));
        e["run"] = json!(run);
        if custom {
            e["profile"] = json!(LEAD_PROFILE);
        }
        out.push(e);
    };
    if event == "start" {
        if let Some(plan) = args.get("plan").filter(|p| !p.is_null()) {
            push(
                serde_json::to_value(LeadRecord::Plan { plan: plan.clone() })?,
                true,
            );
        }
    } else {
        match event {
            "plan" => {
                let plan = args
                    .get("plan")
                    .filter(|p| !p.is_null())
                    .ok_or_else(|| Fail::Tool("`plan` needs the argument `plan`".into()))?;
                push(
                    serde_json::to_value(LeadRecord::Plan { plan: plan.clone() })?,
                    true,
                );
            }
            "member" => {
                let task = need(args, "task", event)?;
                let repository = need(args, "target", event)?;
                let rec = LeadRecord::Member {
                    task: task.to_owned(),
                    repository: repository.to_owned(),
                    space: opt(args, "space"),
                    terminal: opt(args, "terminal"),
                    deliver_run: opt(args, "deliver_run"),
                    machine: opt(args, "machine"),
                };
                push(serde_json::to_value(rec)?, true);
                if !scan.tasks.contains(task) {
                    let title = text_arg(args, "title").unwrap_or(repository);
                    push(
                        json!({"type": "task.started", "tone": "neutral", "task": task,
                            "subject": {"kind": "poimen.repository", "id": repository, "title": title}}),
                        false,
                    );
                }
            }
            "member_done" => {
                let task = need(args, "task", event)?;
                let outcome = text_arg(args, "outcome")
                    .filter(|o| OUTCOMES.contains(o))
                    .ok_or_else(|| {
                        Fail::Tool("`outcome` is succeeded, failed, cancelled or skipped".into())
                    })?;
                if !scan.tasks.contains(task) {
                    return Err(Fail::Tool(format!(
                        "unknown task `{task}`: record its `member` first"
                    )));
                }
                push(
                    json!({"type": "task.finished", "task": task, "outcome": outcome,
                        "tone": tone(outcome), "reason": text_arg(args, "text")}),
                    false,
                );
            }
            "dependency" => {
                let rec = LeadRecord::Dependency {
                    task: need(args, "task", event)?.to_owned(),
                    blocked_by: need(args, "blocked_by", event)?.to_owned(),
                    note: opt(args, "text"),
                };
                push(serde_json::to_value(rec)?, true);
            }
            "grant" => {
                let g = args.get("grant").filter(|g| g.is_object()).ok_or_else(|| {
                    Fail::Tool(
                        "`grant` needs the argument `grant` {id, spaces, tools, max_agents}".into(),
                    )
                })?;
                let list = |k: &str| -> Vec<String> {
                    g[k].as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let rec = LeadRecord::Grant {
                    grant: g["id"]
                        .as_str()
                        .ok_or_else(|| Fail::Tool("`grant.id` is missing".into()))?
                        .to_owned(),
                    spaces: list("spaces"),
                    tools: list("tools"),
                    max_agents: g["max_agents"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or_else(|| Fail::Tool("`grant.max_agents` is a number".into()))?,
                    expires: g["expires"].as_str().map(str::to_owned),
                };
                push(serde_json::to_value(rec)?, true);
            }
            "decision" => {
                let rec = LeadRecord::Decision {
                    text: need(args, "text", event)?.to_owned(),
                    task: opt(args, "task"),
                };
                push(serde_json::to_value(rec)?, true);
            }
            "attention" => {
                let id = need(args, "id", event)?;
                let mut e = json!({"type": "attention", "id": id, "tone": "warning",
                    "needsYou": true, "reason": need(args, "text", event)?});
                if let Some(t) = text_arg(args, "task") {
                    e["task"] = json!(t);
                }
                if let Some(t) = text_arg(args, "terminal") {
                    e["detail"] = json!(t);
                }
                push(e, false);
            }
            "resolved" => {
                push(
                    json!({"type": "attention.resolved", "id": need(args, "id", event)?}),
                    false,
                );
            }
            "note" => {
                push(
                    json!({"type": "progress", "tone": "info", "label": need(args, "text", event)?}),
                    false,
                );
            }
            "resume" => {
                push(
                    serde_json::to_value(LeadRecord::Resume {
                        terminal: me.to_owned(),
                    })?,
                    true,
                );
            }
            "finish" => {
                let outcome = text_arg(args, "outcome")
                    .filter(|o| OUTCOMES[..3].contains(o))
                    .ok_or_else(|| {
                        Fail::Tool("`outcome` is succeeded, failed or cancelled".into())
                    })?;
                push(
                    json!({"type": "run.finished", "tone": tone(outcome), "outcome": outcome,
                        "reason": text_arg(args, "text")}),
                    false,
                );
            }
            other => return Err(Fail::Tool(format!("unknown event `{other}`"))),
        }
    }
    run_writer::append(&shared, &run, &before, &origin, &out)
        .map_err(|e| Fail::Tool(format!("cannot write the run: {e}")))?;
    Ok(json!({"run": run, "repository": at.repository, "seq": seq, "recorded": event}))
}

fn tone(outcome: &str) -> &'static str {
    match outcome {
        "succeeded" => "success",
        "failed" => "danger",
        _ => "neutral",
    }
}

/// `read_run {repository?, run?}`.
pub(super) fn read_run(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["repository", "run"])?;
    let at = locate(client, env, args)?;
    let me = env.terminal_id.filter(|i| !i.is_empty());
    let mut known = lead_runs(&at);
    let mine = |s: &Scan| me.is_some() && s.terminal.as_deref() == me;
    let run = match text_arg(args, "run") {
        Some(r) if valid_run(r) => r.to_owned(),
        Some(r) => return Err(Fail::Tool(format!("`{r}` is not a lead run id"))),
        None => match known.iter().find(|l| !l.scan.finished && mine(&l.scan)) {
            Some(l) => l.run.clone(),
            None => {
                let list: Vec<Value> = known
                    .iter()
                    .filter(|l| !l.scan.finished)
                    .map(|l| json!({"run": l.run, "title": l.scan.title, "terminal": l.scan.terminal}))
                    .collect();
                return Ok(json!({"repository": at.repository, "run": null, "open_runs": list}));
            }
        },
    };
    let Lead {
        events, scan: s, ..
    } = known
        .iter()
        .position(|l| l.run == run)
        .map(|i| known.swap_remove(i))
        .ok_or_else(|| Fail::Tool(format!("unknown run `{run}` in `{}`", at.repository)))?;
    let now = now_secs() as f64;
    let mut state = run_files::fold(&events)
        .snapshot(now, &agent_events::pid_alive)
        .and_then(|r| serde_json::to_value(r).ok())
        .unwrap_or(Value::Null);
    if let Some(o) = state.as_object_mut() {
        o.remove("events");
    }
    let mut plan = Value::Null;
    let mut members: Vec<Value> = Vec::new();
    let mut dependencies = Vec::new();
    let mut grant = Value::Null;
    let mut decisions = Vec::new();
    for r in &s.records {
        match r {
            LeadRecord::Plan { plan: p } => plan = p.clone(),
            LeadRecord::Member { task, .. } => {
                members.retain(|m| m["task"] != json!(task));
                members.push(serde_json::to_value(r)?);
            }
            LeadRecord::Dependency { .. } => dependencies.push(serde_json::to_value(r)?),
            LeadRecord::Grant { .. } => grant = serde_json::to_value(r)?,
            LeadRecord::Decision { .. } => decisions.push(serde_json::to_value(r)?),
            LeadRecord::Resume { .. } => {}
        }
    }
    Ok(json!({
        "repository": at.repository, "run": run, "lead_terminal": s.terminal,
        "yours": mine(&s),
        "last_seq": s.last_seq, "state": state, "plan": plan, "members": members,
        "dependencies": dependencies, "grant": grant, "decisions": decisions,
        "note": "The server is authoritative for terminals: re-check them before you act. A grant is lost when the server restarts; request_grant again.",
    }))
}
