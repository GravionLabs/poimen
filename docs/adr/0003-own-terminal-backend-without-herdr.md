# ADR 0003: Own terminal backend without herdr

- Status: Proposed; the TUI and CLI exclusion ("Out of scope") and "the app owns layout" ("Model") superseded in part by [ADR 0006](0006-poimen-console-with-herdr-scope.md)
- Date: 2026-11-02
- Issues: #149, Feature #148

## Context

poimen is a GUI over herdr (0.9.5, API protocol 22). It uses only a part of herdr:

| Area | herdr surface used by poimen |
|---|---|
| Read | `workspace.list`, `tab.list`, `pane.list`, `ping` |
| Change | `workspace.create/focus`, `tab.create`, `pane.split/rename/send_input`, `agent.start/rename` |
| Live terminals | child process `herdr terminal session observe\|control`, NDJSON with base64 ANSI frames (`stream.rs`) |
| Events | `events.subscribe` for 17 event types plus `pane.agent_status_changed` (`watch.rs`) |
| Theme | `colorSchema: "herdr"` reads the herdr theme |

Behind this sit four things herdr provides: a persistent PTY server, a headless terminal emulator, the workspace/tab/pane model, and agent detection with a status (idle, working, blocked). The rest of herdr (TUI, prefix keys, copy mode, plugins, remote machines, update and handoff) poimen does not use or replaces with its own UI.

The dependency has costs: every live terminal is a CLI child process, early frames get dropped, a subscription is rejected when a referenced pane is gone (resubscribe on every pane-set change), the API protocol drifts between versions, and frames are parsed twice (herdr's grid to ANSI, then xterm.js). Splits and tabs are TUI concepts that poimen mirrors although it has its own editor groups.

herdr detects agents in two ways. Its hook integrations (`herdr integration`, for Claude Code `~/.claude/hooks/herdr-agent-state.sh`, for Copilot `~/.copilot/hooks/`) only report the agent session (`pane.report_agent_session` on session start). The status comes from screen heuristics: per-agent manifests with regex rules over the bottom of the buffer, updated from herdr.dev. These rules are the part that breaks when an agent CLI changes its UI, and they cover 18 agents. The user only needs Claude Code and GitHub Copilot CLI. Both have hooks that carry the status: Claude Code `SessionStart`, `UserPromptSubmit`, `Notification`, `PostToolUse`, `Stop`, `SessionEnd`; Copilot CLI `sessionStart`, `userPromptSubmitted`, `notification` (including permission prompts), `postToolUse`, `agentStop`, `sessionEnd`.

## Decision

poimen gets its own terminal backend. herdr stays as a second backend until the own one covers what poimen uses today.

### Server

- A separate daemon `poimen-server` (Rust, same Cargo workspace) owns the PTYs, so shells and agents keep running when the app closes. One server per user, socket under the user's runtime directory. The app starts it when absent and never stops it.
- PTYs via `portable-pty`. Each terminal gets `POIMEN_SOCKET` and `POIMEN_TERMINAL_ID` in its environment.
- Protocol: NDJSON over the Unix socket with a version, request/response plus an event stream, in the style poimen already speaks with herdr.
- Headless terminal state via `alacritty_terminal`, with a configurable scrollback limit. `terminal.attach` returns a snapshot (screen plus scrollback as ANSI) and then streams raw PTY bytes; snapshot and stream start are tied by a sequence number so no output is lost. Several observers, one controller; taking over is explicit.
- The space list (name, cwd) persists across server restarts. Terminals do not.

### Model

- Flat: spaces (name, cwd) with terminals. Splits, tabs and editor groups stay in the poimen UI.
- The rule "herdr owns state" becomes "the backend owns processes, the app owns layout". The app still never stores process state of its own.

### Agent status

- From hooks only, for Claude Code and GitHub Copilot CLI. No screen heuristics.
- `poimen-server integration install|uninstall|status claude|copilot` writes the hook next to existing ones and leaves herdr's hooks alone.
- Hooks report to `terminal.report_agent` with a sequence number; stale reports are dropped. A hook does nothing outside a poimen terminal and never blocks the agent (short timeout, exit 0).
- Mapping: session start → idle (agent detected), prompt submitted → working, permission notification → blocked, post tool use → working, stop → idle, session end → agent gone. Subagent events are ignored.
- Other agents show as plain terminals.

### Coexistence

- A `Backend` trait in the Tauri crate covers listing, create/close/rename, input, terminal streams and events. `HerdrBackend` wraps today's `herdr.rs`, `stream.rs` and `watch.rs`; `PoimenBackend` talks to `poimen-server`.
- Setting `backend: "herdr" | "poimen"` in `config.json`, default `herdr`, change needs a restart.
- Running herdr sessions are not migrated.

### Out of scope

Remote machines (#11), herdr plugins (Epic #3), a TUI, a CLI and skill for agents to steer other agents, agents other than Claude Code and Copilot. Each needs its own decision when it comes up.

## Consequences

- poimen runs without herdr for local work with Claude Code and Copilot. The CLI child per terminal, the resubscribe workaround and the double parsing go away.
- poimen now maintains a PTY server, its protocol and two hook integrations. Changes in the hook APIs of Claude Code or Copilot need updates in poimen.
- Hooks report what the agent tells them. A blocked state that an agent does not report (for example a dialog without a notification) stays invisible; there is no screen fallback.
- Agents can no longer steer other agents through `herdr agent prompt` on the poimen backend until a CLI exists.
- Until parity, both backends are maintained behind one trait; the herdr backend keeps remote machines and plugins available.
- Features that today depend on herdr (#96 color queries, Epic #3 plugins, #11 remote) have to be decided per backend.

## Alternatives

- **PTY inside the app process.** Simplest, but closing or crashing the app would kill every agent.
- **Screen heuristics like herdr's manifests.** Cover agents without hooks, but break on every UI change of an agent CLI and need a rule catalog. Hooks are enough for Claude Code and Copilot.
- **Full parity with herdr** (tabs and splits on the server, plugins, remote, TUI). Large and mostly unused by poimen.
- **`libghostty-vt` for the headless state.** Promising, but a C/Zig dependency in the Rust build; `alacritty_terminal` is pure Rust and mature. Can be revisited behind the same server boundary.
- **Stay on herdr.** No new maintenance, but the costs in the context remain and poimen's model stays bound to herdr's TUI concepts and protocol drift.
