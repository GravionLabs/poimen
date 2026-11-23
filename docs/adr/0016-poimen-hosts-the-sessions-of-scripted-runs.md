# ADR 0016: poimen hosts the sessions of scripted runs

- Status: Accepted
- Date: 2026-11-23
- Issues: #743
- Builds on: [ADR 0006](0006-poimen-console-with-herdr-scope.md), [ADR 0008](0008-remote-machines-over-ssh.md), [ADR 0010](0010-terminal-panel-for-shells-in-the-app.md), [ADR 0012](0012-mcp-server-for-agent-sessions.md)

## Context

The deliver loop of GravionLabs/skills is a TypeScript script that works through the PBIs of a milestone. For every PBI it runs one agent session. Today the script starts the harness as a child process, so the user sees no terminal, poimen knows nothing about the session, and a crashed script leaves nothing to attach to. The skills ADR 0005 puts the loop behind a host interface (`LoopHost`, `DELIVER_HOST`); a host that is poimen would run each session in a pane of `poimen-server`, where the user can watch it, attach to it and answer a blocked agent.

The pieces exist. `poimen_client::agent` starts an agent in a terminal (`start_agent`) and prompts it (`prompt_agent`); the server learns the agent's state from its hook reports (`terminal.report_agent`); ADR 0012 added `wait_agent` and `read_agent` (the transcript, also for remote machines); `poimen-core/harness.rs` builds the argv of a harness from the templates of the config, per element, never through a shell; a poimen terminal sets `POIMEN_SOCKET` and `POIMEN_TERMINAL_ID`. What is missing is one operation that a script can call, with a defined result.

Three callers need it: the host module of a script, the write tools of the MCP server ([ADR 0012](0012-mcp-server-for-agent-sessions.md)) and the CLI. Three code paths would drift apart.

## Decision

poimen offers one operation, **run a harness session in a pane and report its end**, in `poimen_client::agent`. The CLI command `poimen session run`, the host module of a script and the MCP tools are thin callers of it.

### The session is interactive

The harness runs as in a terminal the user opened: the agent starts in a new terminal, its prompt reaches it as the harness's `promptArgs` (else typed with `prompt_agent` once `SessionStart` is reported), and its state comes from the hook reports. The end of the session is the agent status `done`, `error` or `gone`, or a closed or exited terminal, the same as `wait_agent`. The result text is read afterwards with `read_agent` (`agent.transcript`).

Why: the user can watch the session, attach to it and answer a `blocked` agent; the pane stays a normal terminal ([ADR 0010](0010-terminal-panel-for-shells-in-the-app.md)); everything needed exists. A harness without hook reports cannot be run this way and is refused with that reason.

A print-mode session (`printArgs`, a headless process, output from the PTY, the exit code as the end) is not part of this decision. It is a later, separate decision for harnesses without hooks.

### Input

The operation takes: harness, model (optional), prompt, `cwd`, the space (default: the space of `POIMEN_TERMINAL_ID`), a run id (optional), environment variables (optional), the tab name and the placement (optional). The harness command comes from the harness templates of the config; argv only. A prompt with spaces, quotes, `$()`, `;` or newlines stays one argument.

The run id becomes `AGENT_EVENTS_RUN` in the terminal's environment, next to the variables the caller passes.

### Result

The CLI command prints one JSON object per line on stdout and nothing else:

- `{"type":"started","terminal":"<id>","space":"<id>"}` once the terminal exists.
- `{"type":"status","status":"working|blocked|done|error|gone"}` for every change of the agent status. A `blocked` agent does not end the wait; the user answers it in the pane.
- `{"type":"ended","status":"done|error|gone","terminal":"<id>"}` once, as the last line.

The result text is not part of the records: the caller reads it with `read_agent`, which keeps the stream small and stable. Diagnostics go to stderr.

Exit codes (the numbers are proposed; the PBI that builds it documents the final table):

| Code | Meaning |
| ---: | ------- |
| 0 | the session ended normally (`done`) |
| 2 | usage error |
| 3 | the start failed (no such harness or space, harness without hooks, spawn error) |
| 4 | the session ended with an error (`error` or `gone`) |
| 5 | the connection to the server was lost |
| 6 | a permission was refused |
| 130, 143 | the wait was ended by SIGINT, SIGTERM |

SIGINT or SIGTERM ends the wait and leaves the terminal running, so the user can attach. With `--close-on-end` the command instead ends the session with the harness's `exitInput` and closes the terminal, but only when the caller asked for it.

### Permissions

The model of [ADR 0012](0012-mcp-server-for-agent-sessions.md) holds, and a scripted run starts with the strict form:

- A session starts freely in the space of the calling terminal.
- Any other space needs a confirmation in the app, per call. A confirmation for a whole run ("this run may start sessions in space X") is possible later as a separate decision, as ADR 0012 already says of "always allow".
- Without a connected app, or outside a poimen terminal, a start in another space is refused with the reason.
- The check is made by the server on the space id it resolved, not on names the caller supplies; it is enforced in the server and in `poimen-client`, not only in the CLI.
- A project config that is not trusted (#255) contributes no harness templates; the session starts from the user's templates only.

### Where the session appears

A new terminal in the space, as an `editor` tab ([ADR 0010](0010-terminal-panel-for-shells-in-the-app.md)), named `<run id> <title>` (the title is the PBI or the first words of the prompt). The caller may set the name and the placement (`editor` or `panel`); the defaults hold when it does not.

When the session ends, the terminal stays open with its status, as every terminal does (AGENTS.md "Non-destructive"). Only the caller's explicit `--close-on-end` closes it. With hibernation ([ADR 0014](0014-hibernate-idle-agents.md)) switched on, the server may stop the idle agent of a `done` pane and resume the session on the first attach; the pane itself remains.

### Remote spaces

All requests go through `poimen-client`, so they work through `poimen-server stdio` ([ADR 0008](0008-remote-machines-over-ssh.md)); `cwd` is a path on that machine, and `read_agent` works without a local transcript. This is by construction and not yet tried: a live test on a remote machine is a follow-up item, not a condition of this ADR.

### Relation to agent-events

The script stays the writer of its run. poimen writes no events for a scripted run, and the agent of such a run is not told to call `report_done`. poimen is a controller for the commands, as for any caller of `poimen-client`; it reads runs with `poimen_core::agent_events` as before.

### No persistence

A run starts on request only. This decision adds no scheduled task, service, autostart entry or timer.

### Relation to ADR 0007

A run is not a plugin: the plugin worker lives with the app window and has no remote. A script environment inside poimen (a place where scripts run, not only call) is a separate, later decision.

## Consequences

- One code path serves the CLI, the host module and the MCP server; new callers do not copy the logic.
- The user watches and can steer every session of a run. A `blocked` agent waits for the user, so a run can stall until someone answers; the wait has no timeout of its own, and the caller ends it with a signal.
- Only harnesses with hook reports can run (Claude Code, Copilot at present). A headless mode for others is a later decision.
- Sessions in other spaces need a confirmation per call, which costs interruptions for a script that spreads over spaces; a per-run grant can follow.
- The result records and exit codes are an interface to keep stable and are documented in `docs/protocol.md` and the console docs.
- A pane per PBI stays after the run; closing is the user's job or the caller's `--close-on-end`.
- Follow-ups: a live test on a remote machine; the print mode; a per-run grant for other spaces; the script environment inside poimen ([ADR 0017](0017-script-environment-for-runs.md), #748).

## Alternatives

- **Print mode as the first mode.** Simple, because the exit code is the end, but the user cannot answer a blocked agent, the pane shows only output, and permissions must be fixed by flags beforehand.
- **Both modes now.** Two code paths and two result shapes before one has been used.
- **The script starts a terminal and drives it itself** (`poimen new`, `poimen send`, a poll of the status). Every script re-implements the wait and the end detection, with no shared permission check.
- **A new MCP tool only.** The deliver loop is a Node script, not an agent; it would need an MCP client for what a command does.
- **A confirmation per run now.** Fewer interruptions, but it needs a dialog and a protocol request of its own, and it widens what a prompt-injected script may do; ADR 0012 deferred "always allow" for that reason.
- **Close the pane when the session ends.** It would end a terminal on its own, against the non-destructive rule, and the user could not read what happened.
