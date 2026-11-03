# Features

Everything here is in the application today; the [README](https://github.com/GravionLabs/poimen#readme) has every detail.

## herdr as the backend

- herdr owns the state. poimen never stores pane state; snapshots from herdr are authoritative and events only invalidate them.
- Non-destructive: closing a tab or the app releases controllers and stops only the CLI child processes, never herdr panes. Taking over a controller is always an explicit action.
- poimen talks to herdr's socket API (NDJSON over the Unix socket) and attaches terminals with `herdr terminal session observe|control`. A later own backend is proposed in [ADR 0003](/adr/0003-own-terminal-backend-without-herdr).

## Spaces sidebar

- A tree of herdr workspaces with their panes, per row agent, status, branch and worktree (`git` is read-only). A filter field searches by name, agent or branch.
- Create a space (directory with completion, native folder picker, label), a terminal in a space, split a pane right or down, start an agent in a shell pane, rename and close panes. Closing a pane always asks first and warns while an agent works.
- `sidebar.layout` switches between the tree and two lists (Spaces and the panes of the selected one); `sidebar.rows` defines what a row shows.
- An Explorer view (second item of the activity bar) browses directories.

## Editor tabs with live terminals

- One tab per pane; a preview tab becomes a pinned tab on use. One xterm.js shows the active tab with the WebGL renderer.
- Input by default; read-only when another client controls the pane, with an explicit *Take over*. A mode button toggles input and read-only per tab.
- Cmd+click (macOS) or Ctrl+click opens links; Shift bypasses mouse forwarding for text selection. Mouse cells follow the frame grid herdr sends.
- Command palette (`CmdOrCtrl+Shift+P`) with poimen commands and herdr plugin actions; every shortcut can be changed.

## Agent status and attention

- Statuses (working, idle, blocked, done) come from herdr events.
- A pane that turns blocked while not viewed rings on its tab and sidebar row; one that turns done counts as unread; workspace rows sum their panes. Viewing the tab clears both.
- Optional macOS notifications for blocked and done, and a dock badge with the number of waiting agents. A notification center view lists recent status changes.
- `CmdOrCtrl+Shift+A` jumps to the next waiting agent.

## Subagents and the Flow tab

- The Output panel (`CmdOrCtrl+J`) follows the agent of the active tab and shows its subagents from the session transcripts of Claude Code (`~/.claude/projects`) and GitHub Copilot CLI (`~/.copilot/session-state`), read-only.
- The Flow tab draws the run as a graph: the main agent, skill calls and subagents, parallel ones side by side, nested by the call that started them, with status and duration. It updates live, has a minimap and fit, and opens the entry on click. A Flow can also be opened as its own editor tab.

## Deliver board

- A native view of the herdr `deliver-board` plugin ([ADR 0004](/adr/0004-native-views-for-herdr-plugins)): one table per repository among your pane directories, with the open PBIs, claim, labels, branch and pull request with check state. It refreshes on demand, on window focus and every 60 s. The issue and PR numbers open in the browser, and "Start deliver" runs the plugin's action.

## Color schemes

- Light, dark or system. Bundled schemes, the scheme of your herdr config (`"herdr"`), and your own WezTerm color schemes in `~/.config/poimen/themes/`, watched without a restart. The window chrome is derived from the scheme ([ADR 0002](/adr/0002-color-schemes)).
- Optionally the terminal font and colors of your Ghostty config.

## Settings

Optional `~/.config/poimen/config.json`, applied without a restart; it holds preferences only. Examples: `theme`, `colorSchema`, `terminal.fontFamily`, `terminal.fontSize`, `terminal.autoControl`, `shortcuts`, `spaces.layout` (the pane layout of a new space), `explorer.roots`, `sidebar.layout`, `sidebar.rows`, `herdr.autoStart`, `notifications`. The full list is in the [README](https://github.com/GravionLabs/poimen#settings).

## Decisions

The architecture decisions are published on this site: [Decisions](/adr/).
