# Features

Everything here is in the application today; the [README](https://github.com/GravionLabs/poimen#readme) has every detail.

## The backend

- `poimen-server` is the default backend. It owns the terminals (PTYs) and the state; poimen never stores pane state, and closing a tab or the app leaves the terminals and their agents running. The installers bundle it and poimen starts it when needed.
- Agent status comes from Claude Code and Copilot CLI hooks; on the first start poimen offers to install them.
- Alternative: `"backend": "herdr"` makes poimen a window onto [herdr](https://herdr.dev) (its socket API, terminals via `herdr terminal session observe|control`). It is removed in a later release.

## Spaces sidebar

- A tree of workspaces (spaces) with their panes, per row agent, status, branch and worktree (`git` is read-only). A filter field searches by name, agent or branch.
- Create a space (directory with completion, native folder picker, label), a terminal in a space, split a pane right or down, start an agent in a shell pane, rename and close panes. Closing a pane always asks first and warns while an agent works.
- Close a space from its context menu, the × icon left of the chevron on its row (shown on hover, keyboard focus and for the active space) or the command palette. A space with panes asks first and says how many panes and agents the close ends.
- `sidebar.layout` switches between the tree and two lists (Spaces and the panes of the selected one); `sidebar.rows` defines what a row shows.
- An Explorer view (second item of the activity bar) browses directories.

## Editor tabs with live terminals

- One tab per pane; a preview tab becomes a pinned tab on use. One xterm.js shows the active tab with the WebGL renderer.
- Input by default; read-only when another client controls the pane, with an explicit *Take over*. A mode button toggles input and read-only per tab.
- A read-only agent tab has a prompt bar above the terminal that sends a prompt without taking over the controller; it is hidden while the tab controls the terminal, and the typed text is kept.
- Cmd+click (macOS) or Ctrl+click opens links; Shift bypasses mouse forwarding for text selection. On herdr streams, mouse cells follow the frame grid herdr sends; on the poimen backend xterm.js handles the mouse.
- Compact command palette (`CmdOrCtrl+Shift+P`, JetBrains style: highlighted matches, shortcuts, recent entries) with the poimen commands; every shortcut can be changed. It opens centered on the editor area, which also holds non-terminal tabs (on the window without an open tab). The ? button in the status bar (`CmdOrCtrl+/`) lists all shortcuts with your current bindings.

## Agent status and attention

- Statuses (working, idle, blocked, done) come from the backend (hooks on `poimen-server`, events on herdr). On the poimen backend an agent that finishes a turn (`Stop` / `agentStop`) reports done, and done stays until the next prompt; a new session reports idle.
- A pane that turns blocked while not viewed rings on its tab and sidebar row; one that turns done counts as unread; workspace rows sum their panes. Viewing the tab clears both.
- Optional macOS notifications for blocked and done, and a dock badge with the number of waiting agents. A notification center view lists recent status changes.
- `CmdOrCtrl+Shift+A` jumps to the next waiting agent.

## Subagents and the Flow tab

- The Output panel (`CmdOrCtrl+J`) follows the agent of the active tab and shows its subagents from the session transcripts of Claude Code (`~/.claude/projects`) and GitHub Copilot CLI (`~/.copilot/session-state`), read-only.
- The Flow tab draws the run as a graph: the main agent, skill calls and subagents, parallel ones side by side, nested by the call that started them, with status and duration. It updates live, has a minimap and fit, and opens the entry on click. A Flow can also be opened as its own editor tab.

## GitHub pull requests

The GitHub item of the activity bar opens a tab with the open pull requests of your repositories: checks, review state, size and age, with filters (review requested, mine, all; by repository; by title or number), multi-select and "Open in browser". It uses the GitHub CLI (`gh auth login`) and refreshes at most every 30 seconds.

Below the pull requests, the Issues section shows the open issues of each repository as a tree over GitHub's native sub-issues, with level (epic, feature, PBI, bug), `[needs-refinement]` and `[needs-decision]` markers, assignees and linked pull requests with their checks. Filter by state, level, label, assignee or text; collapse nodes per repository.

The Milestones section lists the open milestones of each repository ordered by version, marks the current one (the lowest `M<n>`, or legacy `x.y.z`, with open issues) and shows due date, open and closed counts and a progress bar. Select a milestone to filter the issues tree to it.

## Color schemes

- Light, dark or system. Bundled schemes, the scheme of your herdr config (`"herdr"`), and your own WezTerm color schemes in `~/.config/poimen/themes/`, watched without a restart. The window chrome is derived from the scheme.
- Optionally the terminal font and colors of your Ghostty config.

## Settings

Optional `~/.config/poimen/config.json`, applied without a restart; it holds preferences only. Examples: `theme`, `colorSchema`, `terminal.fontFamily`, `terminal.fontSize`, `terminal.autoControl`, `terminal.scrollback`, `backend` (`poimen` by default, or `herdr`; needs a restart), `shortcuts`, `spaces.layout` (the pane layout of a new space), `explorer.roots`, `sidebar.layout`, `sidebar.rows`, `herdr.autoStart`, `notifications`. The full list is in the [README](https://github.com/GravionLabs/poimen#settings).
