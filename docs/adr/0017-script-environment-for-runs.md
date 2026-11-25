# ADR 0017: Script environment for runs

- Status: Proposed; "No triggers, no schedules, no autostart" to be superseded in part, for schedules, by the Proposed [ADR 0020](0020-triggered-runs-of-agent-scripts.md)
- Date: 2026-11-23
- Issues: #748, Feature #742
- Builds on: [ADR 0007](0007-plugin-system-with-sandboxed-worker-and-view-contexts.md), [ADR 0008](0008-remote-machines-over-ssh.md), [ADR 0012](0012-mcp-server-for-agent-sessions.md), [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md)

## Context

The deliver loop of GravionLabs/skills is a TypeScript script. [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md) lets such a script run its agent sessions in panes of `poimen-server`: one operation (`poimen_client::agent::run_session`) that the CLI, the MCP server and a host module share. ADR 0016 left one question open: should poimen also get a place where scripts *run* (a script environment of its own), and with which runtime? This ADR decides that, after the host API had been built and used by the first caller (#744 session operation, #745 terminal of a running task, #746 start of a loop from the app).

What the host API showed (what a script needed, what was missing):

- **Calls a script needed.** Start a session and learn its end (`run_session`, with the stream of `started`, `status`, `ended`); read the result text of a session (`read_agent`); write a run's events itself (the script stays the writer, ADR 0016); resolve the space of the caller (`POIMEN_TERMINAL_ID`); look up a space for a repository. Nothing else was asked for. The loop's own logic (order of PBIs, checks, merge, retries) needs git, `gh`, the file system and npm packages, and none of it touches poimen.
- **What was missing.** A typed client in the script's language: a script would have to call the CLI proposed in ADR 0016 and parse its JSON lines by hand. A stable, documented record and exit-code table (ADR 0016 proposes it; the CLI command `poimen session run` is proposed in ADR 0016 and not built yet). A per-run permission grant for other spaces (ADR 0012, ADR 0016: a confirmation per call). A remote-machine live test (ADR 0016 follow-up).
- **What was not needed.** Any code running inside the server or the app: not one call needed a script to react to a terminal's output or to run while no process of the caller existed.
- **Limit of this evidence.** No script has called poimen yet. It comes from building the Rust API and from its tests (an in-process server with a fake harness, `poimen-console/tests/session.rs`) and from starting the loop from the app (#746), not from a long series of real milestone runs. Findings from real runs are added to this ADR before it is accepted; the decision below does not depend on a finding that would need an engine in the server.

A script is code the user wrote and runs with their logins. It can do real damage (it starts agents, types into terminals, runs `gh` and `git`). The project config has a trust check for this reason (#255, `trust.rs`).

## Decision

**Variant (a): scripts stay Node processes that use a typed SDK over the server protocol. poimen embeds no engine and adds no script runner to `poimen-server`.** The CLI proposed in ADR 0016 (`poimen session run`, not built yet) stays the language-neutral interface beneath it.

### The three variants

| | (a) Node process + typed SDK (`@poimen/sdk`) | (b) engine embedded in `poimen-server` (for example QuickJS) | (c) no environment, the CLI is the interface |
|---|---|---|---|
| Lifetime | The script is a process of the caller (a terminal of the run, a CI job). It lives as long as that terminal; the sessions it started live in the server and survive it ([ADR 0009](0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md)). A crashed script leaves attachable panes and can be started again. | Lives in the server, so it survives the app, but also a server update or crash; state of a half-run script must be persisted or lost. | Same as (a): the caller's process. |
| Remote machines | Two cases. The script runs locally and drives a remote space through the app's machine connection (ADR 0008); `cwd` is a path on the target, and only the local machine needs Node and the `poimen` binary. Or the script runs on the machine itself: then that machine needs Node, the SDK and the `poimen` binary next to `poimen-server`. | Runs on the server's machine, so remote scripts work with no Node. Needs a script copy per machine and the engine in every server build. | As (a), without the SDK. |
| Installed per machine | Node, the SDK and the `poimen` binary where the script runs; nothing more for `poimen-server`. | Nothing besides `poimen-server`; the server grows by the engine and its host bindings. | Only the `poimen` binary. |
| Sandbox and permissions | None of its own: the script is a normal process with the user's rights. The server's checks hold: the permission model of ADR 0012 and ADR 0016 is enforced in the server and `poimen-client`, whatever the language of the caller. | A real sandbox is possible (no file system or network unless granted) behind a permission table like ADR 0007's. This is the one thing (b) buys. It costs a host API, a manifest and a consent flow. | As (a). |
| Testing and debugging | npm, vitest, a debugger, source maps, any editor; the SDK can be faked by a socket or the in-process server of the tests. | An engine without Node's tooling: no npm packages, no debugger without more work, a second runtime to match to the skills repo, errors across a binding. | Shell scripts and `jq`; weak for a loop of this size. |
| Size and attack surface of the server | No change. A hanging or malicious script cannot stall the server; its requests are checked like any client's. | Adds an interpreter and its bindings to the process that owns every terminal. A bug in the engine or a binding is a bug in that process; a script that loops must be cut by the server. | No change. |

Why (a): the loop of GravionLabs/skills is the same code on every host and needs no engine in the server; it keeps npm, vitest and a debugger; and a hanging script cannot take down the process that owns the terminals. (b) pays only for machines without Node, or for scripts of third parties that need a sandbox, and the second case is the job of the plugin system ([ADR 0007](0007-plugin-system-with-sandboxed-worker-and-view-contexts.md)). (c) is what a script already has, and it stays; but every script re-implements argument and record parsing, which is the gap the SDK closes.

### What `@poimen/sdk` is

A thin typed client over `poimen-client`'s requests and the CLI records of ADR 0016: `runSession(request, onEvent)`, `readAgent`, `whoami`, and the types of the records and exit codes. It carries no logic of its own beyond the wire shapes, so the Rust side stays the single place of the permission check. Once the CLI exists, it transports over the `poimen` binary's JSON-line output first (no second protocol implementation to keep in step with `PROTOCOL_VERSION`); a direct socket client is a later, separate step if the process spawn proves too costly. Its first user is the host module of GravionLabs/skills (`LoopHost`). Where it is published and how it is versioned against `PROTOCOL_VERSION` is decided with the PBI that builds it.

### Where scripts live, how they are trusted, how they start

- A script lives where its owner keeps it: in a repository (the deliver loop is a script of GravionLabs/skills) or in the user's directory. poimen stores and lists no scripts and has no registry.
- A script is started by a person or by a program they started: a terminal, the **Start loop** dialog (#746), a palette entry or a CI job. **No triggers, no schedules, no autostart.** poimen adds no process that runs a script on its own.
- Trust: a script from a repository is as trusted as that repository's project file (#255). poimen starts such a command only after the trust check the project file already needs (as #746 does for the loop's command line); it does not interpret the script. A script from the user's directory is the user's own code. Once running, it can do what the user's processes can do, except in poimen, where the server's permission model applies: start freely in the caller's own space, a confirmation in the app for any other space, a refusal without a connected app.
- This ADR adds no new permission. The per-run grant for other spaces stays the separate decision ADR 0012 and ADR 0016 name.

### Relation to ADR 0007 and the MCP server

- **Plugins (ADR 0007)** run in the app, in a sandbox, with a manifest, consent and UI contributions; they have no remote. A script runs outside poimen, in the caller's process, with no UI contribution. The two do not overlap: a plugin extends the app; a script drives sessions through the same server. If a third-party script must be sandboxed, it becomes a plugin with the `process:spawn` and `panes:write` permissions of ADR 0007, not an engine in the server. A plugin that needs to run a script starts it as a program through `process:spawn`.
- **MCP server (ADR 0012, #583)** is the interface for an *agent*: tools over stdio, started by the agent. A script is not an agent and does not speak MCP. Both are thin callers of the same operations in `poimen-client` (ADR 0016), so a permission or a result change lands once.

### Not in scope

Scheduling, event triggers, an engine in the server, a script store, a sandbox for scripts and any change to the permission model.

## Alternatives

- **(b) An engine embedded in `poimen-server`.** Rejected for now: it enlarges the one process that owns every terminal, brings a second runtime and its tooling, and its sandbox is only needed for third-party code, which the plugin system covers. Revisit when scripts must run on machines without Node and the CLI is not enough.
- **(c) Only the CLI.** Kept as the base layer, not as the whole answer: a typed SDK removes parsing and exit-code code that every script would repeat.
- **A Node runtime bundled with poimen.** Adds a runtime to ship and update per platform for something the user's Node already does.
- **A script runner in the app.** Ties scripts to the app window's life and has no remote; it would be a plugin (ADR 0007).

## Consequences

- The server's size and attack surface do not change; no engine, no embedded script, no new permission.
- A script's reliability is the script's own; the server keeps the panes it started, so a crashed run can be inspected and continued.
- Node and the `poimen` binary are requirements for scripts, on the machine where they run.
- `@poimen/sdk` becomes an interface to keep stable together with the records and exit codes of ADR 0016 and `docs/protocol.md`.
- A script is not sandboxed. The user trusts it as they trust any program they run, and poimen's server-side checks are the only guard on what it does in poimen.
- Follow-ups: the CLI command `poimen session run` (ADR 0016) and the SDK; the findings of real milestone runs added here before acceptance; the per-run grant for other spaces; a live test on a remote machine.
