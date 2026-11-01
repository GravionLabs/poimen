# AGENTS.md

Guidance for humans and coding agents working in this repository. Generated text (docs, issues, commits, code) is written in US English.

## Architecture

- `src-tauri/src/herdr.rs`: socket API client (NDJSON over the Unix socket). Requests use short-lived connections; subscriptions keep theirs.
- `src-tauri/src/watch.rs`: one long-lived `events.subscribe`. `pane.agent_status_changed` needs a `pane_id`, and herdr rejects the whole subscription when a referenced pane is gone, so the watcher resubscribes on pane-set changes.
- `src-tauri/src/stream.rs`: live terminals by spawning `herdr terminal session observe|control`. Use this public CLI contract; do not reimplement herdr's internal binary client protocol.
- `src/`: vanilla TypeScript + xterm.js. `model.ts` is pure and tested; `terminal-view.ts` is one collapsible card.

## Rules

- **herdr owns state.** poimen never stores pane state of its own; snapshots from herdr are authoritative and events only invalidate them.
- **Non-destructive.** Closing a view or the app releases controllers and kills only the CLI child processes, never herdr panes. Taking over a controller requires an explicit user action.
- **Tests stay read-only.** Live tests (`#[ignore]`) may list and observe, never control, send input or create panes in the user's session.
- **Protocol drift.** herdr's API protocol is versioned (22 at the time of the spike). Keep all wire shapes in `herdr.rs` and `stream.rs`, each with a parsing test.

## Verification

```sh
pnpm check && pnpm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test
```
