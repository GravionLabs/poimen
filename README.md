# poimen

A desktop GUI for [herdr](https://herdr.dev): watch and steer your AI coding agent sessions with collapsible panes, menus and a status sidebar.

*Poimen* (ποιμήν) is Greek for shepherd. herdr keeps the herd; poimen is the window you watch it through.

> **Status: spike.** This validates the architecture. Expect rough edges.

## How it works

```
┌──────────────── poimen (Tauri) ────────────────┐
│ sidebar: workspaces / panes / agent status     │ ◄── events.subscribe (herdr.sock)
│ cards: one xterm.js per open pane, collapsible │ ──► workspace/tab/pane.list
└───────────────┬────────────────────────────────┘
                │ spawns per expanded card
                ▼
  herdr terminal session observe|control <pane>   (NDJSON frames on stdout,
                                                   commands on stdin)
                │
                ▼
          herdr server  (persistence, agent detection, worktrees, remote)
```

- **herdr stays the backend.** Closing poimen, or collapsing a card, only closes the view; the pane and its agent keep running. The herdr TUI keeps working next to poimen.
- **Read-only by default.** A card opens as an observer (any number allowed). Switch it to *Input* to become the terminal's controller (one per terminal). If another controller holds it, the card offers *Take over*.
- **Events are invalidation signals.** Every herdr event triggers a debounced re-read of the workspace, tab and pane lists, as the herdr socket API docs recommend. `pane.agent_status_changed` is applied right away.

## Requirements

- herdr 0.9.5 or newer, server running (`herdr` once)
- Rust 1.94+, Node.js 24, pnpm

## Development

```sh
pnpm install
pnpm tauri dev
```

| Check | Command |
|---|---|
| Frontend types | `pnpm check` |
| Frontend tests | `pnpm test` |
| Rust tests | `cd src-tauri && cargo test` |
| Live smoke tests (read-only, need a running herdr) | `cd src-tauri && cargo test -- --ignored` |
| Lint | `cd src-tauri && cargo clippy --all-targets` |

### App icon

The icon source is `src-tauri/icons/source/icon.svg` (1024 x 1024, a shepherd's crook over a terminal prompt). After changing it, regenerate all sizes and drop the mobile outputs:

```sh
pnpm tauri icon src-tauri/icons/source/icon.svg
rm -rf src-tauri/icons/android src-tauri/icons/ios
```

## Keyboard

| Shortcut | Action |
|---|---|
| ⌘⇧E | Expand all cards |
| ⌘⇧C | Collapse all cards |
| ⌘⇧W | Close all views |
| ⌘R | Refresh |

## License

MIT
