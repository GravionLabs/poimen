# Remote machines

poimen can show spaces and agents of another computer next to your own. It reaches the machine over **ssh**; no port is opened and poimen stores no password or key.

<Screenshot src="machines-dark.png" light="machines-light.png" alt="Settings, Machines: one connected machine with its checks and one that is offline with the command to try" caption="Settings > Backend > Machines: a connected machine with its check and an offline one (demo data)." />

## Add a machine

In your config, `machines` lists them:

```json
{ "machines": [{ "name": "build-box", "ssh": "me@build-box" }] }
```

`ssh` is a target as in `~/.ssh/config` (a `Host` alias or `user@host`). The settings suggest the hosts of your ssh config. `reposDir` says where repositories live on the machine (`~/Source/{owner}/{repo}` by default).

poimen keeps one connection per machine with `ssh -T -o BatchMode=yes <target> poimen-server stdio`, so the machine needs `poimen-server` and key-based login. The sidebar shows a machine as *connecting*, *connected*, *offline* (with the ssh error) or *incompatible* (another protocol version). A dropped connection is retried with a growing delay, or at once from *Reconnect*.

## Install or update the server

If a machine has no server or an old one, the machines page offers **Install** or **Update**. It shows what it will do first and runs nothing before you confirm: it downloads the release archive of the app's own version, checks its checksum, copies the server into `~/.local/bin` over the ssh connection and verifies its version. A failed step removes its staging and keeps the old files. Hooks are a separate step.

## What works

The spaces of a machine appear in the sidebar. Their agents count in attention, the dock badge, notifications and sounds. You can rename, close, split, start agents and send prompts on them, run **Start task** there, and clean up merged tasks. Runs of a machine show in the Runs view. When the machine is offline its last state stays visible, dimmed, and actions say why they are disabled. A drop closes poimen's connections only, never a remote terminal.
