# ADR 0014: Hibernate idle agents

- Status: Proposed
- Date: 2026-11-18
- Issues: #901, Feature #720
- Builds on: [ADR 0003](0003-own-terminal-backend-without-herdr.md), [ADR 0009](0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md)

## Context

An agent that finished its turn (status `done`) keeps its process, and its memory, until the user closes the terminal. With many spaces open, idle agent processes add up. Claude Code and Copilot CLI can resume a session from its transcript, so the process of an idle agent can end and come back later without losing the conversation.

`AGENTS.md` says poimen is non-destructive: closing a view or the app never ends a terminal. Hibernation ends a process the user started, so it is an exception that has to be recorded before any code exists.

What the server knows: the hook-reported agent status per terminal (`working`, `blocked`, `done`, none), the session id (`SessionStart`, ADR 0003), the terminal's PTY host and controllers.

## Decision

poimen-server may stop an idle agent process, never the terminal, and resume the session later. Hibernation is off by default.

### Who

The server. It knows status and session id, and it works without the app and for the TUI (ADR 0006). The app and the TUI only show the state.

### Safety rules

All of these hold, else nothing happens:

- The setting is on (off by default; the idle time is a setting, with a minimum).
- The agent status is `done` and has been `done` without change for the set time. Never `working`, never `blocked`, and never a terminal without a hook-reported status (silent: no hooks, unknown state).
- The harness of the terminal defines `exitInput` (what to type to end the agent) and `resumeArgs` (the arguments that resume a session, with `{session}`). A harness without both is never hibernated.
- The session id is known (from the hooks), so the resume goes to the same session.
- No controller is typing: a terminal with a controller attached is not hibernated.
- No input reached the terminal after the `done` report (#988): text in the agent's input box (a draft) would be lost under the exit input. The server keeps the time of the last input in memory; a new status report starts a new `done` period.
- No unsent prompt draft in the app (#1163): text in the app's prompt bar is not input to the terminal, so the server cannot see it. The app tells it with `terminal.set_draft` on its attached connection; the server keeps the draft in memory per attached client and drops it when that client detaches or disconnects. `hibernate_sweep` skips a terminal with a draft, and once it is cleared the idle time counts as for any `done` terminal (the `done` period did not restart).

### How an agent is stopped

The server types the harness's `exitInput` at the prompt, which is safe because the status is `done` and the agent waits for input. The shell and the terminal stay; only the agent process ends. The server then records the hibernated state (session id, harness, working directory) with the terminal in `server.json`. poimen never sends a signal and never kills the terminal.

If the agent does not exit within a timeout, the server gives up, leaves the terminal as it is and reports the failure; it does not force it.

### When it is resumed

Automatically on the first controller attach to the terminal. An observer (the sidebar, a second client, `poimen attach` without `--control`) triggers nothing. The server types the harness's command with `resumeArgs` and the stored session id into the shell, and the status follows the hooks again.

### What is lost

The agent's in-memory state beyond its transcript: running background tasks and subagents, unsaved prompt text, tool permissions granted for the session, and anything the agent held in its process that the transcript does not carry. The terminal's scrollback and the shell's state stay.

## Consequences

- An exception to the non-destructive rule of `AGENTS.md`, named there with a link to this ADR. The rule "closing a view or the app never ends a terminal" stays true.
- New harness fields `exitInput` and `resumeArgs`, new settings (on/off, idle time), a hibernated state in the protocol and in `server.json`; each wire shape lands in `poimen-protocol` with a parsing test (protocol drift rule). Follow-up PBIs of #720 build them.
- The sidebar and the TUI show a hibernated agent differently from a running one.
- A resume that fails (session gone, harness changed) leaves a shell at its prompt and a notice; nothing is lost beyond what hibernation already cost.

## Alternatives

- **The app hibernates.** Needs the app running, does not cover the TUI or a closed app. Rejected.
- **Kill the process.** Loses a turn that is not finished if the status is stale. Typing the exit input at a `done` prompt is the harness's own clean exit. Rejected.
- **Resume on any attach, observers included.** A sidebar preview would wake every agent. Rejected.
- **Resume only by an explicit action.** Safer but adds a step the user has to remember; the first controller attach is already an explicit act of the user. Rejected.
- **On by default.** Changes the behavior of every installation without a choice. Rejected.
