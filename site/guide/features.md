# Features

Everything here is in the application today; the [README](https://github.com/GravionLabs/poimen#readme) has every detail.

## herdr as the backend

- herdr owns the state. poimen never stores pane state; snapshots from herdr are authoritative and events only invalidate them.
- Non-destructive: closing a tab or the app releases controllers and stops only the CLI child processes, never herdr panes. Taking over a controller is always an explicit action.
- poimen talks to herdr's socket API (NDJSON over the Unix socket) and attaches terminals with `herdr terminal session observe|control`.

## Spaces sidebar

- A tree of herdr workspaces with their panes, per row agent, status, branch and worktree (`git` is read-only). A filter field searches by name, agent or branch.
- Create a space (directory with completion, native folder picker, label), a terminal in a space, split a pane right or down, start an agent in a shell pane, rename and close panes. Closing a pane always asks first and warns while an agent works.
- Close a space from its context menu, the × icon left of the chevron on its row (shown on hover, keyboard focus and for the active space) or the command palette. A space with panes asks first and says how many panes and agents the close ends.
- `sidebar.layout` switches between the tree and two lists (Spaces and the panes of the selected one); `sidebar.rows` defines what a row shows.
- An Explorer view (second item of the activity bar) browses directories.

## Editor tabs with live terminals

- One tab per pane; a preview tab becomes a pinned tab on use. One xterm.js shows the active tab with the WebGL renderer.
- Input by default; read-only when another client controls the pane, with an explicit *Take over*. A mode button toggles input and read-only per tab.
- A read-only agent tab has a prompt bar above the terminal that sends a prompt without taking over the controller; it is hidden while the tab controls the terminal, and the typed text is kept.
- Cmd+click (macOS) or Ctrl+click opens links; Shift bypasses mouse forwarding for text selection. Mouse cells follow the frame grid herdr sends.
- Compact command palette (`CmdOrCtrl+Shift+P`, JetBrains style: highlighted matches, shortcuts, recent entries) with poimen commands and herdr plugin actions; every shortcut can be changed. It opens centered on the editor area, which also holds non-terminal tabs (on the window without an open tab). The ? button in the status bar (`CmdOrCtrl+/`) lists all shortcuts with your current bindings.

## Agent status and attention

- Statuses (working, idle, blocked, done) come from herdr events.
- A pane that turns blocked while not viewed rings on its tab and sidebar row; one that turns done counts as unread; workspace rows sum their panes. Viewing the tab clears both.
- Optional macOS notifications for blocked and done, and a dock badge with the number of waiting agents. A notification center view lists recent status changes.
- `CmdOrCtrl+Shift+A` jumps to the next waiting agent.

## Subagents and the Flow tab

- The Output panel (`CmdOrCtrl+J`) follows the agent of the active tab and shows its subagents from the session transcripts of Claude Code (`~/.claude/projects`) and GitHub Copilot CLI (`~/.copilot/session-state`), read-only.
- The Flow tab draws the run as a graph: the main agent, skill calls and subagents, parallel ones side by side, nested by the call that started them, with status and duration. It updates live, has a minimap and fit, and opens the entry on click. A Flow can also be opened as its own editor tab.

## Deliver board

- A view fed by the herdr `deliver-board` plugin (it must be installed): one table per repository among your pane directories, with the open PBIs, claim, labels, branch and pull request with check state. It refreshes on demand, on window focus and every 60 s. The issue and PR numbers open in the browser, and "Start deliver" runs the plugin's action.
- It will be replaced by built-in GitHub views: an issues tree, a pull request view and milestones, read through the `gh` CLI, with no plugin and no Node.js needed.

## Color schemes

- Light, dark or system. Bundled schemes, the scheme of your herdr config (`"herdr"`), and your own WezTerm color schemes in `~/.config/poimen/themes/`, watched without a restart. The window chrome is derived from the scheme.
- Optionally the terminal font and colors of your Ghostty config.

## Settings

Optional `~/.config/poimen/config.json`, applied without a restart; it holds preferences only. Examples: `theme`, `colorSchema`, `terminal.fontFamily`, `terminal.fontSize`, `terminal.autoControl`, `shortcuts`, `spaces.layout` (the pane layout of a new space), `explorer.roots`, `sidebar.layout`, `sidebar.rows`, `herdr.autoStart`, `notifications`. The full list is in the [README](https://github.com/GravionLabs/poimen#settings).
