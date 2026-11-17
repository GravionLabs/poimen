# ADR 0013: Install poimen-server on a machine from the app after a confirmation

- Status: Proposed
- Date: 2026-11-17
- Issues: #864, Feature #718
- Builds on: [ADR 0008](0008-remote-machines-over-ssh.md), [ADR 0009](0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md)
- Amends: [ADR 0008](0008-remote-machines-over-ssh.md), section "Installing poimen-server on the remote"

## Context

ADR 0008 leaves the installation of `poimen-server` on a remote machine to the user: poimen shows an install command, the user copies it and runs it on the machine. It states "The app does not upload a binary to the remote" and the same holds for an update of an `incompatible` server. In practice this is the step where users stop: they must open a second terminal on the machine, know the release tag and the archive for the architecture, and put the binary on the `PATH` of a non-interactive shell.

Owner decision 2026-11-17: poimen may install the server on a machine after an explicit confirmation. This ADR records how. Everything else of ADR 0008 (transport, credentials, repositories, version check) stays.

What exists:

- The release workflow attaches `poimen-server-<version>-<target>.tar.gz` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (job `server-linux`, built by the same steps as `scripts/prepare-sidecar.sh`). Each archive holds two static binaries: `poimen-server` and `poimen-pty-host` (`-rc` suffixes on the rc channel), which the server needs next to it (ADR 0009). A beta has no release, only workflow artifacts.
- `poimen-server --version` prints the version and the protocol version; the machine check of the app already reads it.
- A machine is reached with `ssh -T -o BatchMode=yes -- <target> poimen-server[-rc] stdio` (`machines.rs`), so the binary must be found on the non-interactive `PATH`.
- Terminals run in their own `poimen-pty-host` and survive a server restart (ADR 0009).

## Decision

poimen installs or replaces `poimen-server` on a Linux machine over the existing SSH connection, only after the user confirms the exact plan in the app. This amends ADR 0008: "the user runs the install command" becomes "poimen runs it after a confirmation"; the command stays visible and can still be copied and run by hand.

### Where the binaries come from

- The release archive of the app's own version and channel: `poimen-server-<version>-<target>.tar.gz` of the GitHub release `v<version>` (on the rc channel the pre-release `v<version>-rc<nnn>`, whose archive carries the `-rc` binaries). The version and channel are those compiled into the app (`CHANNEL`, bundled version), so the installed server always has the protocol the app was built for.
- `<target>` comes from the machine, not from a guess: `uname -sm` over ssh. `Linux x86_64` is `x86_64-unknown-linux-musl`, `Linux aarch64` and `arm64` are `aarch64-unknown-linux-musl`. Any other result (macOS, Windows, another architecture) stops with that text; ADR 0008's scope (Linux only) is unchanged.
- A build without a release asset (a beta, a development build) has nothing to install: the action is not offered and the machine shows why. poimen never builds on the machine and never installs a different version than its own.

### How they get there

- **Primary: copy over the ssh connection.** The app downloads the archive itself (it already reaches GitHub for updates and `gh`), verifies it (below) and streams it to the machine through ssh stdin: `ssh -T -o BatchMode=yes -- <target> 'tar -xzf - -C <dir>'`. The machine needs `tar` and `gzip` and nothing else, no `curl` and no `gh`, and no outbound access to GitHub (machines behind a proxy or without internet work).
- Download on the machine (`curl`/`gh release download`) is not used by the app. The command that ADR 0008 showed stays available as "Copy command" for a user who prefers it.
- poimen uses the same ssh options as for the connection (`BatchMode=yes`, the user's `~/.ssh/config`). A failing ssh is shown with its message; nothing is retried silently.

### Install path

- User directory, no root: `~/.local/bin` on the machine (created when missing). The two binaries are written as temporary files in that directory and moved into place with `mv`, so a server that is running keeps its open file and a failure leaves the old binary.
- poimen-server finds its PTY host next to its own binary (ADR 0009), so both always go to the same directory, in one step.
- The non-interactive `PATH` of ssh often lacks `~/.local/bin`. After the install poimen runs `command -v poimen-server[-rc]` the way the connection does; when it is not found, the connector's command for that machine uses the absolute path (`$HOME/.local/bin/poimen-server[-rc] stdio`) and the confirmation says so. poimen never edits shell profiles or `/etc`.
- No `sudo`, no package manager, no systemd unit. The server is still started by the connection (`stdio`), not as a service.

### Verification

- Before sending: the SHA-256 of the downloaded archive is compared with the published one. The release workflow publishes `poimen-server-<version>-<target>.tar.gz.sha256` next to each archive (a new asset, a follow-up of the implementation PBI; until it exists GitHub's own asset `digest` of the release API is the reference). A mismatch stops before any byte reaches the machine.
- After installing: `poimen-server[-rc] --version` runs on the machine over ssh and must report the app's version and protocol version; `poimen-pty-host[-rc] --version` must report its own. A mismatch is an error: the new files are removed, the old ones stay, and the machine state is `incompatible` or `missing` as before. Only then the machine reconnects.

### What the confirmation shows, and that nothing runs before it

- The dialog (`app-dialog-shell`, the Dialogs rule of `AGENTS.md`) names: the machine and its ssh target, the version and channel to install (and the version found there, for an update), the archive and its origin, the target path on the machine, and the exact commands that will run over ssh (`uname -sm`, `mkdir -p`, `tar -xzf`, `mv`, `--version`), in a copyable block.
- Before the confirmation poimen runs only what is needed to fill the dialog and changes nothing on the machine: the `uname -sm` and `command -v` probes (read-only) and, locally, the download and checksum. No file is written on the machine, and no process is started there other than the probes, until the user presses the confirming button.
- The confirming action is the one primary button ("Install on <machine>"); Cancel is the default focus. Every install or update asks again; no setting remembers an approval, so an approval on one machine or for one version does not extend to another.

### Update of an incompatible or outdated server

- The same flow. A machine in `incompatible` (other protocol) or `missing` offers "Install server…" / "Update server…" with the dialog above; the dialog adds the version found on the machine. poimen sends no other request to an incompatible server (ADR 0008) and none until the install ended.
- poimen never updates a machine unprompted, also not when the app itself was updated.

### Running terminals

- Terminals run in their own `poimen-pty-host` per terminal (ADR 0009) and survive the replacement of the server binary: the old server process (if one runs, started by an earlier connection) keeps its open inode and the hosts keep theirs. The install replaces the files by `mv` and never stops, signals or kills a process.
- A server that is running with the old binary keeps running until its connection ends; the next connection starts the new one, which adopts the listed hosts (ADR 0009, #604). If the new server has another protocol than the old, the adoption rules of ADR 0009 apply; poimen does not close terminals for an install.
- The dialog tells the user when terminals run on the machine ("n terminals keep running; they are not stopped") using the last known snapshot of the machine, when there is one.
- A PTY host of an older version is not replaced by this flow; its binary file is replaced for new terminals only.

### Hooks on the machine

- `poimen-server integration install claude|copilot|gemini` (agent status hooks, edits `~/.claude/settings.json` and similar) stays a separate step with its own confirmation, as in ADR 0006 and the hooks offer of the app. Installing the server does not install hooks and does not touch any agent configuration. After a successful install the app may offer the hooks step for that machine as a second, separately confirmed dialog.

### What stays excluded

- No stored secret: no password, key or token is stored or sent; authentication stays the user's SSH setup. The GitHub download is anonymous (public release assets) or uses the user's own `gh` login on the app machine; no credential goes to the remote machine.
- No root: no `sudo`, nothing outside the user's home, no profile or system file edited.
- No open port: the server is still reached only through `ssh ... stdio`.
- No install without a confirmation, no silent update, no install of a version other than the app's own, no unattended install when a machine is added or reconnects.
- macOS and Windows machines, and a local server, are out of scope (the local server comes with the app).

## Alternatives

- **Keep ADR 0008 (user runs the command).** Safest and simplest, but it is the step users abandon, and a version change after an app update repeats it on every machine. Rejected by the owner decision.
- **Download on the machine (`curl` or `gh release download`).** Needs outbound access to GitHub and a tool on the machine, and the checksum is verified there by a tool poimen does not control. Kept only as the copyable command.
- **Install with a script fetched and piped to a shell (`curl | sh`).** Runs unreviewed text with the user's rights; the exact commands could not be shown. Rejected.
- **Install system-wide (`/usr/local/bin`, a package).** Needs root, which poimen does not use. Rejected.
- **Update automatically with the app.** Changes software on machines nobody asked about. Rejected; updates are confirmed per machine.

## Consequences

- A machine goes from "missing" or "incompatible" to working in one confirmed step from the app, without a second terminal.
- The release workflow gains `.sha256` assets for the server archives; the install is not offered for a build whose release has none and whose asset digest is unavailable.
- The connector may use an absolute path to the binary for a machine whose non-interactive `PATH` lacks `~/.local/bin`.
- Running terminals are untouched by an install; a server replaced under a running connection takes effect at the next connection.
- Implementation follows in later PBIs (probe, download and checksum, copy and verify, the dialog, the release checksum assets); this ADR changes no code. ADR 0008 keeps its status and its other decisions; its "Installing poimen-server on the remote" section is superseded by this ADR once it is accepted.
