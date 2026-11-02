# poimen

A desktop GUI for [herdr](https://herdr.dev): watch and steer your AI coding agent sessions with editor tabs, menus and a status sidebar.

*Poimen* (ποιμήν) is Greek for shepherd. herdr keeps the herd; poimen is the window you watch it through.

> **Status: spike.** This validates the architecture. Expect rough edges.

## How it works

```
┌──────────────── poimen (Tauri) ────────────────┐
│ sidebar: workspaces / panes / agent status     │ ◄── events.subscribe (herdr.sock)
│ tabs: one xterm.js for the active tab          │ ──► workspace/tab/pane.list
└───────────────┬────────────────────────────────┘
                │ spawns per visible tab
                ▼
  herdr terminal session observe|control <pane>   (NDJSON frames on stdout,
                                                   commands on stdin)
                │
                ▼
          herdr server  (persistence, agent detection, worktrees, remote)
```

- **herdr stays the backend.** Closing poimen, or closing a tab, only closes the view; the pane and its agent keep running. The herdr TUI keeps working next to poimen.
- **Read-only by default.** A pane opens as an observer (any number allowed). The switch in the tab bar (the tab shows `RO` or `IN`) makes you the terminal's controller (one per terminal). If another controller holds it, the view offers *Take over*.
- **Editor tabs.** Opening a pane adds a tab to one tab bar above the terminal: a click opens a preview tab (italic, replaced by the next preview), a double click pins it, a second click on an open pane activates its tab. Tabs close with the x or a middle click and reorder by drag. Only the active tab keeps a stream open; hidden tabs close theirs, the pane keeps running.
- **Angular frontend.** The UI is Angular 22 with helix-ui (light and dark follow the system); `src/app/core` is the bridge to the Tauri commands and events, `src-tauri` stays a thin shell ([ADR 0001](docs/adr/0001-frontend-stack.md)).
- **Spaces tree.** The sidebar is a helix `hx-tree` of workspaces and their panes (the herdr tab is secondary text). A badge tells agents (by kind, e.g. `cl` for claude) from plain terminals (`>_`); each row shows the status dot, git branch (`wt` for a linked worktree), directory (`foreground_cwd`) and the status message, state label or integration tokens when present. The filter field matches name, agent, branch, tab, directory and text; arrow keys, Home/End and type-ahead navigate, Enter opens. Branch and worktree come from a read-only `git rev-parse` in the pane directory (herdr's pane list has none).
- **Layout.** An activity bar (Spaces) sits left of a sidebar you resize by dragging its edge (arrow keys work too) and toggle with `Cmd+B` (`Ctrl+B` elsewhere). Width and collapsed state persist in poimen's own UI state (webview local storage, not the user settings file); the layout works from 720 px window width.
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
| Frontend types (incl. templates) | `pnpm check` |
| Frontend lint | `pnpm lint` |
| Frontend format | `pnpm format:check` |
| Frontend tests | `pnpm test` |
| Frontend build | `pnpm build` |
| Rust tests | `cd src-tauri && cargo test` |
| Live smoke tests (read-only, need a running herdr) | `cd src-tauri && cargo test -- --ignored` |
| Lint | `cd src-tauri && cargo clippy --all-targets -- -D warnings` |

CI (`.github/workflows/ci.yml`, built on GravionLabs/ci) runs the frontend checks and `cargo fmt`, `clippy` and `test` of `src-tauri` on every pull request; its gate job `ci` and the `pr-checks / PR checks` job are the required checks. The Rust toolchain is pinned in `rust-toolchain.toml`, Node.js in `.nvmrc`.

### Editors

- **VS Code** (`.vscode/`, install the recommended extensions): the launch configuration "Tauri Dev" runs `pnpm tauri dev`; "Debug Rust" starts the background task `ui:dev` (`ng serve` on port 1420), waits until it is ready and debugs `src-tauri/target/debug/poimen` under CodeLLDB; "Debug Rust tests" debugs the `src-tauri` unit tests. The tasks `check`, `test` and `cargo test` run the checks.
- **Zed** (`.zed/`): `task: spawn` offers `pnpm tauri dev`, `ng serve`, `pnpm test` and `cargo test`; `debugger: start` offers "Debug Rust" under CodeLLDB. Start the `ng serve` task first, Zed has no background pre-launch task.
- **Frontend**: a debug build loads the UI from `http://localhost:1420`; debug it in the webview DevTools (right click > Inspect, or `Cmd+Option+I` / `Ctrl+Shift+I`).

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

## Releases

- Work branches are cut from `release/next` and merge into it; the `Release branch` workflow keeps the release PR `release/next` → `main` open and current (`github-release` skill).
- Work PRs into `release/**` are **squash** merged; their PR title must be a Conventional Commit (`pr-checks` title job) and becomes the one commit of the change.
- A release is made by merging that release PR into `main` with a **merge commit, never squash** (the `main` ruleset allows only merge commits), so GitVersion sees every commit. A hotfix PR into `main` is merged the same way, and every commit of it must be a Conventional Commit.
- The version comes from Conventional Commits (`versioning/determine-version` of GravionLabs/ci, `GitVersion.yml`): `fix:`/`perf:`/`revert:` → patch, `feat:` → minor, `type!:` or `BREAKING CHANGE:` → major. A release with only fixes is a patch, one with a `feat:` a minor.
- While the version is 0.x, use **no** `!` and no `BREAKING CHANGE:` until 1.0 is intended: either one jumps the version to 1.0.0.
- On the push to `main`, `.github/workflows/release.yml` runs the CI checks, determines the version, commits `CHANGELOG.md` (git-cliff, `cliff.toml`) to `main`, tags `v<version>`, creates the GitHub Release and closes the issues the release delivered. The changelog commit is pushed with the `GH_PAT` secret (a repository admin, bypass actor of the `main` ruleset).
- The versions in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` are placeholders; the tag carries the released version.
- Tags are `v<major>.<minor>.<patch>`, as in the other GravionLabs repositories; milestones carry the same names.
- The rulesets `main` and `release` are created by `scripts/setup-rulesets.sh`.

## License

MIT
