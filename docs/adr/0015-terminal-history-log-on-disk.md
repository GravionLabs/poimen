# ADR 0015: Terminal history log on disk

- Status: Proposed
- Date: 2026-11-22
- Issues: #973, Feature #950; follow-ups #1194, #1195, #1196
- Builds on: [ADR 0009](0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md), [ADR 0014](0014-hibernate-idle-agents.md)

## Context

What a terminal printed lives in the PTY host's 1 MiB ring and in the server's `alacritty_terminal` grid. Both are gone after a reboot, so the history of a finished agent session cannot be looked at again. A log on disk keeps it. It also means terminal output, which may hold secrets, is written to disk, so the privacy rule has to be recorded before code exists.

## Decision

`poimen-server` appends what each terminal prints, and its resizes, to a log file per terminal.

### Who writes

The server, not the host. The host stays small and is not updated with the server (ADR 0009). Output the host holds while the server is down, beyond its ring, stays a `gap` and is not logged. The replay of the ring when the server adopts a host is logged only for the records the log does not hold (#1194).

### What is stored

Raw PTY bytes as the host delivered them, and resize records (`cols`, `rows`). Not stored: input, the grid, agent status. Checkpoints (#1194, below) are in the same file.

### Format

`<history dir>/<terminal id>`: the 8 bytes `PMNHIST1`, then records `len: u32 LE | crc32: u32 LE | payload`. The payload is a kind byte (`1` output, `2` resize) and the body; the checksum covers the payload. A reader returns the records in order, ignores a torn last record and stops at the first damaged one, so a crash or a bit flip loses only what follows. It never stops the server.

### Where and how much

The `Config.history_dir`, by default `history/` next to `server.json`. Per terminal 8 MiB: the oldest records are dropped (the file is rewritten without them, down to 7/8 of the cap, so the rewrite does not happen on every write). In total 256 MiB: the oldest records of the largest log are dropped first. Both limits are constants for now; they become settings in #1196. The log of an ended terminal is kept until the user closes the terminal (#1196).

### Privacy

Terminal output is on disk, without redaction. The files are private (mode 0600, the directory 0700; on Windows the directory has an owner-only ACL). On by default; the setting that turns it off is #1196. Compression and redaction are not done.

### Not blocking the PTY

The output path only queues the record in a bounded channel for a writer thread; a full queue drops the record and logs it once. A write error (disk full, permission) is logged to `server.log` once per terminal and the terminal goes on.

### Checkpoints and the rebuild (#1194)

A checkpoint record (kind `3`) holds the serialized grid (screen, scrollback, modes, title; the bytes of `Emulator::snapshot`), its size and the host `seq` it covers; the records before it in the file are what it covers. It is written every 1 MiB of output, on a clean shutdown, and as the empty first record of a new log. Kind `4` records the host `seq` after an output record. A rebuild starts from the newest checkpoint that restores (an invalid one falls back to the one before, else to the start) and replays the records after it. Checkpoints count toward the caps; eviction cuts at a checkpoint, so a log never loses its starting point. After `adopt_terminals` the grid is rebuilt from the log and the host ring is read only after the `seq` the log covers: no `gap` marker when the ring has everything after it or the host's `gap` ends before it, a marker for the records neither holds. A log that does not belong to the host (no `seq`, or beyond the host's) is replaced. A damaged checkpoint or log is skipped; it never stops the server.

## Left to the sibling PBIs

- #1195: the read-only terminal after a reboot.
- #1196: the setting, the limits as settings, removal on close.

## Consequences

- Secrets that a program prints are readable by anyone who can read the user's files; the permissions are the only protection.
- A few percent of CPU for the copy and the checksum on heavy output; the disk write is off the PTY path.
