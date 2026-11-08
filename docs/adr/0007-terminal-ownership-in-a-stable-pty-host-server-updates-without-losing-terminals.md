# ADR 0007: Terminal ownership in a stable PTY host, server updates without losing terminals

- Status: Accepted
- Date: 2026-11-08
- Builds on: [ADR 0003](0003-own-terminal-backend-without-herdr.md), [ADR 0006](0006-poimen-console-with-herdr-scope.md)

## Context

`poimen-server` owns the PTYs of all terminals and keeps them when the app closes or updates (`docs/updater.md`). It does not keep them when the server itself is replaced: terminals are not persisted (`docs/protocol.md`), a restarted server starts without terminals, and every shell and agent in them ends. Since 0.13.0 the app offers a confirmed restart of an outdated server (#500). Whoever confirms it loses all running sessions, including a deliver loop. So a server release that fixes something forces a choice between running an outdated server and losing work.

A running process cannot be saved and restored. What survives is a process whose PTY master is still held open by someone. That is a question of who holds it:

- Unix: the master is a file descriptor. It can be inherited across `exec` or passed to another process with `SCM_RIGHTS`.
- Windows: ConPTY's `HPCON` is not a kernel handle that `DuplicateHandle` can move to another process. It is bound to its owner, and when the owner ends without a holder the console ends and the attached processes get a close signal. The pipes can be duplicated, the console cannot.

Windows therefore needs a process that owns the ConPTY for its whole life, and it should not be the process that is updated often. The per-version copies (`%LOCALAPPDATA%\poimen-server\bin\<version>\`, the Linux AppImage staging) already let an old process run while the installed file is replaced.

## Decision

Terminal ownership moves out of `poimen-server` into a small, stable process, `poimen-pty-host`. The server becomes a client of the hosts. The design is the same on all platforms.

- **One host per terminal.** A host creates the PTY (`portable-pty`, ConPTY on Windows through the vendored copy), spawns the child, and owns both until the child exits. It does nothing else: no spaces, no agent status, no terminal emulation, no tokens.
- **Host protocol.** A local endpoint per host (Unix socket, Windows named pipe, owner-only like the server's) with a versioned, minimal protocol: `hello` (host protocol version, child pid), `input`, `resize`, `output` stream with sequence numbers, `exit` (code), `kill`. The host keeps a bounded ring of recent output, so a reconnecting server can catch up. The host protocol changes only in a backwards compatible way, and a new server must speak every host version it can meet.
- **The server is a client.** It keeps the emulator grid, the controller rules, agent status, tokens and the `terminal.attach` protocol for the app and console as today (`docs/protocol.md` is unchanged for clients). It reads the host's output, feeds the grid and answers terminal queries as it does now.
- **Server update.** The old server writes the list of its terminals (id, space, name, host endpoint, size, controller-relevant state) to `server.json` next to the spaces, and ends. The hosts and their children keep running. The new server starts, reads the list, reconnects to every host and rebuilds each grid from the host's ring. Clients reattach through `terminal.attach` and their reconnect handling. A host whose endpoint no longer answers is dropped from the list.
- **Crash of the server** is handled the same way: the hosts survive, the next server adopts them.
- **Host lifetime.** A host ends with its child (and a `kill` from the server). An orphaned host with no server for a long, configurable time ends its child and itself. `poimen-pty-host` ships next to `poimen-server` and runs from a per-version copy where an installed binary cannot be replaced (Windows, AppImage), as the server does now. Copies of other versions are removed when no host runs from them.
- **Migration.** A server from before this ADR owns its terminals itself. The first update to the version with hosts still ends them, once. From then on updates keep terminals. The new server starts new terminals in hosts only.
- **Scope.** The host process is not an update target in the normal flow. A fix to it applies to terminals started after the update. The app shows which hosts run an older host version, with a per-terminal restart the user chooses.

## Consequences

- Updating or crashing the server no longer ends terminals, on Windows, macOS and Linux with one mechanism. The confirmed restart (#500) becomes harmless instead of destructive.
- No platform-specific descriptor passing and no `exec` trick; the Windows ConPTY limit is respected.
- More processes (one small host per terminal) and a second protocol with a compatibility duty. The host must stay small and conservative, since it is the part that is not updated with the server.
- Output now crosses one more local hop. The host keeps only a bounded ring, so a server that is down for long loses what scrolled past it; the grid and scrollback of the server are rebuilt from the ring, not from full history.
- Terminal state that the server does not persist today (agent status, tokens) must be saved with the list or re-reported by the hooks after the adoption. Stale `seq` rules apply as now.
- Tests need a fake host and an adoption test per platform; tests never start or stop a real session.
- The first update to this version ends terminals, once.

## Alternatives

- **`exec` of the new binary in the same process (Unix only).** Keeps pid, parent and descriptors, with the state serialized across the `exec`. Simple on Unix, but there is no equivalent on Windows, so it would need a second mechanism there. Rejected for that.
- **Pass the descriptors to a new process with `SCM_RIGHTS`.** Works on Unix, orphans the children and loses exit codes, and does not exist for ConPTY. Rejected.
- **Rolling update with two servers side by side.** The new server runs next to the old one, new terminals go to the new one, old ones stay until they close. No handover and no new process type, but the client must talk to several servers, which touches the `Backend` trait and the app, and old servers stay around indefinitely. Kept as a fallback, not chosen.
- **Save and restore terminals.** Only layout, names, cwd and scrollback can be restored, never the running process. This would not meet the goal.
- **One host for all terminals.** Fewer processes, but one host crash ends every terminal and the host cannot be restarted per terminal. Rejected.
