# ADR 0018: Scoped write grant and lead runs for orchestration

- Status: Proposed
- Date: 2026-11-24
- Issues: #1208, #1207
- Builds on: [ADR 0012](0012-mcp-server-for-agent-sessions.md), [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md), [ADR 0008](0008-remote-machines-over-ssh.md)
- Amended by: [ADR 0021](0021-mcp-2026-07-28-and-its-extensions.md) (the push model of #1182 is MCP Events; `wait_any` stays as the fallback)
- Amendment: [#1448](#amendment-1448) (a grant can cover worktree starts, grants survive a server restart, `grant_lost`)

## Context

A lead agent coordinates deliver runs in several spaces (#1207): it starts a loop per repository, waits on them and answers blocked sessions. [ADR 0012](0012-mcp-server-for-agent-sessions.md) confirms every write in another space per call and leaves "always allow" to a separate decision; [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md) defers a per-run grant the same way. A plan over three repositories and twenty PBIs would mean dozens of dialogs. Also, a lead's context does not last a multi-hour run, so the state of the run must live outside the session.

The pieces: `confirm.rs` in `poimen-server` relays `agent.confirm` to the apps and waits for `agent.confirm_answer` (it only relays; `poimen mcp` decides when to ask). The server resolves the target space id of a write. `poimen_core::agent_events` reads runs; its reducer folds `run.*`, `task.*`, `step.*`, `progress` and `attention` events and ignores unknown types and profiles.

## Decision

This ADR amends ADR 0012 "Permissions": a per-call confirmation stays the default, and the user may replace it for one lead run with one scoped grant. It decides the grant, the lead role, the run state and `wait_any`.

### The scoped grant

A grant is held by the **server**, not by the MCP process. This changes the split of duties of ADR 0012 and `confirm.rs` today (the server only relays, `poimen mcp` decides when to ask): the server also keeps the grant, and `poimen mcp` sends each write with its caller terminal and the server answers `covered` or `needs confirmation`; `poimen mcp` still asks the app only on the second answer. The server counts `started_by` agents for the limit. The grant guards `poimen mcp` as the per-call confirmation does: a process that speaks the socket protocol itself is not stopped by it (ADR 0012).

It covers:

- **Spaces** by server-resolved space id, a fixed list at grant time. Spaces the lead creates later (`create_space`) are added to the grant only through the same confirmation (a "plan change"), never silently. Names the agent supplies are never matched.
- **Tools**: a subset of the write tools (`start_agent`, `prompt_agent`, `start_shell`, `stop_terminal`, `create_space`). `write_terminal` into a terminal not started by the lead is never covered: it keeps its per-call confirmation with the text shown (ADR 0012).
- **Maximum parallel agents**: a number across all granted spaces, counted by the server from agents the lead started (`started_by`) that are not `done`, `error` or `gone`. A start over the limit is refused with the reason, not queued.
- **Duration**: ends at the first of the end of the lead run (`run.finished`, or the lead terminal gone), revocation, or an expiry, default 8 hours and at most 24; there is no grant without an end.

It never covers another machine. A grant names spaces of the machine whose server holds it; a remote machine is reached through its own `poimen mcp` and needs its own grant, confirmed on its own (ADR 0008, ADR 0012).

**Showing and confirming.** The lead calls `request_grant` (plan summary, spaces, tools, limit, duration). The server sends `agent.confirm` with a grant reason to the apps, as for a write. The app shows a dialog (`app-dialog-shell`) with the lead agent, each space by name and path, the tools, the limit and the end, with Cancel focused (`cancelDefault`); a timeout counts as "no". It is confirmed once; the user cannot widen it in the dialog, only deny or narrow (uncheck spaces, lower the limit). Without a connected app, or outside a poimen terminal, no grant is given.

**While active.** The app lists active grants (lead, spaces, count of running agents, end) in the notification center and a status bar entry, and every covered write is still announced with `agent.write_notice`/`agent.wrote`. Covered writes need no dialog; blocked agents still wait for the user or the lead.

**Revoking.** The user revokes in the app (the entry, or the stop control of the lead's run); the lead can also revoke its own grant (`revoke_grant`) and does so at its end. The server revokes on its own when the duration ends or the lead terminal is closed. Revocation is immediate for new writes (they fall back to the per-call confirmation, or refusal without an app). **Running agents are left alone**: by the non-destructive rule, revoking ends no terminal and sends no input. The app offers "stop agents started under this grant" as a separate, explicit action with a confirmation, which uses `stop_terminal` semantics for the listed terminals only. An agent that was already prompted keeps its prompt; a blocked one stays blocked for the user.

### The lead role

- The grant belongs to one terminal, the **lead terminal**: the terminal whose id (`POIMEN_TERMINAL_ID`) called `request_grant`. The server stores it with the grant (`grant.lead_terminal`) and checks the caller's terminal id on every covered write; the same grant used from another terminal is refused. Today the caller's terminal id is taken from the request (`caller_terminal_id` in `confirm.rs`; the server only checks that the terminal exists), so the lead check is as spoofable as the per-call confirmation: a local process that claims the lead's id can use the grant. This ADR accepts that limit, as ADR 0012 does. Binding a connection to its terminal (for example a per-terminal secret that the server injects into its environment) is a separate hardening decision that would cover both.
- A grant lasts as long as the lead terminal, not as long as its session. When the lead's agent session ends or is hibernated (ADR 0014) and the user resumes it in the same terminal, the grant is still there and the new session uses it with no dialog, because the terminal id is unchanged.
- A new session in **another** terminal (a restarted lead, a new window) cannot use the old grant. It reads the run record, shows the user the plan and calls `request_grant` with `resume: <run id>`; the server presents the old grant's spaces in the dialog, and on confirmation transfers the lead terminal to the caller and ends the old grant. The user decides, so a new session never inherits a grant without the user's answer.
- One lead run has at most one active grant; a second `request_grant` for the same run replaces it after confirmation.

### Where the state of a run lives

The state is an **agent-events run of the lead**, not server state. The run is the record that the user already sees in the Runs view, it survives a server restart and a lead session, it needs no new store, and the core reader reads it.

- **File.** `<git-common-dir>/.agents/events/<run>.jsonl` of the repository of the lead's space, run id `lead-<secs>`, written by the MCP tools of the lead (an exception to "the MCP tools never write events" of ADR 0012, as `report_done` already is). The lead's own repository holds it even when the work is in others; cross-repository membership is in the records.
- **Core events only**, so the Runs view and every core reader work without a profile: `run.started` (title = the user's request, `host["poimen.terminal"]` = lead terminal); one `task.started`/`task.finished` per member run (task id `<repo-slug>#<n>`, title = repository and milestone or PBI; `attention` for a blocked session with the terminal id in its `detail`; `attention.resolved` when answered); `progress` for notes; `run.finished` at the end.
- **Plan and links in a profile.** Fields the core reader ignores go in events of profile `orchestrate@1` carried on these events: `plan` (the approved plan: repositories with path, milestone, order, cross-repository blockers), `member` (repository, space id, terminal id, the run id of its deliver loop, the machine), `grant` (grant id, space ids, tools, limit, expiry; never a secret) and `decision` (what the lead answered to a blocked session, one line). The records are append-only, one JSON object per line; the shape is `poimen-protocol`-style documented with a parsing test in the PBI that builds it.
- **Resume.** A new lead session calls `list_runs`, finds the open run `lead-*` whose `host["poimen.terminal"]` is its terminal or that the user names, and reads it with the reducer: open tasks are the members still to watch, the last `plan` is the plan, `member` records map them to terminals, and `get_events` of each member's run says where it stands. It then re-checks the truth in the server (`list_terminals`, agent status), because the record may lag; the server stays authoritative for terminals, the run record for the plan and decisions. The lead writes a `progress` event at least every 20 minutes while it waits, so the core reducer's abandonment time (one hour without events) does not mark a live run abandoned; the grant and the resume do not depend on that rule. A run shown as abandoned is offered to the user as "resume or close", never resumed silently.
- A server restart loses nothing of this; it loses grants, which are in memory and end with the server (a restarted server asks again), so a resumed lead always asks the user once.

### `wait_any` and the push model

`wait_any` (`terminals[]`, `timeout_s?`) is `wait_agent` over several terminals: it subscribes to the server's events before reading statuses, returns the **first** terminal that is `done`, `blocked`, `error` or `gone` with the status, the blocked notification text and the `report`, and lists the others still running. It is a read tool: any space, no confirmation, and it never polls.

It is the first version; it does not decide the push model of #1182. The relation:

- `wait_any` is a *pull that blocks*: the agent asks, the server answers on the first matching event. A push model would deliver the same events to an agent that is not waiting (a message into its session). Both read the same source, the server's events and the agent-events runs, and neither replaces agent-events as the truth.
- `wait_any` takes terminal ids and returns an event shaped like the push event will be (`terminal`, `status`, `text`), so the lead's loop does not change when push arrives; push may then also feed `wait_any` from its subscription, which is an implementation change only.
- If #1182 accepts a push model, `wait_any` stays for harnesses that cannot receive pushes; this ADR does not depend on that outcome and #1182 may supersede the waiting part.

## Consequences

- One confirmation covers a whole plan; the user sees and can revoke it, and the limit bounds how many agents a prompt-injected lead can start. Writes outside the granted spaces and `write_terminal` into foreign terminals still ask.
- The blast radius of one grant is the granted spaces and the number of agents; a lead that is injected can prompt agents in those spaces for up to the grant duration. The dialog says so.
- The server gains grant state and its checks, the app a dialog and an overview, the protocol the requests `agent.grant_request`, `agent.grant_revoke`, `agent.grants` and a grant reason in `agent.confirm`; each with a parsing test, and `PROTOCOL_VERSION` rises.
- A lead session can end and resume with no dialog in the same terminal, with one dialog in another.
- Grants are lost on a server restart. That is accepted: a lost grant only means more dialogs.
- `create_space`, known repositories, the overview across repositories and `wait_any` are separate PBIs of #1207; this ADR fixes their rules, not their code.
- Writes across machines stay out. A plan over several machines needs one grant per machine.
- ADR 0016's "per-run grant" follow-up is answered here: a scripted run that is not a lead run has no grant and confirms per call.

## Alternatives

- **"Always allow" per tool or per agent, no scope.** Fewest dialogs, but it ends the protection of ADR 0012 for good; scope, limit and end are what make a grant acceptable.
- **A grant held by the MCP process.** The process is the thing a prompt-injected agent controls, and it ends with the session; the server can enforce and survive.
- **A grant tied to the agent session id instead of the terminal.** A resumed session has a new id, so it would lose the grant, and the id is easy to claim. The terminal id is stable across sessions, and the user confirms any transfer.
- **Run state in the server.** Authoritative and fast, but a new store, invisible to the Runs view and to other readers, and tied to one machine. agent-events already carries runs, tasks and attentions.
- **A new core event type for plan and members.** The core is shared with other tools; a profile keeps it unchanged.
- **A grant that survives a server restart.** It would have to be stored on disk and could be replayed; asking once again is cheaper.
- **Stop all agents on revoke.** Safe against a runaway, but it ends terminals on its own and loses work, against the non-destructive rule; the explicit stop action is available.
- **Wait for push (#1182) instead of `wait_any`.** The lead needs to wait on many loops now; push is an open ADR of its own.

## Amendment (#1448)

Date: 2026-11-28. Issues: #1448, #1450; code: #1451, #1452, #1453, #1454. The status of this ADR stays Proposed.

A lead that runs unattended stops at its first worktree start, which asks per call even inside a grant, and loses its grant when `poimen-server` restarts. This amendment changes both. It replaces, for the cases below, the per-call question for a worktree start (ADR 0012, #981: the new space counts as another space), the sentences "Grants are lost on a server restart. That is accepted" and "it loses grants, which are in memory and end with the server", and the rejected alternative "A grant that survives a server restart". Everything else of this ADR and of ADR 0012 stays, including that `write_terminal` into a terminal the lead did not start is never covered.

### Worktree starts under a grant

- A grant may carry `worktrees: true`. The lead asks for it in `request_grant` (default `false`), and the grant dialog shows it as its own line with the blast radius. The user can uncheck it; an answer can never add it when it was not asked (narrow only, as for the spaces and the limit).
- It covers `start_agent` with `worktree` only when the grant has `worktrees: true`, `start_agent` among its tools and the **source space** among its spaces. Every other worktree start asks per call, also from the caller's own space.
- `max_agents` applies as for other starts: a start over the limit is refused with the reason, not queued.
- The space poimen makes on the new worktree **joins that grant by its server-resolved id**, as an explicit add right after the space is created. The server adds it only when the grant is the caller's, has `worktrees`, contains the source space, and the new space's directory is a linked worktree of the source space's repository (same git common directory). The grant never matches "any space of a granted repository", so a space the user or another agent makes later is not covered.
- A joined space is listed in the grant overview like any other, and the tools of the grant apply to it, so the lead prompts and stops its workers without a dialog. The add and its announcement happen together; a grant that was revoked or ran out in between refuses the add. A failed add closes the new space and fails the start; the worktree directory may stay, as after any failed start.
- This is the one case in which a space enters a grant without a second dialog: the user confirmed it in advance with the `worktrees` line. `create_space` still adds a space only through a confirmation.

### Grants across a restart

- The server writes its grants to `server.json` (mode 0600, in the user's private state directory) on every change, before the request that changed them is answered. It also keeps a bounded list of ended grants as `{id, reason}`, with the reason `revoked`, `expired` or `lead_gone`, each until its `expires_at` has passed. A grant replaced by a later grant of the same lead, or transferred to another lead terminal, counts as `revoked`. Unanswered requests are never written.
- A restarted server restores a grant only when it adopted the lead terminal (ADR 0009) and that terminal is alive, and `expires_at` is still ahead by the wall clock. Spaces that no longer exist are dropped from it; a grant left with none is dropped. Restore never creates a grant the file did not hold and never extends `expires_at`: downtime counts against the duration.
- Grants come only from the current state file. A damaged or newer `server.json` yields no grants: not a part of them, and none from a backup, which may predate a revocation.

### `grant_lost`

- The lead's run records the grant it holds (the `grant` record). When a write of that lead is covered by no live grant and the recorded id is neither active nor ended on the server (the state was not restored), the write is refused with `grant_lost` and no per-call dialog is shown: an unattended lead learns at once that it must call `request_grant` again, instead of waiting on a dialog nobody answers.
- An attention with the stable id `grant-lost-<grant>` is raised **on the lead's own run**, so the Runs view shows the run as needing the user. It is raised once per grant and run. If it cannot be written, the write stays refused and the tool result says so.
- A recorded grant that ended for a known reason is not lost: the write asks per call as before, and the answer names the reason.
- A lead without an open run, or whose run records no grant, keeps the per-call behaviour.
- The recorded id is written by the lead and is not trusted: it can only turn a question into a refusal, never a write into a covered one.

### Security consequences

- The blast radius of a grant with `worktrees` grows from the listed spaces to those spaces and new worktrees of their repositories. `max_agents` bounds the agents running at once, not how many worktrees are made over the duration of the grant. Each new space shows in the overview and each covered write is announced; revoking works as before.
- A grant on disk is readable and writable only by the user. A process that runs as the user and writes the file can forge a grant, or put back an earlier file with a grant that was revoked since, until its `expires_at`. Such a process can already speak the socket protocol, which the grant does not guard against (see "The scoped grant"), but the file is a second way in that outlasts the process.
- If writing the file fails, the server logs it and retries on the next change. Until then a restart can bring back the earlier state, including a grant revoked in between.

### Alternatives rejected

- **A grant matches any space of a granted repository.** No add request is needed, but spaces the user opens for their own work would be covered without being listed at any point. The explicit add keeps the list of covered spaces exact.
- **Cover every worktree start of a granted space without a separate line.** Fewer fields, but the user would not see that the grant reaches new directories and could not refuse that part alone.
- **Keep grants in memory and only report `grant_lost`.** Nothing on disk, but an update or crash at night still stops the run until the user answers.
- **A separate grant file.** The same protection and the same risks as `server.json`, with a second store to keep consistent with the terminals the grants name.
- **A notification of the app for `grant_lost`.** It is gone for a user who was not there; the attention stays on the run until it is resolved.

