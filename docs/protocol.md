# poimen-server protocol

Protocol version: **2** (version 1 had no terminal output; version 2 adds `terminal.attach`, the stream records and the controller rules for `terminal.input` and `terminal.resize`).

`poimen-server` is a headless daemon that owns the PTYs, so shells and agents keep running when the app closes. The protocol is client-neutral: no Tauri or app concept appears in it. The wire types live in `src-tauri/crates/poimen-protocol`, a client in `src-tauri/crates/poimen-client`.

## Transport

- NDJSON over a Unix socket: one JSON object per line, UTF-8, `\n` terminated.
- Socket path: `$POIMEN_SOCKET`, else `$XDG_RUNTIME_DIR/poimen/server.sock`, else `<temp dir>/poimen-<user>/server.sock`. The directory has mode 0700, the socket 0600. One server per user: a second server on a live socket exits; a stale socket file is replaced.
- Requests use short-lived connections (connect, one or more requests, close). A connection that sent `events.subscribe` only receives events afterwards.
- A line that is not a request gets an error response and the connection stays open.

## Messages

Request: `{"id": "1", "method": "space.create", "params": {...}}`. `id` is any string chosen by the client; `params` may be omitted when a method has no required parameter.

Response: `{"id": "1", "result": {...}}` or `{"id": "1", "error": {"code": "not_found", "message": "..."}}`. `id` is `null` in an error for a line that could not be parsed.

Event: `{"event": "terminal.exited", "data": {...}}`. Lines with an `event` field are events; all others are responses.

Error codes: `parse_error`, `invalid_request`, `unknown_method`, `invalid_params`, `not_found`, `exited` (the terminal's process has exited), `spawn_failed`, `not_controller`, `conflict`, `internal`.

## Objects

Space: `{"id": "s1", "name": "api", "cwd": "/home/me/api"}`.

Terminal: `{"id": "t2", "space_id": "s1", "name": "zsh", "cwd": "...", "cols": 80, "rows": 24, "status": "running" | "exited", "exit_code": 0}` (`exit_code` only when exited) `agent` (`{"status": "idle" | "working" | "blocked" | "done", "agent": "claude", "session_id": "..."}`; `agent` and `session_id` optional) is present while hooks report an agent in the terminal, and cleared when it ends or the process exits.

The server stores no layout: groups, splits and tabs are the client's business. Terminal ids are unique per server run; ids of spaces survive restarts.

## Requests

| Method            | Params                                                                  | Result                | Notes                                                                                           |
| ----------------- | ----------------------------------------------------------------------- | --------------------- | ----------------------------------------------------------------------------------------------- |
| `ping`            | none                                                                    | `{version, protocol, pid}` | `protocol` is the protocol version (2).                                                    |
| `events.subscribe`| none                                                                    | `{subscribed: true}`  | The connection then carries events only. List after subscribing to avoid missing changes.       |
| `space.list`      | none                                                                    | `{spaces: [Space]}`   |                                                                                                 |
| `space.create`    | `name`, `cwd`                                                           | `{space}`             | Non-empty `name` and `cwd`. Persisted.                                                          |
| `space.rename`    | `space_id`, `name`                                                      | `{space}`             | Persisted.                                                                                      |
| `space.close`     | `space_id`                                                              | `{}`                  | Closes its terminals first (`terminal.closed` each). Persisted.                                 |
| `terminal.list`   | `space_id?`                                                             | `{terminals: [Terminal]}` | Exited terminals stay listed until closed.                                                  |
| `terminal.create` | `space_id`, `cwd?`, `name?`, `command?` (argv), `cols?`, `rows?`        | `{terminal}`          | `cwd` defaults to the space's, `command` to `$SHELL` (else `/bin/sh`), size to 80x24. Never through a shell. |
| `server.set_colors` | `foreground`, `background`, `cursor?`, `palette?` (16)                | `{}`                  | `#rrggbb` colors the server answers color queries with while no controller is attached; see Terminal queries. `cursor` defaults to `foreground`, an absent `palette` keeps the previous one. |
| `terminal.rename` | `terminal_id`, `name`                                                   | `{terminal}`          |                                                                                                 |
| `terminal.close`  | `terminal_id`                                                           | `{}`                  | Kills the process (SIGHUP) and forgets the terminal.                                            |
| `terminal.input`  | `terminal_id`, `data`                                                   | `{}`                  | `data` is written as is; `exited` error when the process has ended. An observer, or a controller that was taken over, gets `not_controller`. |
| `terminal.resize` | `terminal_id`, `cols`, `rows`                                           | `{}`                  | Both at least 1. Resizes the PTY and the grid, and is the only way to set the size. `not_controller` from an observer, and from a connection that is not attached while a controller exists (without one, any connection may resize). |
| `terminal.attach` | `terminal_id`, `mode` (`observe` or `control`), `take_over?` (default false) | `AttachResult`   | See Attach. The connection becomes a stream connection. `not_found`, `conflict`. |
| `terminal.report_agent` | `terminal_id`, `seq`, `status` (`idle`, `working`, `blocked`, `done`, `gone`), `agent?`, `session_id?` | `{accepted}` | See Agent status. `not_found` for an unknown terminal. |

Terminals get `POIMEN_SOCKET` (the server's socket) and `POIMEN_TERMINAL_ID` in their environment, and `TERM=xterm-256color` when the server has none.

## Attach

The server keeps a headless terminal (an `alacritty_terminal` grid with a scrollback of `POIMEN_SCROLLBACK` lines, default 10000) per terminal, fed from the PTY, so a client can attach at any time and see the current state. Live output is not rendered by the server: it is passed on as raw bytes, the grid is only read to build the snapshot.

`terminal.attach` is sent on a connection of its own (not a short-lived one). Its result, `AttachResult`:

`{"terminal_id": "t2", "seq": 17, "cols": 80, "rows": 24, "mode": "observe", "snapshot": "<base64>"}`

`snapshot` is the base64 of a byte stream. Written to a fresh emulator of `cols` x `rows` (xterm.js) it restores the screen and scrollback, the cursor (position, shape, visibility), the SGR pen, the scroll region, the title and the modes: alternate screen (the primary screen is restored behind it), mouse tracking 1000/1002/1003 with 1005/1006, bracketed paste, application cursor and keypad, focus reporting, autowrap, origin, insert and line-feed modes. Not restored: the kitty keyboard protocol, charsets, a pending wrap at the last column, hyperlinks, and a synchronized update (`?2026`) that is still open.

From then on the connection carries stream records (the events below) and still accepts requests (`terminal.input`, `terminal.resize`). Every `terminal.output` and `terminal.resized` record has a `seq`; the first one after the result is `seq + 1` and they are contiguous. The snapshot is built and the client registered under one lock per terminal, so no byte is lost or repeated between the snapshot and the stream.

- Roles: any number of observers and at most one controller per terminal. `control` while a controller exists fails with `conflict` unless `take_over` is true; then the previous controller receives `terminal.detached` with reason `taken_over`. Observers receive output and resize records, and cannot send input or resize (`not_controller`). The size is the controller's: observers render at the size of the last `terminal.resized` (or `cols`/`rows` of the result).
- Closing the connection detaches. Attach and detach of the controller are published to subscribers as `terminal.controller_attached` and `terminal.controller_detached`, so the server (and tools) know when nobody answers the terminal's queries.
- Backpressure: output is coalesced (what arrives within 4 ms, at most 32 KiB per record). Each client has a bounded queue (128 records); the PTY is never blocked by a client. A client whose queue is full is detached with `terminal.detached` reason `overflow` (a client that does not read at all is cut off after a 5 s write timeout) and must attach again for a fresh snapshot.
- `terminal.detached` (the server ended the attachment: `overflow`, `taken_over`, `closed`) and `terminal.exited` (the process ended, with its exit code; the stream ends after it) are distinct. Attaching to an exited terminal returns the final snapshot, then `terminal.exited`.

## Terminal queries

Programs ask the terminal for its colors (OSC 10, 11, 12 and 4 with `?`), the cursor position (DSR `CSI 6 n`) and its attributes (DA1 `CSI c`, DA2 `CSI > c`). The headless grid of the terminal (`alacritty_terminal`) produces the answers and the server writes them to the PTY, but only while the terminal has no controller. With a controller attached, its xterm.js answers the same bytes (#96) and the server stays silent, so a query never gets two answers; the hand-over is at attach and detach of the controller (observers do not count). What the grid produced while a controller was attached is dropped, never answered late. Colors come from `server.set_colors` (a dark default until the app has sent one); a color the program set itself with OSC 4, 10 or 11 wins. The app sends the active scheme on start and on every scheme change. Adding the request is backwards compatible, so the protocol version stays 2.

## Agent status

Agent status comes from hooks only; the server never reads the screen. A reporter (the Claude Code hook) sends `terminal.report_agent` with a `seq` that increases with every report (the hook uses nanoseconds since the epoch). A report whose `seq` is not above the last accepted one of that terminal is stale: it is dropped and the result is `{"accepted": false}`. `status` `gone` means the agent ended and clears the terminal's `agent`. `agent.status_changed` is published only when the state of the terminal actually changed. Adding the request, the `agent` field of Terminal and `session_id` of the event is backwards compatible, so the protocol version stays 2.

On the first start with the poimen backend the app offers to run `integration install` for the agents whose hooks are missing (`status` decides); declining is remembered in the UI state. Claude Code: `poimen-server integration install|uninstall|status claude [--settings <path>]` registers `poimen-server hook claude` in `~/.claude/settings.json` (entries marked with `# poimen-agent-hook`, next to existing ones such as herdr's; uninstall removes only these). The hook does nothing when `POIMEN_TERMINAL_ID` is unset, gives up after 1.5 s and always exits 0. Mapping: `SessionStart` idle, `Stop` done (the turn ended; the status stays done until the next prompt, poimen clears the unread mark when the tab is viewed), `UserPromptSubmit` and `PostToolUse` working, `Notification` of type `permission_prompt` blocked, `SessionEnd` gone; events with an `agent_id` (subagents) are ignored.

Copilot CLI: `poimen-server integration install|uninstall|status copilot [--settings <path>]` writes `~/.copilot/hooks/poimen-agent-state.json` (a file of our own, marked with `# poimen-agent-hook`; an existing file without the marker is never touched), registering `poimen-server hook copilot <event>`. Same hook behavior as for Claude Code. Mapping: `sessionStart` idle, `agentStop` done (same as `Stop`), `userPromptSubmitted` and `postToolUse` working, `notification` of type `permission_prompt` blocked (reported by Copilot CLI 1.0.71), `sessionEnd` gone; subagent events are ignored. Both payload styles are accepted: camelCase (`sessionId`, `notificationType`, event from the command line) and PascalCase names with snake_case fields (`hook_event_name`, `session_id`, `notification_type`).

Starting an agent and prompting it need no request of their own (`poimen_client::agent`): `start_agent` checks that the terminal has no agent, types the command line (words single-quoted for the shell) with Enter through `terminal.input` and resolves when the terminal's `agent` appears, i.e. when the `SessionStart` report arrives (30 s, then a timeout error that points to the hooks). `prompt_agent` types the text with Enter, unless the agent is `working` or `blocked` (nothing is typed), and waits up to 10 s for it to report `working` or `blocked`. The `Backend` methods `agent_start`, `agent_prompt` and `agent_rename` of the poimen backend use them; the agent name is the name of its terminal (`terminal.rename`).

## Events

| Event                  | `data`                                       | When                                                  |
| ---------------------- | -------------------------------------------- | ----------------------------------------------------- |
| `space.created`        | `{space}`                                    | after `space.create`                                  |
| `space.renamed`        | `{space}`                                    | after `space.rename`                                  |
| `space.closed`         | `{space_id}`                                 | after `space.close`                                   |
| `terminal.created`     | `{terminal}`                                 | after `terminal.create`                               |
| `terminal.renamed`     | `{terminal}`                                 | after `terminal.rename`                               |
| `terminal.closed`      | `{terminal_id, space_id}`                    | after `terminal.close` or `space.close`               |
| `terminal.exited`      | `{terminal_id, exit_code}`                   | the process ended on its own (not for closed ones); also the last stream record |
| `terminal.output`      | `{terminal_id, seq, data}`                   | stream record: raw PTY bytes, base64 in `data`        |
| `terminal.resized`     | `{terminal_id, seq, cols, rows}`             | stream record: the controller resized the terminal    |
| `terminal.detached`    | `{terminal_id, reason}`                      | stream record: `overflow`, `taken_over` or `closed`   |
| `terminal.controller_attached` | `{terminal_id}`                      | subscribers: a controller attached                    |
| `terminal.controller_detached` | `{terminal_id}`                      | subscribers: the controller detached or was dropped   |
| `agent.status_changed` | `{terminal_id, status, agent?, session_id?}` | the agent state of a terminal changed (after `terminal.report_agent`); `status` is `idle`, `working`, `blocked`, `done` or `gone`. |

## Persistence

The space list (id, name, cwd) is stored in `$POIMEN_STATE_DIR/server.json`, else `$XDG_STATE_HOME/poimen/server.json`, else `~/.local/state/poimen/server.json`, and read at start. Terminals are not persisted: a restarted server starts without terminals.

## Lifecycle

The app starts `poimen-server` detached when no server answers and never stops it. It looks for the binary next to its own executable first, then on the login-shell `PATH`. Releases ship it as a sidecar (`bundle.externalBin` in `tauri.bundle.conf.json`, built by `scripts/prepare-sidecar.sh` in `release.yml`); a plain `pnpm tauri build` does not, so build it with `cargo build --release -p poimen-server` and put it beside the app. The socket directory must be a real directory owned by the user with mode 0700: the server tightens one it owns and refuses anything else, and the client refuses to connect otherwise (so a `POIMEN_SOCKET` override must point into a private directory). A `server.lock` next to the socket keeps two servers from both judging a socket stale. Compatibility: a client compares `ping.protocol` with the version it was written for.
