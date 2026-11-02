# AGENTS.md

Guidance for humans and coding agents working in this repository. Generated text (docs, issues, commits, code) is written in US English.

## Architecture

- `src-tauri/src/herdr.rs`: socket API client (NDJSON over the Unix socket); `SUPPORTED_PROTOCOL` (the protocol poimen is written for) lives here. Requests use short-lived connections; subscriptions keep theirs.
- `src-tauri/src/watch.rs`: one long-lived `events.subscribe`. `pane.agent_status_changed` needs a `pane_id`, and herdr rejects the whole subscription when a referenced pane is gone, so the watcher resubscribes on pane-set changes.
- `src-tauri/src/settings.rs`: user settings (`~/.config/poimen/config.json`, optional Ghostty config); `get_settings` command plus a polling watcher emitting `settings`. Preferences only, never UI state; frontend side is `core/settings.ts`.
- `src-tauri/src/stream.rs`: live terminals by spawning `herdr terminal session observe|control`. Use this public CLI contract; do not reimplement herdr's internal binary client protocol.
- `src/`: Angular 22 (standalone components, signals) on helix-ui, built with `@angular/build` (`angular.json`), tested with Vitest. `src/app/core/` is the Tauri bridge and state: `tauri.ts` (the only `invoke`/`listen` caller), `herdr.ts` (wire types, pure helpers), `herdr-store.ts` (snapshot signals, debounced re-read), `terminal-stream.ts` (routes `terminal` records by session id). `src/app/terminal-view/` wraps xterm.js for the active editor tab (`core/editor-tabs.ts` holds tabs, preview/pinned and mode; `src/app/ui/helix-candidates/tab-bar/` is the generic tab bar, a helix-ui candidate); `src/app/sidebar/` shows the spaces tree (`hx-tree`; `src-tauri/src/git.rs` answers the read-only `repo_info` command with branch and worktree per directory); `src/app/activity-bar/` is the view rail; `core/ui-state.ts` holds persisted UI state (sidebar width/collapsed), never pane state.

## Rules

- **herdr owns state.** poimen never stores pane state of its own; snapshots from herdr are authoritative and events only invalidate them.
- **Non-destructive.** Closing a view or the app releases controllers and kills only the CLI child processes, never herdr panes. Taking over a controller requires an explicit user action.
- **Tests stay read-only.** Live tests (`#[ignore]`) may list and observe, never control, send input or create panes in the user's session.
- **Protocol drift.** herdr's API protocol is versioned (22 at the time of the spike). Keep all wire shapes in `herdr.rs` and `stream.rs`, each with a parsing test.

## Decisions

- Frontend stack: Angular 22 and helix-ui, and when a component goes to helix-ui ([ADR 0001](docs/adr/0001-frontend-stack.md)).
- Color schemes: bundled and imported schemes, a derived chrome and the `colorSchema` setting ([ADR 0002](docs/adr/0002-color-schemes.md)).

## Verification

```sh
pnpm check && pnpm lint && pnpm format:check && pnpm test && pnpm build
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## CI and releases

- `.github/workflows/ci.yml` checks the frontend and the Tauri crate (with the Linux system libraries) through GravionLabs/ci `@v1`; its always-running `ci` gate job (green when the checks passed or no app files changed) and `pr-checks / PR checks` (`pr-checks.yml`: readiness, Conventional Commit title) are the checks required on `main` and `release/**`.
- Reusable workflows and actions of GravionLabs/ci are referenced at `@v1`, never `@main` or a SHA.
- Work is integrated on `release/next`: every PBI branch is cut from it and its PR targets it (squash); `main` only receives the release PR (merge commit). `release.yml` versions with GitVersion, writes `CHANGELOG.md` with git-cliff and tags `v<version>`. README "Releases" has the rules.
