# ADR 0008: Remote machines over poimen-server via SSH

- Status: Proposed
- Date: 2026-11-09
- Issues: #277, Feature #11 (reworked to "Remote spaces with tasks"); #37 closed as not planned; #38 replaced by the sidebar PBI
- Builds on: [ADR 0003](0003-own-terminal-backend-without-herdr.md), [ADR 0006](0006-poimen-console-with-herdr-scope.md)

## Context

Remote machines were out of scope in ADR 0003 ("Out of scope": "Remote machines (#11)"). The herdr backend could have reached a remote through herdr itself, but herdr's machine forwarding does not carry interactive terminals (the herdr exit in 0.10.0 removed that path), so it cannot serve the terminals poimen shows.

ADR 0003 gave poimen its own terminal backend, `poimen-server`, with a documented NDJSON protocol over a local Unix socket (Windows: a named pipe). The server owns the PTYs, keeps the terminal grid and answers `ping` with its protocol version. Nothing in that design requires the server to run on the same machine as the app. What is missing is a transport and a rule for what happens on the remote side.

Two requirements shape the choice:

- Credentials for GitHub (`gh`), Claude Code and GitHub Copilot CLI are logged in on the machine where the work runs. Copying them to the app machine would move secrets that poimen does not need to hold.
- The user already reaches remote machines with SSH, with keys and hosts in `~/.ssh/config`. Opening a network port on the remote would add a new attack surface that SSH already covers.

Scope limits: remote platforms are Linux x86_64 and aarch64. macOS as a remote is out of scope for now. Moving a running local space to a remote is out of scope. A remote space is created when a task starts on that machine, so delegation happens at task start.

## Decision

### Transport

- poimen reaches a remote server with `ssh -T <target> poimen-server stdio`. The NDJSON protocol of ADR 0003 runs over the stdin and stdout of that command. `poimen-server stdio` is a new mode of the server binary that serves one client connection on its stdio, with the same requests, events and framing as the local socket.
- No port is opened on the remote. The app never stores a key or a password. Authentication is key-based and comes from `~/.ssh/config` (host, user, identity file, jump host).
- poimen runs ssh with `BatchMode=yes`, so a missing key or an unknown host key fails at once instead of prompting in a process without a terminal. The failure is shown to the user with the ssh message.
- `<target>` is the `Host` alias of `~/.ssh/config` as the user defines it. poimen does not parse ssh configuration.

### Version check and incompatible servers

- On connect, the first request is `ping`. The client compares the protocol version of the answer with the version it was built for, as it does for the local server.
- A server that answers with another protocol version puts the remote into state `incompatible`. poimen sends no further requests to it and shows the version of both sides. Only the install command of this ADR is offered as the fix.
- A remote whose `poimen-server` cannot be run (not installed, not on the non-interactive `PATH`) is shown as missing, with the install command. poimen does not treat it as an error of the connection.
- poimen does not upgrade a remote server by itself. An outdated remote is replaced by the user running the install command again, as with a local server (ADR 0006, Consequences: a restart is a confirmed user action).

### Repositories on the remote

- Repositories live under `reposDir` on the remote. The default is `~/Source/{owner}/{repo}`, with `{owner}` and `{repo}` taken from the GitHub `origin` of the repository, as in the local layout.
- A repository missing on the remote is cloned on demand with `gh repo clone` into `reposDir`. The clone never goes into an existing directory: if the target path exists and is not the clone of that repository, poimen stops and reports the path.
- Worktrees of a task follow `tasks.worktreeDir` relative to that clone, as they do locally (`worktree.rs`). The worktree rules of ADR 0005 and the task flow apply unchanged; only the machine differs.

### Credentials

- Credentials stay on the remote. `gh`, Claude Code and GitHub Copilot CLI are logged in there, once, by the user on the remote.
- poimen forwards no tokens, no `gh` configuration and no agent credentials over the SSH channel, and does not copy them to the app machine.
- poimen checks the logins on the remote (`gh auth status`, and the agent CLIs) and shows the command the user runs to log in when a check fails. It does not run the login itself.

### Installing poimen-server on the remote

- poimen shows the install command for the remote. On Linux it downloads the release asset of the server with `gh release download` and places the binary where the non-interactive `PATH` finds it. The command is shown, copied by the user and run by the user on the remote.
- The app does not upload a binary to the remote. There is no copy of the app's own binary over SSH.
- The command names the release tag of the app, so the installed server matches the protocol of the app. A mismatch is the `incompatible` state above.

## Alternatives

- **herdr `--machine` forwarding.** herdr would own the remote machine and its terminals. It does not carry interactive terminals since the herdr exit in 0.10.0, so it cannot serve the terminals poimen shows. Rejected.
- **TCP with TLS and tokens.** poimen-server listens on a port of the remote, and a token authenticates the app. It needs a listener, certificate handling and token storage on both machines, and it moves the trust decision from SSH keys to a new secret. Rejected in favour of SSH, which the user already runs.
- **SSH socket forwarding (`ssh -L` of the Unix socket).** The local socket of the remote server is forwarded to the app machine. It needs a long-lived tunnel per remote, a socket path on both sides and a reconnect logic of its own. The stdio command gives one connection per use, with the lifetime of the ssh process. Rejected for now.

## Consequences

- No port is opened on any remote, and no secret of the app is stored or sent to a remote. Access follows the SSH setup the user already maintains.
- The protocol of ADR 0003 is the only remote contract. A remote needs `poimen-server stdio` and nothing else from poimen.
- Remote servers are upgraded by the user with the shown command. An outdated or incompatible remote stays visible and gets no requests until it is replaced.
- Logins are set up once per remote by the user. poimen can say what is missing but cannot fix it.
- Only Linux remotes are supported for now. A macOS remote needs its own install command and its own check later, under a new decision.
- A remote space is created by a task start. Converting a running local space into a remote space stays out of scope.
- ADR 0003's "Out of scope" line for remote machines now points here. ADR 0003's status and its other decisions are unchanged.
