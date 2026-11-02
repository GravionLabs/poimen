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
- **Input by default.** A pane opens for input: poimen first tries `herdr terminal session control` without `--takeover` and, as the only controller, sets the terminal size itself so the output width follows the window. If another client already holds the controller (for example the herdr terminal UI), the tab falls back to an observer (read-only) and says why, with an explicit *Take over* button (never automatic). The flat mode button in the status bar (text `Input` or `Read-only` with a pencil or lock icon; the tab shows `RO` or `IN`) toggles between input and forced read-only per tab while it is open; it is dimmed and disabled while another client controls the terminal. If another client takes the controller later, the tab goes read-only with a banner and going back to input stays your action. `terminal.autoControl: false` opens all tabs read-only again. In input mode the mouse goes to the pane application; Cmd+click (macOS) or Ctrl+click (Linux, Windows) on a link opens it in the default browser (URLs found in plain text lose trailing `. , ; : ' "` and an unbalanced closing `)` or `]`, like the herdr link-trim plugin; `Foo_(bar)` stays whole), and Shift bypasses the forwarding for text selection (Shift+drag). Copy the selection with Cmd+C (macOS) or Ctrl+Shift+C, in input and read-only tabs (read-only tabs select with a plain drag). Paste with Cmd+V or Ctrl+Shift+V in input mode, as bracketed paste when the pane application asked for it. The wheel scrolls the pane.
- **Editor tabs.** Opening a pane adds a tab to one tab bar above the terminal: a click opens a preview tab (italic, replaced by the next preview), a double click pins it, a second click on an open pane activates its tab. Tabs close with the x or a middle click and reorder by drag. Only the active tab of each group keeps a stream open; hidden tabs close theirs, the pane keeps running.
- **Split editor groups.** The tab context menu (right click) splits a tab right or down into a new group with its own tab bar; `Cmd+\` / `Ctrl+\` splits the active tab right, with Shift down. Drag a tab onto another group's tab bar to move it. Dividers resize by drag or with the arrow keys; each visible group streams its active tab and its terminal size is sent to herdr when it changes. Closing the last tab of a group removes the group. A pane is open in one group at a time.
- **Angular frontend.** The UI is Angular 22 with helix-ui (light and dark follow the system); `src/app/core` is the bridge to the Tauri commands and events, `src-tauri` stays a thin shell ([ADR 0001](docs/adr/0001-frontend-stack.md)).
- **Spaces tree.** The sidebar is a helix `hx-tree` of workspaces and their panes (the herdr tab is secondary text). A badge tells agents (by kind, e.g. `cl` for claude) from plain terminals (`>_`); each row shows the status dot, git branch (`wt` for a linked worktree), directory (`foreground_cwd`) and the status message, state label or integration tokens when present. The filter field matches name, agent, branch, tab, directory and text; arrow keys, Home/End and type-ahead navigate, Enter opens. Branch and worktree come from a read-only `git rev-parse` in the pane directory (herdr's pane list has none).
- **Layout.** An activity bar (Spaces, Explorer) sits left of a sidebar you resize by dragging its edge (arrow keys work too) and toggle with `Cmd+B` (`Ctrl+B` elsewhere). Width and collapsed state persist in poimen's own UI state (webview local storage, not the user settings file), and so do the open tabs, active tab, editor groups and split sizes: poimen reopens as it was closed, and tabs whose pane no longer exists in herdr are dropped with a notice; the layout works from 720 px window width.
- **Toolbar.** A thin bar at the top has a flat theme button on the right (icon and text: auto, sun or moon for `system`, `light`, `dark`). A click cycles system → light → dark; the tooltip and accessible name read like `Theme: system (click for light)`. The choice is written to the `theme` entry of `config.json` (other keys and comments stay) and applies at once; `system` follows the OS appearance live, and an external edit of the file updates the button.
- **Status bar.** The bottom bar shows the herdr connection (connected, reconnecting when the event watcher lost herdr, unreachable when a snapshot read failed, each with the reason), the count of waiting (blocked) agents (a click opens the next one) and the active tab: pane id, the mode button (input, or read-only with the reason when another client controls the pane) and terminal size.
- **Events are invalidation signals.** Every herdr event triggers a debounced re-read of the workspace, tab and pane lists, as the herdr socket API docs recommend. `pane.agent_status_changed` is applied right away.

### Creating, splitting and closing panes

Panes are managed in herdr, so the result shows up through herdr's events like any other change.

- **New space**: the "+" button in the Spaces header opens a dialog with a directory field (completes directories as you type), a Browse button (native folder picker) and a label (default: the directory name). It creates a herdr workspace (`workspace.create` with `cwd` and `label`) and focuses it. A space that already has a pane inside the directory, else one labelled with the directory name, is focused instead and nothing is created. A label that another space uses falls back to the path relative to the home directory. A missing directory or a failing herdr call shows the error in the dialog.
- **New terminal**: File > New Terminal (`CmdOrCtrl+T`), or right click a workspace in the sidebar. A dialog asks for the directory (the workspace's pane directories are suggested; empty uses herdr's default). It creates a tab in the workspace (`tab.create`) and opens it.
- **Split**: right click a pane in the sidebar (Split right / Split down), or its tab (New pane to the right / below). The new shell starts in the pane's directory (`pane.split`).
- **Close pane**: right click a pane in the sidebar or its tab (Close pane / Close herdr pane). It always asks first and warns when an agent is working or waiting (`pane.close`). Closing a tab with the tab's own Close entry still only closes the view.
- **Start agent**: right click a pane in the sidebar or its tab (Start agent...). A dialog asks for the agent kind, an optional name (defaults to the kind) and optional arguments (split on spaces); the pane has to be at its shell prompt (`agent.start`).
- **Rename**: right click a pane (Rename... / Rename pane...). An agent pane renames the agent (`agent.rename`), a plain terminal its label (`pane.rename`); an empty name clears it.

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

The menu bar has File (new terminal, close tab), Edit, View (sidebar, split, zoom, refresh), Go (command palette, next waiting agent, tab switching) and Window. Defaults follow VS Code; `CmdOrCtrl` is Cmd on macOS and Ctrl elsewhere. Every shortcut can be changed in the settings file (see below).

| Action | Setting key | Default |
|---|---|---|
| New terminal (asks for the directory) | `newTerminal` | CmdOrCtrl+T |
| Close tab | `closeTab` | CmdOrCtrl+W |
| Close all tabs | `closeAll` | CmdOrCtrl+Shift+W |
| Refresh | `refresh` | CmdOrCtrl+R |
| Toggle sidebar | `toggleSidebar` | CmdOrCtrl+B |
| Split right / down | `splitRight` / `splitDown` | CmdOrCtrl+\\ / CmdOrCtrl+Shift+\\ |
| Zoom in / out / actual size (terminal font) | `zoomIn` / `zoomOut` / `zoomReset` | CmdOrCtrl+= / CmdOrCtrl+- / CmdOrCtrl+0 |
| Command palette (poimen commands and herdr plugin actions) | `commandPalette` | CmdOrCtrl+Shift+P |
| Next waiting agent | `nextWaiting` | CmdOrCtrl+Shift+A |
| Next / previous tab | `nextTab` / `previousTab` | CmdOrCtrl+Alt+Right / CmdOrCtrl+Alt+Left |

The **Plugins** button in the top bar lists the herdr plugin actions (`plugin.action.list`, re-read each time the menu opens, on window focus and at start); a pick runs the action with the active pane as context. The context menu of a pane (sidebar row or tab) lists the same actions and runs them with that pane as context. Errors show as a notice.

While a terminal has focus, only shortcuts that use Cmd are captured; every other key (Ctrl combinations on Linux and Windows included) goes to the program in the pane. Use the menu, or bind a `Cmd+` (Super) shortcut, to reach an action from a focused terminal there.

## Settings

Optional `~/.config/poimen/config.json`, watched and applied without a restart. Invalid entries log a warning and fall back to the default. It holds preferences only; UI state (sidebar width, open tabs) is kept by the app.

```json
{
  "theme": "system",
  "terminal": { "fontFamily": "JetBrains Mono", "fontSize": 13, "ghostty": false, "autoControl": true },
  "shortcuts": { "toggleSidebar": "Ctrl+Alt+B", "refresh": "" },
  "spaces": { "layout": { "panes": [{ "name": "editor", "command": "nvim" }, { "name": "shell", "split": { "from": "editor", "direction": "down", "ratio": 0.7 } }] } },
  "explorer": { "roots": ["~", "/srv/projects"], "showHidden": false },
  "sidebar": {
    "rows": {
      "agent": [["state_icon", { "token": "name", "bold": true }, "state_text"], ["agent", "$deliver_step", "branch", "dir"]]
    }
  }
}
```

- `theme`: `light`, `dark` or `system` (default). The toolbar button writes this entry; poimen writes only the entries its settings view and toggles change, never the rest of the file.
- `colorSchema`: the name of a bundled color scheme, `"herdr"` (the scheme named in `[theme] name` of `~/.config/herdr/config.toml`, watched like `config.json`; it fixes light or dark like a single scheme and disables the toolbar button, and `terminal`, a missing file or an unknown name warns and uses `poimen`), or a pair `{ "light": "<name>", "dark": "<name>" }` that `theme` picks from (`system` follows the OS appearance, light when it cannot be read) and that keeps the toolbar theme button working; a single scheme fixes light or dark from its background and disables that button (tooltip "Theme fixed by color schema <name>"), and a pair needs both names or it warns and uses `poimen`. Names: `poimen` (default, follows `theme`), or one of herdr's `catppuccin`, `tokyo-night`, `tokyo-night-day`, `dracula`, `gruvbox`, `gruvbox-light`, `one-dark`, `one-light`, `solarized`, `solarized-light`, `kanagawa`, `kanagawa-lotus`, `rose-pine`, `rose-pine-dawn`, `vesper`. The scheme colors the terminal and the chrome (surfaces, accent, status colors, derived from its palette) and decides light or dark from its background, so `theme` is ignored while one is set. A wrong type or an unknown name logs a warning and uses `poimen`. The values and licenses are in `src/app/themes/`.
- The settings view (the gear at the bottom of the activity bar, opens a Settings tab) sets the appearance and the color scheme without editing the file: schemes are listed as dark and light with swatches, hovering or focusing one previews it live, a click or Enter saves it (leaving the list restores the saved one), and a single scheme or a light/dark pair can be chosen. Saving writes only `theme` or `colorSchema` in `config.json` and keeps your comments.
- Your own color schemes: drop WezTerm color scheme files (`.toml` with a `[colors]` table: `ansi` and `brights` of 8 colors each, `foreground`, `background`, optional `cursor_bg`, `selection_bg`, `selection_fg`, and an optional `[metadata] name`) into `~/.config/poimen/themes/` and use the name in `colorSchema`, alone or in a pair. The name is `metadata.name`, else the file name in kebab-case (`Tokyo Night.toml` is `tokyo-night`). Sources: the `colors/` directory of WezTerm or the `wezterm/` folder of [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) (MIT). The directory is watched and changes apply without a restart. A user scheme with a bundled name replaces the bundled one; an invalid file (or a repeated name) logs a warning naming the file and is skipped.
- `terminal.fontFamily` / `terminal.fontSize` (6 to 72, default 13).
- `terminal.autoControl`: `true` (default) opens panes for input when no other client controls them; `false` opens every tab read-only.
- `shortcuts`: accelerator per action (keys in the table above), such as `Cmd+Shift+K`, `CmdOrCtrl+Alt+Left` or `F5`. It needs Cmd, Ctrl or Alt (or is an F key); an empty string unbinds the action. Unknown actions, invalid accelerators and ones already used by another action log a warning and keep the default.
- `spaces.layout`: the pane layout of a **new** space (a focused existing space is not changed). It uses the schema of the herdr-plugins `layout-bootstrap`: `panes[]` with `name`, optional `command` (typed into the pane, then Enter) and, from the second pane on, `split.from` (an earlier pane, default the previous one), `split.direction` (`right` or `down`) and `split.ratio` (0 to 1, default 0.5, the share the split pane keeps); the first pane is the root pane of the space. Each pane starts in the space directory and is renamed to its `name`. The layout is chosen automatically: `.agents/herdr-layout.json` in the directory (the repository root inside a repository), else `spaces.layout`, else the built-in default (`agent` left 60 % running `claude`, `tests` right top, `shell` right bottom). An invalid project file shows a warning in the status bar naming the file and the problem, and the space gets a single pane; an invalid `spaces.layout` logs a warning and uses the default. The **New space layout** section of the settings view edits it without touching the file by hand: it shows `spaces.layout` or the built-in default (marked "Default"), and per pane its name, command (empty = shell) and, from the second pane on, split from / direction / the share the split pane keeps (0.1 to 0.9); panes can be added, removed and moved, problems show inline, and only a valid layout can be saved. "Save" writes only `spaces.layout` (comments kept), "Reset to default" removes it.
- `explorer.roots`: the roots of the Explorer view (second item of the activity bar), a non-empty list of absolute paths; a leading `~` is the home directory (default `["~"]`). The tree shows directories only, sorted by name, and lists a folder when it expands; an unreadable folder shows no children. `explorer.showHidden` (default `false`) shows directories that start with a dot. The active view and the expanded folders are UI state and survive a restart. Double-clicking a folder, or "Create space here" in its context menu, creates a space for it (with the configured layout) or focuses the existing one; folders that are the working directory of a pane of a space carry a dot. A wrong type logs a warning and keeps the default. The **Explorer** section of the settings view manages the roots without editing the file: "Add folder…" (native folder picker), remove (not the last one), move up/down, and "Show hidden folders"; it writes only `explorer.roots` / `explorer.showHidden` (paths under the home directory as `~/…`) and keeps your comments.
- `sidebar.layout`: `"tree"` (default) shows workspaces with their panes below; `"split"` shows two lists as in the herdr sidebar: **Spaces** (the workspaces, the selected one lists its terminal panes) and **Agents** (every agent pane across the workspaces in herdr order, with its workspace name, rendered with `sidebar.rows.agent`), stacked with a divider line between them: drag the divider (or focus it and use the arrow keys, Shift for bigger steps) to resize the two lists, each keeps at least 15 % and scrolls on its own, and the ratio survives a restart (UI state, default 50/50). The toggle button next to the title "Spaces" switches it and writes this entry; the selected workspace of `split` is UI state, not a setting. Anything else logs a warning and uses `tree`.
- `sidebar.rows.workspace` / `sidebar.rows.agent` / `sidebar.rows.terminal`: what a row of the Spaces tree shows, a list of lines, each a list of tokens (a name, or `{ "token": "<name>", "bold": true, "dim": true }`). Tokens: `state_icon` (status dot), `name`, `state_text`, `agent`, `title` (the `deliver_step` token, else the terminal title), `branch`, `dir` (short path), `tab`, and `$<name>` (the herdr token `<name>` of the pane). On a workspace row `branch` and `dir` are the values all its panes share; on a pane row they show only when they differ from the workspace. Empty values and values equal to an earlier one on the row are skipped. The filter matches the shown values. The default is `workspace`: `[["state_icon","name","state_text","branch","dir"]]`, `agent` and `terminal`: `[["state_icon","name","state_text"],["agent","title","branch","dir"]]`. An unknown token, a wrong type or an empty list logs a warning and keeps the default of that row type.
- `terminal.ghostty`: `true` takes font family, size, `background`, `foreground`, `cursor-color`, `selection-background`, `selection-foreground` and `palette = N=#rrggbb` (0 to 15) from `~/.config/ghostty/config`. The colors lie over the terminal colors of the color scheme (the chrome stays from the scheme); explicit entries above win. A Ghostty `theme = <name>` is not read.

## Releases

- Work branches are cut from `release/next` and merge into it; the `Release branch` workflow keeps the release PR `release/next` → `main` open and current (`github-release` skill).
- Work PRs into `release/**` are **squash** merged; their PR title must be a Conventional Commit (`pr-checks` title job) and becomes the one commit of the change.
- A release is made by merging that release PR into `main` with a **merge commit, never squash** (the `main` ruleset allows only merge commits), so GitVersion sees every commit. A hotfix PR into `main` is merged the same way, and every commit of it must be a Conventional Commit.
- The version comes from Conventional Commits (`versioning/determine-version` of GravionLabs/ci, `GitVersion.yml`): `fix:`/`perf:`/`revert:` → patch, `feat:` → minor, `type!:` or `BREAKING CHANGE:` → major. A release with only fixes is a patch, one with a `feat:` a minor.
- While the version is 0.x, use **no** `!` and no `BREAKING CHANGE:` until 1.0 is intended: either one jumps the version to 1.0.0.
- On the push to `main`, `.github/workflows/release.yml` runs the CI checks, determines the version, commits `CHANGELOG.md` (git-cliff, `cliff.toml`) to `main`, tags `v<version>`, creates the GitHub Release and closes the issues the release delivered. The changelog commit is pushed with the `GH_PAT` secret (a repository admin, bypass actor of the `main` ruleset).
- `release.yml` writes the released version into `package.json`, `src-tauri/Cargo.toml`, `Cargo.lock` and `src-tauri/tauri.conf.json` and commits it (`chore(release): set version <v> [skip ci]`) before the changelog commit, so the tag points at manifests carrying the released version. Between releases they hold the previous release's version.
- The status bar shows `poimen <version>` (the Tauri app version from `tauri.conf.json`) and `herdr <version>` (socket `ping`; `herdr ?` when unreachable or too old; warning colour and tooltip when the protocol differs from the supported 22). The `tauri.conf.json` version, set by `release.yml`, is the source of truth; `herdr <version>` is shown without a protocol check when herdr reports no protocol. Local builds show the last released version.
- Tags are `v<major>.<minor>.<patch>`, as in the other GravionLabs repositories; milestones carry the same names.
- The rulesets `main` and `release` are created by `scripts/setup-rulesets.sh`.

## License

MIT
