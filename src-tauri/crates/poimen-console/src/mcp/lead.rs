//! `record_run` and `read_run` (#1212, ADR 0018): the state of a lead run lives in an
//! agent-events run `lead-<secs>` in the repository of the lead's space, not in the session. The
//! lead writes the plan, its members, the grant and its decisions; a new lead session reads them
//! back and resumes. Core events (`run.*`, `task.*`, `attention*`, `progress`) show the run in the
//! Runs view; the records of the profile `orchestrate@1` (`poimen_protocol::lead`) carry the rest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use poimen_client::Client;
use poimen_core::agent_events::{self, Reducer};
use poimen_protocol::files::{now_secs, utc};
use poimen_protocol::lead::{LEAD_PROFILE, LEAD_RUN_PREFIX, LeadRecord};
use serde_json::{Value, json};

use super::{Env, Fail, report, runs, server_error, text_arg};

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
        "description": "Records the state of YOUR lead run outside your session, so a new lead session can resume it (read_run). It appends to an agent-events run `lead-<secs>` in the repository of your space, which the Runs view shows. Write after every step: `start` (title = the user's request; optional `plan`) once, then `plan` (the approved plan: repositories with path, milestone, order, blockers; the newest replaces older ones), `member` (a started loop: `task` `<repo>#<n>`, `target` = its repository directory, `space`, `terminal`, `deliver_run`, `machine`), `member_done` (`task`, `outcome`, `text`), `dependency` (`task`, `blocked_by`), `grant` (what request_grant gave: `grant` id, `spaces`, `tools`, `max_agents`), `decision` (what you answered to a blocked session: `text`, `task`), `attention` / `resolved` (a blocked session: `id`, `task`, `terminal`, `text`), `note` (progress, `text`; write one at least every 20 minutes while you wait so the run is not shown abandoned), `resume` (after the user confirmed taking over a run from another terminal; `run`) and `finish` (`outcome`, `text`). Never record a secret. A write to your own run file: no confirmation.",
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
}

fn scan(text: &str) -> Scan {
    let mut s = Scan::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(e) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        s.last_seq = s.last_seq.max(e["seq"].as_i64().unwrap_or(0));
        match e["type"].as_str() {
            Some("run.started") => {
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
        if let Some(r) = LeadRecord::from_line(line) {
            if let LeadRecord::Resume { terminal } = &r {
                s.terminal = Some(terminal.clone());
            }
            s.records.push(r);
        }
    }
    s
}

fn lead_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| agent_events::is_run_file(p))
        .filter_map(|p| {
            let run = agent_events::run_of(&p)?.to_owned();
            run.starts_with(LEAD_RUN_PREFIX).then_some((run, p))
        })
        .collect();
    files.sort();
    files
}

fn load(path: &Path) -> Option<(String, Scan)> {
    let text = runs::read_complete(path)?;
    let s = scan(&text);
    Some((text, s))
}

/// The open lead runs of the directory, newest first, with their scan.
fn open_runs(dir: &Path) -> Vec<(String, Scan)> {
    let mut v: Vec<_> = lead_files(dir)
        .into_iter()
        .rev()
        .filter_map(|(run, p)| load(&p).map(|(_, s)| (run, s)))
        .filter(|(_, s)| !s.finished)
        .collect();
    v.sort_by_key(|(r, _)| std::cmp::Reverse(r.clone()));
    v
}

struct Where {
    dir: PathBuf,
    repository: String,
}

fn locate(client: &Client, env: &Env, args: &Value) -> Result<Where, Fail> {
    let repository = match text_arg(args, "repository") {
        Some(r) => r.to_owned(),
        None => {
            let caller = caller(env)?;
            client
                .list_terminals(None)
                .map_err(server_error)?
                .into_iter()
                .find(|t| t.id == caller)
                .map(|t| t.cwd)
                .ok_or_else(|| {
                    Fail::Tool(format!("terminal {caller} is not known to the server"))
                })?
        }
    };
    let dir = runs::events_dir(&repository).map_err(Fail::Tool)?;
    Ok(Where { dir, repository })
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
    let own = open_runs(&at.dir)
        .into_iter()
        .find(|(_, s)| s.terminal.as_deref() == Some(me));

    let (run, mut seq, scan, mut out) = if event == "start" {
        if let Some((run, _)) = own {
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
        let mut secs = now_secs();
        while at
            .dir
            .join(format!("{LEAD_RUN_PREFIX}{secs}.jsonl"))
            .exists()
        {
            secs += 1;
        }
        let run = format!("{LEAD_RUN_PREFIX}{secs}");
        let started = json!({
            "v": 1, "seq": 1, "ts": utc(now_secs()), "run": run, "type": "run.started",
            "tone": "neutral", "host": {"poimen.terminal": me},
            "agent": {"harness": terminal.program},
            "subject": {"kind": "poimen.lead", "id": me, "title": title},
        });
        (run, 1, Scan::default(), vec![started])
    } else {
        let run = match text_arg(args, "run") {
            Some(r) if valid_run(r) => r.to_owned(),
            Some(r) => return Err(Fail::Tool(format!("`{r}` is not a lead run id"))),
            None if event == "resume" => {
                return Err(Fail::Tool("`resume` needs the argument `run`".into()));
            }
            None => own.map(|(r, _)| r).ok_or_else(|| {
                Fail::Tool(
                    "you have no open lead run: `start` one, or read_run to list them".into(),
                )
            })?,
        };
        let (_, s) = load(&at.dir.join(format!("{run}.jsonl")))
            .ok_or_else(|| Fail::Tool(format!("unknown run `{run}` in `{}`", at.repository)))?;
        if s.finished {
            return Err(Fail::Tool(format!("run `{run}` is finished")));
        }
        if event != "resume" && s.terminal.as_deref() != Some(me) {
            return Err(Fail::Tool(format!(
                "refused: run `{run}` belongs to another lead terminal; `resume` it after the user confirmed"
            )));
        }
        let seq = s.last_seq;
        (run, seq, s, Vec::new())
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
    report::append(&at.dir, &run, &out)
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
    let open = open_runs(&at.dir);
    let run = match text_arg(args, "run") {
        Some(r) if valid_run(r) => r.to_owned(),
        Some(r) => return Err(Fail::Tool(format!("`{r}` is not a lead run id"))),
        None => match open
            .iter()
            .find(|(_, s)| s.terminal.as_deref() == me && me.is_some())
        {
            Some((r, _)) => r.clone(),
            None => {
                let list: Vec<Value> = open
                    .iter()
                    .map(|(r, s)| json!({"run": r, "title": s.title, "terminal": s.terminal}))
                    .collect();
                return Ok(json!({"repository": at.repository, "run": null, "open_runs": list}));
            }
        },
    };
    let (text, s) = load(&at.dir.join(format!("{run}.jsonl")))
        .ok_or_else(|| Fail::Tool(format!("unknown run `{run}` in `{}`", at.repository)))?;
    let mut reducer = Reducer::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        reducer.push_line(line);
    }
    let now = now_secs() as f64;
    let mut state = reducer
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
        "yours": s.terminal.as_deref() == me && me.is_some(),
        "last_seq": s.last_seq, "state": state, "plan": plan, "members": members,
        "dependencies": dependencies, "grant": grant, "decisions": decisions,
        "note": "The server is authoritative for terminals: re-check them before you act. A grant is lost when the server restarts; request_grant again.",
    }))
}
