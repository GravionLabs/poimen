# ADR 0012: MCP server for agent sessions

- Status: Proposed
- Date: 2026-11-15
- Issues: #612
- Builds on: [ADR 0006](0006-poimen-console-with-herdr-scope.md)

## Context

Agents run in poimen terminals, and poimen-server knows what the app shows: spaces, terminals, the agent status that the hooks report, token counts and (through `poimen-core`) the runs of agent-events. The agent itself cannot ask for any of it. Two needs follow (#583):

- An agent should see its surroundings: which spaces and terminals exist, which agents work or wait, which space it runs in, which deliver runs are open.
- An agent should act in them: start another agent in a terminal, or prompt one, without the user typing it.

The Model Context Protocol (MCP) is the interface that Claude Code, Copilot CLI and Gemini CLI already load tools from. A local MCP server started by the agent over stdio needs no open port and no stored secret, which fits [ADR 0008](0008-remote-machines-over-ssh.md).

The pieces exist: `poimen-client` has the requests and `Client::start_agent`/`prompt_agent` (`agent.rs`), `docs/protocol.md` documents them, the console binary `poimen` ([ADR 0006](0006-poimen-console-with-herdr-scope.md), #170) already holds a CLI over `poimen-client`, and a poimen terminal sets `POIMEN_SOCKET` and `POIMEN_TERMINAL_ID`. `poimen_core::agent_events` is the reader of runs that the app uses.

The risk is write access. An agent that can prompt any terminal can type into the shell or the agent of another project, so a prompt-injected agent would reach everything poimen manages.

## Decision

poimen offers an MCP server for agent sessions, and writes outside the caller's own space need the user's confirmation in the app.

### Shape

- `poimen mcp` is a subcommand of the console binary (#170). The agent starts it as a stdio MCP server (JSON-RPC over stdin and stdout); it ends when stdin closes.
- It is a client of poimen-server like `poimen ls` and `poimen send`: it uses `poimen-client` only and the requests of `docs/protocol.md`, with no protocol code of its own. The server is `terminal_socket_path()` (`POIMEN_SOCKET` in a poimen terminal, else the default). It never starts or stops a server.
- The server stays the owner of state. The MCP process keeps no state beyond the caller's id and reads the server on every call.
- Out of scope: GitHub data, Changes and Flow.

### Tools

Read tools:

- `list_spaces`: spaces with their tabs.
- `list_terminals`: terminals with space, name, status, agent kind, agent status and tokens.
- `current_space`: the space and terminal of the caller, found through `POIMEN_TERMINAL_ID`. Outside a poimen terminal it answers that there is none.
- `list_runs`: deliver runs of a repository, read with `poimen_core::agent_events` (the same reducer and `Tailer` as the app).

Write tools:

- `start_agent` (`Client::start_agent`): starts an agent in a terminal, optionally in a new terminal of a space.
- `prompt_agent` (`Client::prompt_agent`): types a prompt into the agent of a terminal, refused while that agent is working or blocked, as for `poimen prompt`.

Results are text and structured content with ids that the tools accept. Tool errors are MCP errors with the server's `code` and `message`, never a crash.

### Permissions

- A write tool acts freely in the space of the calling terminal (the space of `POIMEN_TERMINAL_ID`).
- A write tool that targets any other space needs a confirmation in the app. The app shows the agent, the target space and terminal, and the prompt or command; the call waits for the user's answer, and a timeout counts as "no". The confirmation is per call; there is no "always".
- Without a connected app, or outside a poimen terminal (no `POIMEN_TERMINAL_ID`), a write to another space is refused with the reason. Read tools do not need the app.
- The check uses the space id of the target, resolved by the server, and not names that the agent supplies. How the request reaches the app (a request and event pair in `docs/protocol.md`) is decided with the write tools (#615).

### Relation to agent-events

Deliver runs come from the same core reader as the app: `poimen_core::agent_events` (agent-events core only). The MCP tools never write events. The status of agents comes from the hook reports that the server already keeps.

## Consequences

- An agent can plan with knowledge of the workspace, and a lead agent can start and prompt others. Both use the status the user sees.
- The app gains a confirmation dialog for cross-space writes, and the protocol gains the request that carries it (#615). Until then the write tools do not exist.
- Writes by an agent stay bounded to its own space unless the user agrees each time. That costs interruptions for cross-space work; "always allow" can come later as a separate decision.
- The tool list becomes an interface to keep stable and documented. New tools (GitHub, Changes, Flow) need their own decision.
- Delivery is split: read-only tools #613, installation as an integration next to the hooks (`poimen-server integration install`, the kinds of `integration.rs`) #614, write tools #615.
- A remote machine is reached through its own `poimen mcp` on that machine; this ADR adds no cross-machine tool.

## Alternatives

- Extend the CLI only (`poimen ls`, `poimen prompt`). Agents can already run it, but they need instructions on how, and the output is for people. MCP gives typed tools that every supported agent lists by itself. The CLI stays.
- An MCP server inside poimen-server over HTTP. It would open a port and need credentials, which ADR 0008 avoids, and it could not tell which terminal calls.
- A separate binary `poimen-mcp`. One more artifact to ship and sign; the console binary already has the client code and the packaging.
- Writes free in every space. Simple, but one injected prompt would reach every project and terminal.
- Writes never outside the own space. Safest, but rules out a lead agent coordinating work in several spaces; the confirmation keeps that possible.
- Confirmation in the agent's own permission prompt only. The agent harness does not know which space is the target, and not every harness asks.
