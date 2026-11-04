# ADR 0004: Native views for herdr plugins

- Status: Superseded by [ADR 0005](0005-github-views-and-agent-harnesses.md) (plugin view contract)
- Date: 2026-11-02
- Issues: #42, Feature #14

> Superseded: poimen builds these views in (ADR 0005) and removes its herdr plugin integration (#234). Kept for the record.

## Context

The GravionLabs plugins in [GravionLabs/herdr-plugins](https://github.com/GravionLabs/herdr-plugins) (herdr 0.9.5, API protocol 22) draw terminal UIs: popups that a Node script renders with escape sequences. poimen already runs them:

- `plugin_actions` / `plugin_invoke` list and run plugin actions (`plugin.action.list`, `plugin.action.invoke`) from the Plugins menu, the context menus and the command palette.
- Plugin panes open as tabs (#41, `core/plugin-panes.ts`): `plugin.pane.open` with placement `tab`, then the pane is an xterm.js tab like any other.

That works, but a TUI inside xterm.js cannot use what poimen has: selectable rows, links, a sidebar badge, native keyboard handling, light and dark themes. The plugins that matter for the deliver workflow show structured data:

| Plugin | What it shows | Where the data comes from | Interaction |
|---|---|---|---|
| `deliver-board` | One row per open `pbi` issue: claim, labels, branch, PR, check counts | `gh api` issues and `gh pr list` (cached 30 s with ETag in `HERDR_PLUGIN_STATE_DIR`), `git ls-remote`; parsed in `lib/board.ts` | Select, open PR or issue in the browser, refresh |
| `review-popup` | Last deliver review of the PBI behind a branch: round, unmet criteria, blocking findings, notes, stale flag | File `<git-common-dir>/.agents/state/review-<nr>.json` (schema `schema/deliver-review.schema.json`, version 1), validated in `lib/review.ts`, found in `lib/load.ts` | Select a finding, open `file:line` in `$EDITOR` in a split, list all reviews off a PBI branch |
| `deliver-view` | Nothing visual: it sets a herdr agent view (`agent.view.set`) that sorts the Agents list (blocked first, then `deliver` token) | The pane tokens `deliver`, `deliver_step`, `deliver_pr` the deliver skill reports | None (startup hook and one `apply` action) |

Facts about the herdr plugin API that bound the options:

- A plugin declares `[[actions]]` (a command), `[[panes]]` (a command with a placement: overlay, popup, split, tab, zoomed), `[[startup]]` and event hooks. A plugin has no way to push a view into a GUI; herdr only knows terminal panes.
- `plugin.action.invoke` returns `plugin_action_invoked` with a `log` entry (`PluginCommandLogInfo`: `status` running, succeeded or failed, `exit_code`, `stdout`, `stderr`). `plugin.log.list` returns the same entries. The status can be `running`, so stdout is not guaranteed to be complete in the invoke response, and the schema does not state a size limit for it.
- `plugin.list` gives each plugin's `manifest_path`, so the plugin directory is known. Actions and panes get `HERDR_PLUGIN_STATE_DIR`, `HERDR_PLUGIN_CONTEXT_JSON` and `HERDR_BIN_PATH` from herdr; a process poimen starts itself does not.
- The plugins keep their logic in `lib/` modules that run without herdr (`lib/board.ts`, `lib/review.ts`, `lib/load.ts`, `lib/view.ts`); only `bin/*.ts` and the rendering are terminal-specific. Their tests run against recorded `gh` JSON and fixtures.

ADR 0003 (proposed) may remove herdr as the backend. A contract that works only through herdr's API would not survive that.

## Decision

poimen renders native views for two plugins and leaves the third as a terminal pane. The plugin keeps the logic, poimen only renders.

### Candidates and criteria

A plugin gets a native view when all of these hold:

1. **Structured data.** Its output is a list or record, not a stream of terminal output.
2. **Read-only core.** The view can show the data without poimen implementing the plugin's rules. Writes stay actions of the plugin.
3. **A native view gains something.** Selection, links, badges or layout that a TUI in xterm.js cannot give.
4. **Logic stays in one place.** No second implementation of the parsing, caching or validation in Rust or TypeScript.

| Candidate | Decision | Reason |
|---|---|---|
| `deliver-board` | Native view (first, PBI #43) | Meets all four: tabular, read-only, the biggest gain (rows, status badges, PR links), logic in `lib/board.ts` |
| `review-popup` | Native view (second) | Structured JSON with a schema; findings become clickable `file:line` entries. Needs the open-in-editor action, so it follows the board |
| `deliver-view` | No native view | Criterion 1 fails: the plugin has no data to show, it configures herdr's agent list. poimen's own Agents tree can honor the same tokens later; that is poimen UI work, not a plugin view |

Plugins without data (`link-trim`, `agent-notify`, `worktree-cleanup`, `layout-bootstrap`, `deliver-starter`, `deliver-log`, `command-palette`, `repo-open`) stay actions and terminal panes. The terminal pane of `deliver-board` and `review-popup` stays: it still serves herdr's TUI and any poimen without the native view.

### Contract: a `data` command that prints JSON

A plugin that wants a native view ships a headless entry point and declares it in its manifest as an ordinary action:

```toml
[[actions]]
id = "data"
title = "Deliver board data (JSON)"
command = ["node", "bin/data.ts"]
```

Rules of the contract:

- **Output.** One JSON object on stdout, exit code 0. Errors are part of the data (`{"version":1,"error":{"code":"no-gh-login","message":"..."}}`), exit code non-zero only for a crash. Nothing else on stdout; diagnostics go to stderr.
- **Envelope.** `{"version": <int>, "plugin": "<id>", "generated": "<ISO 8601>", "data": {...}}` or the error form above. `version` is the shape version of that plugin's `data`; poimen supports a list of versions per plugin and shows "update the plugin" for an unknown one. The shapes are JSON Schemas in the plugin repository (`schema/data.schema.json`), copied into poimen as fixtures with a parsing test, like the wire shapes of herdr.
- **Input.** Arguments, not herdr context: `--cwd <dir>` (the repository), plus per-plugin flags. The context herdr would pass (`HERDR_PLUGIN_CONTEXT_JSON`) is optional; the script must work without it.
- **Read-only and idempotent.** The command writes nothing except its own cache.
- **Own entry point.** poimen reads `manifest_path` from `plugin.list`, takes the action's `command` and runs it itself as a child process in the plugin directory, with a timeout. It sets `HERDR_PLUGIN_STATE_DIR` to the plugin's state directory when herdr reports one and otherwise to a poimen directory, so the 30 s `gh` cache of the board is shared between popup and view. It does not go through `plugin.action.invoke`.

Shapes `#43` can build on (version 1; the exact schema lands with the plugin PBI):

```json
{
  "version": 1,
  "plugin": "deliver-board",
  "generated": "2026-11-02T10:00:00Z",
  "data": {
    "repo": "GravionLabs/poimen",
    "rows": [
      {
        "number": 42,
        "title": "docs(adr): native views for herdr plugins",
        "url": "https://github.com/GravionLabs/poimen/issues/42",
        "assignee": "jkaufmann",
        "labels": ["in-progress"],
        "branch": "docs/42-adr-native-plugin-views",
        "pr": {
          "number": 150,
          "url": "https://github.com/GravionLabs/poimen/pull/150",
          "state": "OPEN",
          "checks": { "state": "pending", "passed": 3, "failed": 0, "pending": 1, "skipped": 0 }
        }
      }
    ]
  }
}
```

These fields are the existing `Row`, `PullRequest` and `ChecksSummary` types of `lib/board.ts`, so the first version is a serialization of what the plugin already computes. `review-popup` returns `{ "pbi", "branch", "sha", "head", "stale", "round", "unmet": [], "review": {...} }` (the file's schema plus the computed `head` and `stale`) for a PBI branch and `{ "reviews": [{ "pbi", "round", "branch", "stale" }] }` otherwise.

### Actions stay in the plugin

The view never reimplements an action. Opening a PR is `shell open` in poimen. Opening a finding in `$EDITOR` in a split calls the plugin's own action (`plugin_invoke`) with the file and line in the context, or poimen's own editor tab once it has one. Refresh is "run the data command again", on a user action and on focus, never on a timer faster than the 30 s cache.

### poimen side

- A `PluginView` registry maps `plugin_id` plus pane entrypoint to an Angular component and its supported `version`s. Where a native view exists and the plugin's `data` action is present, the pane entry in the palette opens the native tab; otherwise it opens the terminal pane (#41) as today.
- A native view is an editor tab of its own kind, not a herdr pane. It adds no pane state: poimen only caches the last payload for display (the "herdr owns state" rule is about panes).
- A Tauri command `plugin_data(plugin_id, args)` runs the command and returns the parsed envelope; the parsing test lives next to the other wire shapes in `herdr.rs`. Everything is read-only; no test controls a pane.

## Consequences

- The plugin stays the single owner of its logic and tests. The terminal UI and the native view render the same data, so they cannot disagree about a PR or a stale review.
- poimen needs no `gh` or `git` code. It does depend on the plugin version: the `version` field and a schema fixture make that visible instead of a silent mismatch.
- poimen runs plugin code outside herdr. Plugins are already installed and trusted by the user and run as the same user from `plugin.action.invoke`; the command is the one the manifest declares, so no new execution path is invented, but it must be run with a timeout, without a shell and with `--cwd` validated against the known spaces.
- Without herdr (ADR 0003) the contract still works if plugins stay installable as directories; poimen would then need its own plugin discovery (see "Leaving herdr" for the triggers). The state-directory and `HERDR_*` assumptions are limited to the cache.
- The data command is extra surface in each plugin and must be kept in step with the terminal UI; sharing `lib/` keeps that small.
- A plugin author who wants a native view must write the data command and the schema; there is no native view for a third-party plugin without a poimen component.

## Alternatives

- **Parse the terminal pane.** Read the plugin pane's screen and extract rows. Fragile, loses structure, breaks on any layout change. Rejected.
- **`plugin.action.invoke` and read `stdout` from the `log`.** No second process management, works through herdr only. The log's `status` can be `running`, so the response may carry partial stdout, and a size limit is not documented. Kept as the fallback if running the command directly turns out to be a problem; the envelope is the same, so switching is a transport change. To be verified against herdr before #43 ships.
- **A state file the plugin keeps current** (for example `HERDR_PLUGIN_STATE_DIR/board.json`), read by poimen. Good for `review-popup`, whose source already is a file, but a board file needs a writer: either the popup is open or a timer runs in the plugin, and the file is stale otherwise. Poimen can read the review file directly, but then it re-implements the worktree lookup and stale check. Rejected as the general contract; a plugin may still implement `data` by reading its file.
- **poimen reimplements the plugins** (own `gh` and `git` calls in Rust). Two implementations of PR matching and review validation. Rejected.
- **A plugin API in herdr for views.** herdr has no GUI; outside poimen's control. Rejected.
- **Native view for `deliver-view`.** Nothing to render; see candidates.

## Leaving herdr

[ADR 0003](0003-own-terminal-backend-without-herdr.md) (proposed) may add a poimen backend and later drop herdr. It puts herdr plugins out of scope and leaves them to the herdr backend. This section records how the `data` contract survives that.

| Herdr-bound | Herdr-independent |
|---|---|
| Discovery: `plugin.list` and `manifest_path` | The `data` contract: envelope, schemas, `--cwd` input |
| The action and pane runtime: `plugin.action.invoke`, `plugin.pane.open` (#39 to #41) | The plugin scripts and `lib/` modules, including the `gh` and `git` calls |

Two ways on, if poimen runs native views without herdr:

1. **poimen's own plugin directory.** poimen reads the same `plugin.toml` manifests from a directory of its own and runs the `data` commands as it does today. No plugin changes. Fits when the plugins stay useful as they are and Node is available.
2. **Port the logic to Rust** where poimen needs it permanently. First candidate: `deliver-board` (`gh api`, `gh pr list`, `git ls-remote`). This is a second implementation, so it is only worth it for a view that poimen treats as a core feature, and it gets the same fixtures as `lib/board.ts`.

Triggers (this ADR's reading of ADR 0003, which names none): (a) ADR 0003 is accepted or later, and the default `backend` moves from `herdr` to `poimen` or herdr is dropped once the own backend covers what poimen uses; (b) the open decision for Epic #3 plugins "per backend" is taken for the poimen backend, which decides only Epic #3. Until one applies nothing changes, and this ADR does not choose between the options. The decision is deferred to ADR 0003.

Rule for plugin authors, so both options stay open: `data` output must not use herdr-specific fields (herdr ids, pane ids, workspace ids). Herdr context may appear only as optional context that a consumer can ignore. The board's `data` action is added by [GravionLabs/herdr-plugins#216](https://github.com/GravionLabs/herdr-plugins/pull/216) (issue #215).

## Proposed follow-ups in GravionLabs/herdr-plugins

Not created yet; the owner opens them when this ADR is accepted.

1. `deliver-board`: `bin/data.ts` and `schema/data.schema.json`, `[[actions]] data`, shared with the popup through `lib/board.ts`; test against the recorded fixtures. Needed by poimen #43.
2. `review-popup`: `data` action for a branch and for the list of reviews, including the computed `stale`; open-finding action callable with file and line from the context.
3. Docs: a "native view contract" page in the plugin docs (envelope, error codes, versioning), and a note in each README.
4. `deliver-view`: nothing. Possible later request: export the sort rule as data so poimen's Agents tree can order the same way.

## Implementation notes (#43)

Deviations from this ADR in the first implementation (Status stays Proposed):

- The state dir is `~/.config/poimen/plugin-state/deliver-board`, not the plugin's shared state dir, because herdr does not report that directory. The popup's 30 s `gh` cache is therefore not shared.
- The palette always opens the native tab (a button in the unsupported state falls back to the terminal pane) and the frontend calls a board-specific `deliver_board` command instead of the generic `plugin_data(plugin_id, args)`, because the PATH and spawn logic is specific to the board so far. Generalizing it is left for the next native view.
