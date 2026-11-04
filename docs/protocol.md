# poimen-server protocol

Protocol version: **1**.

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

Error codes: `parse_error`, `invalid_request`, `unknown_method`, `invalid_params`, `not_found`, `exited` (the terminal's process has exited), `spawn_failed`, `internal`.

## Objects

Space: `{"id": "s1", "name": "api", "cwd": "/home/me/api"}`.

Terminal: `{"id": "t2", "space_id": "s1", "name": "zsh", "cwd": "...", "cols": 80, "rows": 24, "status": "running" | "exited", "exit_code": 0}` (`exit_code` only when exited).

The server stores no layout: groups, splits and tabs are the client's business. Terminal ids are unique per server run; ids of spaces survive restarts.

## Requests

| Method            | Params                                                                  | Result                | Notes                                                                                           |
| ----------------- | ----------------------------------------------------------------------- | --------------------- | ----------------------------------------------------------------------------------------------- |
| `ping`            | none                                                                    | `{version, protocol, pid}` | `protocol` is the protocol version (1).                                                    |
| `events.subscribe`| none                                                                    | `{subscribed: true}`  | The connection then carries events only. List after subscribing to avoid missing changes.       |
| `space.list`      | none                                                                    | `{spaces: [Space]}`   |                                                                                                 |
| `space.create`    | `name`, `cwd`                                                           | `{space}`             | Non-empty `name` and `cwd`. Persisted.                                                          |
| `space.rename`    | `space_id`, `name`                                                      | `{space}`             | Persisted.                                                                                      |
| `space.close`     | `space_id`                                                              | `{}`                  | Closes its terminals first (`terminal.closed` each). Persisted.                                 |
| `terminal.list`   | `space_id?`                                                             | `{terminals: [Terminal]}` | Exited terminals stay listed until closed.                                                  |
| `terminal.create` | `space_id`, `cwd?`, `name?`, `command?` (argv), `cols?`, `rows?`        | `{terminal}`          | `cwd` defaults to the space's, `command` to `$SHELL` (else `/bin/sh`), size to 80x24. Never through a shell. |
| `terminal.rename` | `terminal_id`, `name`                                                   | `{terminal}`          |                                                                                                 |
| `terminal.close`  | `terminal_id`                                                           | `{}`                  | Kills the process (SIGHUP) and forgets the terminal.                                            |
| `terminal.input`  | `terminal_id`, `data`                                                   | `{}`                  | `data` is written as is; `exited` error when the process has ended.                             |
| `terminal.resize` | `terminal_id`, `cols`, `rows`                                           | `{}`                  | Both at least 1.                                                                                |

Terminals get `POIMEN_SOCKET` (the server's socket) and `POIMEN_TERMINAL_ID` in their environment, and `TERM=xterm-256color` when the server has none.

Terminal output and the screen snapshot are not part of version 1 (they follow in the next protocol version); the server drains and discards the output.

## Events

| Event                  | `data`                                       | When                                                  |
| ---------------------- | -------------------------------------------- | ----------------------------------------------------- |
| `space.created`        | `{space}`                                    | after `space.create`                                  |
| `space.renamed`        | `{space}`                                    | after `space.rename`                                  |
| `space.closed`         | `{space_id}`                                 | after `space.close`                                   |
| `terminal.created`     | `{terminal}`                                 | after `terminal.create`                               |
| `terminal.renamed`     | `{terminal}`                                 | after `terminal.rename`                               |
| `terminal.closed`      | `{terminal_id, space_id}`                    | after `terminal.close` or `space.close`               |
| `terminal.exited`      | `{terminal_id, exit_code}`                   | the process ended on its own (not for closed ones)    |
| `agent.status_changed` | `{terminal_id, status, agent?}`              | an agent in a terminal changed state; `status` is `idle`, `working`, `blocked` or `done`. Defined now; reported by hooks in a later release. |

## Persistence

The space list (id, name, cwd) is stored in `$POIMEN_STATE_DIR/server.json`, else `$XDG_STATE_HOME/poimen/server.json`, else `~/.local/state/poimen/server.json`, and read at start. Terminals are not persisted: a restarted server starts without terminals.

## Lifecycle

The app starts `poimen-server` detached when no server answers and never stops it. Compatibility: a client compares `ping.protocol` with the version it was written for.
