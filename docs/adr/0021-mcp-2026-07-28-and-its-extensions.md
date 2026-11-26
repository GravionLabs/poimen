# ADR 0021: MCP 2026-07-28 and its extensions

- Status: Proposed
- Date: 2026-11-25
- Issues: #1299, Feature #1298, #1182
- Builds on: [ADR 0008](0008-remote-machines-over-ssh.md), [ADR 0012](0012-mcp-server-for-agent-sessions.md), [ADR 0018](0018-scoped-write-grant-and-lead-runs-for-orchestration.md)
- Amends: [ADR 0012](0012-mcp-server-for-agent-sessions.md) (protocol revision, long waits as tasks), [ADR 0018](0018-scoped-write-grant-and-lead-runs-for-orchestration.md) (`wait_any` and the push model of #1182)

## Context

`poimen mcp` ([ADR 0012](0012-mcp-server-for-agent-sessions.md)) is a stdio MCP server that speaks the revisions `2025-06-18`, `2025-03-26` and `2024-11-05` after an `initialize` handshake and declares only `tools` (`poimen-console/src/mcp.rs`).

The MCP revision `2026-07-28` makes the protocol stateless: no `initialize`, every request carries its revision and client capabilities in `_meta`, a server answers `server/discover`, results carry `resultType`, `ping` is gone, and state across calls is a server-minted handle passed as an argument. Optional extensions are negotiated in `capabilities.extensions`. Three of them meet open questions of poimen:

- **Tasks** (`io.modelcontextprotocol/tasks`, generally available since 2026-11-24): a request returns a task handle instead of blocking; the client polls `tasks/get` until `completed`, `failed` or `cancelled`, can answer `input_required` through `tasks/update` and cancel with `tasks/cancel`. The task must exist before the handle is sent, and a client may poll it again after a restart.
- **Events** (`io.modelcontextprotocol/events`, beta): `events/list` names event types with argument and payload schemas; a client subscribes by push (`events/stream`, a held request on the same connection), poll (`events/poll`) or webhook (`events/subscribe`), and resumes from an opaque cursor.
- **Skills** (`io.modelcontextprotocol/skills`, beta): `skills/list` and `skills/get` return `SKILL.md` entries with a manifest (SHA-256 and size per file); the files are read with `resources/read`. The host verifies the bytes and asks the user before loading.

Elicitation (now the multi round-trip pattern: a result `input_required` that the client answers by retrying the request) lets a server ask the user of the calling client.

Where poimen stands:

- Long blocking tool calls: `wait_agent` and `wait_any` (`timeout_s`), `request_grant` (up to 120 s) and every write that waits for `agent.confirm` (60 s). A harness timeout or a restarted session loses the wait.
- #1182 asks how an agent is woken by an event instead of calling `wait_agent`, `get_events` or `read_terminal` in a loop; [ADR 0018](0018-scoped-write-grant-and-lead-runs-for-orchestration.md) made `wait_any` the first version and left the push model open. `report_done` already types one line into an idle starter.
- The orchestration skill (#983) lives in GravionLabs/skills and names tools of `poimen mcp`; a skill and a binary of different versions disagree.
- Host support on 2026-11-25: the client matrix of modelcontextprotocol.io lists no support for Tasks, Events or Skills in Claude Code or VS Code Copilot; Skills partially in ChatGPT and fast-agent.

## Decision

`poimen mcp` speaks `2026-07-28` next to the older revisions and offers Tasks, Events and Skills as opt-in extensions. Every tool keeps its behavior for a client that does not declare an extension. The cross-space confirmation stays in the app.

### Revisions

- `server/discover` is answered always: the supported revisions (`2026-07-28` first), `capabilities` with `tools`, `resources` when skills are on, and `extensions` as built, `serverInfo`, `ttlMs` and `cacheScope`.
- A request with `io.modelcontextprotocol/protocolVersion` in `_meta` is answered by that revision, with or without an earlier `initialize`; an unknown revision is `UnsupportedProtocolVersion` (`-32022`). A request without it uses the revision agreed by `initialize`, as today.
- Results of `2026-07-28` carry `resultType` and, where the revision requires them, `ttlMs` and `cacheScope`; `tools/list` keeps a fixed order. `ping` stays for the old revisions.
- The MCP process stays without state of its own (ADR 0012). The revision changes nothing in the tools, the permissions or the socket.

### Tasks

The server decides per request, and only when the request declared `io.modelcontextprotocol/tasks`:

| Tool | Returns a task |
| ---- | -------------- |
| `wait_agent`, `wait_any` | always |
| `request_grant` | always |
| a write tool (`start_agent`, `prompt_agent`, `start_shell`, `stop_terminal`, `write_terminal`, `create_space`) | only when it waits for `agent.confirm` |
| every other tool | never |

- **Result.** The result of a `completed` task is the result the synchronous call gives, including a refusal, a denied confirmation and a timeout. `failed` is for what is a tool error today (unknown terminal, lost server). A blocked agent ends `wait_agent` as today (`completed`, status `blocked`); the task does not use `input_required`, because the input belongs in the agent's pane or in `prompt_agent`, not to the waiter.
- **Where a task lives.** A wait task is a handle minted by `poimen mcp` that encodes the request (tool, arguments, start, deadline). `tasks/get` evaluates it again against the server: one `terminal.list` and the run files read with the core `Tailer`, no loop. So any `poimen mcp` process answers it, also after the client or the MCP process restarted, and no store is added. A handle carries only what the agent could pass to the read tool itself; a forged handle reads no more than the read tools do.
- A confirmation task needs the pending write to outlive the MCP process. `confirm.rs` already gives each question an id (`secret_id("confirm")`) but drops it when the waiting request ends; it keeps the question and its answer until the confirm wait ends, the handle names the id, and the write runs once, on the first `tasks/get` that sees "yes" (the server marks the confirmation used). A server restart loses it, and the task is `failed` with that reason; asking again is cheap (as for grants in ADR 0018).
- `tasks/cancel` ends the wait and withdraws a pending confirmation. It never ends a terminal and never sends input (non-destructive).
- `pollIntervalMs` is 2000 ms. With Events (below) a client can be told instead of polling; `poimen mcp` itself never polls the server.
- `ttlMs` of a wait task is its deadline (`timeout_s`, default 300 s, at most 3600 s as today); of a confirmation task the confirm wait.

### Events

Events are the push model of #1182. They deliver what the server's events and the agent-events runs already record; agent-events stays the truth.

| Event type | Arguments | Payload | Cursor |
| ---------- | --------- | ------- | ------ |
| `terminal.status` | `terminals[]` or `space` | `terminal`, `status`, `text`, `report` (the shape `wait_any` returns) | none: from now |
| `run.event` | `repository`, `run`, `types[]?` | the agent-events record | `<run>:<seq>` |
| `attention` | `space` or `repository` + `run` | terminal or task, `text` | as its source |

- Delivery is push (`events/stream`) and poll (`events/poll`). Webhook is not offered: it needs an open port and a stored callback, which ADR 0008 avoids.
- `terminal.status` has no addressable history: a client that resumes starts from now and reads `get_terminal` for the state it missed. `run.event` resumes from the record `seq` and says `truncated` when the run file no longer holds it.
- A subscription is read-only: any space, no confirmation, like the read tools.
- The sources are the ones `wait_any` uses: the server's event subscription (opened before the first read) and a watcher on the run directories. `wait_any` and the events share that code.
- No message bus is embedded and none is connected; the dromos question of #1182 is answered by this extension.

### Skills

- Behind the setting `mcp.skills`, off by default until Claude Code or Copilot CLI load skills over MCP: `poimen mcp` declares `resources` and `io.modelcontextprotocol/skills` with `directoryRead: false` and serves one skill, `skill://poimen-orchestrate/SKILL.md`: the loop over `start_agent`, `wait_any`, `read_agent` and `report_done` and the grant rules of ADR 0018.
- The skill is compiled into the binary, so it matches the tools of the same version. No file of a project or the user is served: an untrusted project (#255) must not reach an agent's instructions through poimen.
- The manifest is computed from the served bytes; an unknown URI is `-32602`. Verification and approval are the host's job.
- The text lives in this repository. GravionLabs/skills keeps the generic deliver loop and refers to this skill for the tools of poimen.

### Not adopted

- **Elicitation for the cross-space confirmation.** The calling harness does not know which space a write targets, and a prompt-injected agent sits in that harness; ADR 0012 put the confirmation in the app for that reason, and it stays there.
- **MCP Apps, OAuth and enterprise auth.** `poimen mcp` is local over stdio, and its views are the app's.
- **An HTTP transport.** As in ADR 0012.

### What becomes obsolete

Nothing is removed while hosts lack the extensions.

- `timeout_s` and the long blocking calls stay for clients without Tasks; with Tasks the deadline is only the task's `ttlMs`.
- `wait_any` stays for harnesses without Events, as ADR 0018 foresaw; it is no longer "the first version" of a push model but its fallback.
- The open choice of #1182 (embedded bus, dromos, own protocol) is closed by Events.
- The separately installed orchestration skill becomes optional for hosts that load skills over MCP. The `report_done` preamble of `start_agent` stays: a worker needs it without loading a skill.
- `ping` is not offered in `2026-07-28`.

## Consequences

- One stateless dispatch serves old and new clients; tests cover both paths per revision.
- Long waits survive a harness timeout and a restart of the agent's session when the host supports Tasks, without state in `poimen mcp`; confirmations in `confirm.rs` keep their answer and a "used" mark.
- An agent can be woken by poimen through a standard every MCP host may implement, instead of a poimen-only mechanism.
- The orchestration skill and the tools ship together; the setting keeps it off until a host loads it.
- Each extension is a separate PBI of #1298 (#1300 revisions, #1301 Tasks, #1302 Events, #1303 Skills) and goes live only when a host supports it. Until then the work is tested against the spec with a test client, not with a harness.
- The tool tables of `site/guide/mcp.md` and `docs/agents/console.md` gain a column for the extension per tool.

## Alternatives

- **Wait for host support.** Costs nothing now, but #1182 stays open and a lead keeps polling. The revision PBI is small, and each extension is opt-in.
- **Only `2026-07-28`.** Today's harnesses speak `2025-06-18` and older.
- **A task store in `poimen mcp` or the server.** Durable for any request, but state that the stateless revision avoids; the waits can be evaluated again from what the server owns.
- **Tasks with `input_required` for a blocked agent.** It would route the answer to the waiting agent instead of the user or the lead's `prompt_agent`, which is the input path the user sees.
- **Our own push protocol or a bus for #1182.** Works with no host support but needs a poimen-only client in every harness; Events is the standard for the same need.
- **Serve the project's skills (`.agents/skills`) over MCP.** Harnesses already read them from disk; serving them would pass untrusted project text through poimen.
