# AGENTS.md

Guidance for humans and coding agents working in this repository. Generated text (docs, issues, commits, code) is written in US English.

## What this is

poimen coordinates AI coding agent sessions: a Tauri + Angular app (`src/`, `src-tauri/`) and a console client (`poimen`), both clients of `poimen-server`, which owns terminals, spaces and tabs. Crates: `src-tauri/crates/`; site: `site/`; decisions: `docs/adr/`.

## Areas

How each area works is in `docs/agents/`. Read the file before you touch the area. **Notes on a change go to the area file, not to this one**: this file stays under 12 KB (a ratchet checks).

- `backend-and-server.md`: before you touch `poimen-server`, `poimen-client`, `poimen-protocol`, `poimen-pty-host`, `poimen-core` or the `Backend` trait.
- `console.md`: the TUI, the CLI, `poimen attach`, `poimen mcp`.
- `spaces-and-layouts.md`: space creation, layouts, templates, the project file, worktrees, trust.
- `settings.md`: user settings, import/export, color schemes, sounds.
- `remote-machines.md`: machines, SSH, actions on remote spaces.
- `agents-and-hooks.md`: agent hooks, integrations, harnesses, agent actions, run badges.
- `github.md`: the GitHub views, the `gh` client, the task picker.
- `transcripts-and-runs.md`: transcripts, Flow tab, token totals, `run.events`, the Runs view.
- `changes-and-git.md`: the Changes tab, line comments, findings, fetch/pull.
- `frontend.md`: app shell, tab layout, terminal search, sidebar order.
- `release-and-site.md`: the updater and `site/`.
- `ci-and-releases.md`: workflows, Renovate, security settings, macOS signing.
- `tests.md`: before you add or review a test; the revert question.

## Rules

- **The backend owns state** (`poimen-server`). poimen never stores pane state of its own; snapshots from the backend are authoritative and events only invalidate them.
- **Non-destructive.** Closing a view or the app releases controllers and never ends a terminal. Taking over a controller requires an explicit user action. One exception, off by default: the server may stop an idle agent process (status `done` for the set time, harness with `exitInput` and `resumeArgs`, known session id) and resume its session on the first controller attach; the terminal itself is never ended ([ADR 0014](docs/adr/0014-hibernate-idle-agents.md)).
- **Tests stay read-only.** Live tests (`#[ignore]`) may list and observe, never control, send input or create panes in the user's session.
- **Buttons, tags and fields (#844).** Actions are `button[hx-button]` (helix-ui, `HxButton` in the component's `imports`), information is not a button.
  - One filled **primary** per surface: the action the surface exists for (Save, Install). Everything else is `variant="outlined" severity="secondary"`; low-weight actions (Cancel in a row, Reset, a link-like step) are `variant="text"`. **Danger** (`severity="danger"`) only for an action that destroys or cannot be undone (Delete, Remove, Stop server), filled when it is the one action of a confirmation, else outlined or text.
  - Small is the default in dense rows (`size="small"`, `min-height` is `--control-height` in `src/styles.css`). Icon-only buttons have `aria-label` and a `title` (tooltip).
  - A status, count or label that does nothing is an `hx-tag` (or plain text): no border, no pointer cursor, no hover. If it can be clicked it is a button (`variant="text"`), which has the hover and `:focus-visible` state.
  - Inputs and selects in a row use `height: var(--control-height)` so `align-items: center` lines them up with the buttons.
  - Colors come from the tokens only; the button specs (`button-contrast`, `button-usage`) and reference surfaces are in `docs/agents/frontend.md`.
- **Tooltips (#1039).** Icon-only controls use `hx-tooltip`, not `title` (`aria-label` stays); `ui/title-usage.spec.ts` counts `title`; see `frontend.md`.
- **Context menus (#851).** Every context menu is an `app-context-menu` (`src/app/ui/context-menu/`, model in `menu-model.ts`): the parent renders it with `@if` at a point (`at`) with `MenuGroup`s and handles `picked`/`closed`. It owns the backdrop, the focus (taken on open, returned to the opener), Arrow/Home/End/first-letter/Enter/Escape, group separators and headings, the clamping inside the window (`clampMenu`, `alignRight`) and disabled entries (`disabled: "reason"` is the tooltip, dimmed, focusable, picks nothing). No menu CSS in a component.
- **Dialogs (#848).** Every dialog is an `app-dialog-shell` (`src/app/ui/dialog-shell/`): a native modal `<dialog>` with the heading, the one close icon button and the content projected; buttons go in a `.dialog-buttons` element (Cancel first, the confirming action last as the one primary, `severity="danger"` for a destructive one), the shared rules for `.hint`, `.error`, labels and fields are in `src/styles.css` under `dialog.dialog`. The shell moves the focus into the dialog (first field, else the confirming button; `[cancelDefault]="true"` for a destructive or risky confirmation puts it on Cancel and Enter does not confirm), keeps Tab inside, emits `cancelled` for Escape, backdrop and the close icon, `confirmed` for Enter outside a form, a button and a list, and returns the focus to the opener. A dialog spec checks focus on open, Escape and focus return with `ui/dialog-shell/dialog-contract.ts`. Components use decorator inputs in the shell and `Icon` because the specs run without Angular's compiler transform.
- **Feedback (#1040).** Toast (`SpaceActions.notify`/`fail`): a result the user did not watch, or why a write failed. Inline: a lasting state (disconnected, read-only). Agent events (waiting, done, failed): notification center only, never a toast.
- **Protocol drift.** The server protocol is versioned (`PROTOCOL_VERSION`). Keep all wire shapes in the `poimen-protocol` crate, each with a parsing test.

## Decisions

- Frontend stack: Angular 22 and helix-ui, and when a component goes to helix-ui ([ADR 0001](docs/adr/0001-frontend-stack.md)).
- Color schemes: bundled and imported schemes, a derived chrome and the `colorSchema` setting ([ADR 0002](docs/adr/0002-color-schemes.md)).
- Own terminal backend `poimen-server`, the only one (herdr removed, #174); agent status from Claude Code and Copilot hooks (proposed, [ADR 0003](docs/adr/0003-own-terminal-backend-without-herdr.md)).
- GitHub views and agent harnesses native in poimen; the herdr plugin integration (ADR 0004) is removed (accepted, [ADR 0005](docs/adr/0005-github-views-and-agent-harnesses.md)).
- poimen console with herdr scope: one `poimen` binary (TUI and CLI), tabs and splits owned by poimen-server, shared code in `poimen-core`; supersedes ADR 0003 in part (proposed, [ADR 0006](docs/adr/0006-poimen-console-with-herdr-scope.md)).
- Plugin system with a sandboxed worker and view contexts; supersedes ADR 0005's "no plugin system now" in part (proposed, [ADR 0007](docs/adr/0007-plugin-system-with-sandboxed-worker-and-view-contexts.md)).
- Remote machines over `poimen-server stdio` through SSH, no open port and no stored secret (proposed, [ADR 0008](docs/adr/0008-remote-machines-over-ssh.md)).
- Terminals owned by a stable `poimen-pty-host` per terminal, so a server update or crash keeps them (accepted, [ADR 0009](docs/adr/0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md)).
- Shells in a terminal panel at the bottom of the app by default, agents in editor tabs; the placement (`editor`/`panel`) is a server-owned tab field, splits stay (proposed, [ADR 0010](docs/adr/0010-terminal-panel-for-shells-in-the-app.md)).
- Named layouts: `layouts[]` in the user config with the shape of ADR 0006 (tabs with split trees, nested tabs, app-only `place`), templates refer to them by name, old keys keep working, presets stay project palette entries (proposed, [ADR 0011](docs/adr/0011-named-layouts-referenced-by-templates.md)).
- MCP server for agent sessions: `poimen mcp` over `poimen-client`; writes are free in the caller's own space, any other space needs a confirmation in the app (proposed, [ADR 0012](docs/adr/0012-mcp-server-for-agent-sessions.md)).
- Install `poimen-server` on a machine from the app after a confirmation, over the ssh connection, checksum verified; amends ADR 0008 (proposed, [ADR 0013](docs/adr/0013-install-poimen-server-on-a-machine-from-the-app.md)).
- Hibernate idle agents: the server stops the agent process of a `done` terminal after a set idle time and resumes the session on the first controller attach, off by default, an exception to the non-destructive rule (proposed, [ADR 0014](docs/adr/0014-hibernate-idle-agents.md)).
- poimen hosts the sessions of scripted runs: one `poimen-client` operation, interactive sessions, JSON-line records, permissions as ADR 0012 (accepted, [ADR 0016](docs/adr/0016-poimen-hosts-the-sessions-of-scripted-runs.md)).
- Scoped write grant, lead runs and `wait_any`; amends ADR 0012 (proposed, [ADR 0018](docs/adr/0018-scoped-write-grant-and-lead-runs-for-orchestration.md)).

## Verification

```sh
pnpm check && pnpm lint && pnpm format:check && pnpm test && pnpm build
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
pnpm ratchets
```

Before a push: `pnpm check:changed [base]` runs only what the changed paths need (`scripts/changed-paths.json`, as in CI). The full run above stays before the pull request.

`pnpm ratchets` (`scripts/ratchets/`) runs checks with a baseline that may only shrink: files over 800 lines, real-time waits and unnumbered skips in tests, paths named in the notes that do not exist, and the size of this file. When one fails, fix the cause (split the file, wait on a condition, correct the path, move text to `docs/agents/`); never add to a baseline by hand. When you fixed entries, `pnpm ratchets --prune` lowers the baselines (it never adds or raises one). Run it alone and first (it takes seconds, no build): after a change near a limit, before build and tests; a fix only a ratchet asked for needs `pnpm ratchets` and the touched tests, not the full suite again.

## CI and releases

- Reusable workflows and actions of GravionLabs/ci are pinned to a commit SHA with the version as a comment (`@<sha> # v1.15.0`), never `@main` or a branch; Renovate updates them. Details: `docs/agents/ci-and-releases.md`.
- Manual acceptance: [`docs/manual-acceptance.md`](docs/manual-acceptance.md) is the checklist per OS and package (macOS `.dmg`; Windows `-setup.exe` and `.msi`; Linux `.deb`, `.rpm` and `.AppImage`) that CI cannot run. The RC test is done by hand beside the release PR, not as a gate before it (owner decision 2026-11-07), with `tauri dev` and the installers of the pre-release; no `release-test` issue is created per milestone anymore. A gap does not block the release PR; it becomes a bug.
- Work is integrated on `release/next`: every PBI branch is cut from it and its PR targets it (squash); `main` only receives the release PR (merge commit). `release.yml` versions with GitVersion, writes `CHANGELOG.md` with git-cliff and tags `v<version>`. README "Releases" has the rules.
- Release flow: release review -> pre-release (`release.yml` dispatched on `release/next`, the channel comes from the branch: `v<version>-rc<nnn>`, GitHub pre-release, installers without MSI and RPM, no commit, no changelog, no issue closing) built as **Poimen RC** (`POIMEN_CHANNEL=rc` and the overlay `src-tauri/tauri.rc.conf.json`: identifier `dev.gravionlabs.poimen.rc`, `mainBinaryName` `poimen-rc`, sidecar `poimen-server-rc` from `scripts/prepare-sidecar.sh`, no updater artifacts or endpoint), installed next to the stable app; `RC` in the window title (`lib.rs::window_title`, `core/backend.ts::windowTitle`) and the status bar (`app_channel`) -> release PR open at once -> manual RC test beside it (`docs/manual-acceptance.md`) -> bugs in the milestone while the release PR is open become a new RC, otherwise they go to the next milestone -> human merge of the release PR. GitVersion (`tag-prefix`) and git-cliff (`tag_pattern`) ignore `-rc` tags; `releases/latest` ignores pre-releases (`docs/updater.md`). A dispatch on any other branch builds `-beta<nnn>` installers as workflow artifacts (no tag, release or commit).
