# Spaces and layouts

## Spaces and the sidebar

The sidebar lists **spaces**, one per project directory, with their terminals. Each row shows what you configure it to show: the agent and its status, the git branch (`wt` for a linked worktree), the directory, a pull request badge with its checks, and counts such as changed files or commits ahead and behind. A filter field matches name, agent, branch, tab and directory. The arrow keys, Home and End, and type-ahead move through the rows; Enter opens one.

- **Layout.** `sidebar.layout` picks the tree, two lists (Spaces, and the agents across all of them) or cards (one collapsible card per space). `sidebar.rows` defines what a row shows.
- **Order.** Drag spaces and tabs to arrange them, or sort by name, attention or most recently used (`sidebar.sort`). Pin a space from its menu to keep it on top. Spaces in linked git worktrees sit below the space of their main checkout.
- **Explorer.** The second item of the activity bar browses directories. Double-click a folder or pick *Create space here* to open it as a space.
- **Notifications.** The bell lists the last 100 status changes (blocked, done) with time and tab. A click opens the tab.

## Creating, splitting and closing

- **New space:** the **+** in the Spaces header, a folder in the Explorer, or the command palette.
- **New pane:** `CmdOrCtrl+T` or right-click a space. A dialog asks for the type: Terminal (the default) or an installed agent; the pane starts in the space's directory.
- **Split:** right-click a pane or its tab, *Split right* or *Split down*. The new shell starts in the pane's directory.
- **Start agent:** right-click a shell pane, *Start agent...* (kind, optional name and arguments).
- **Rename and close:** from the same menu. Closing a pane always asks first and warns while an agent works. It ends the terminal and the processes of its session. Closing a **tab** only closes the view.

A space that lost its terminals, for example after a restart that could not adopt them, offers a **Rebuild** button that builds its layout again in its directory.

## Editor tabs and live terminals

A pane opens as a tab above the terminal: a click opens a preview tab, a double click pins it. Tabs reorder by drag, and a tab can be split right or down into another group (`CmdOrCtrl+\`). Only the active tab of each group streams; the others keep running on the server.

A pane opens **for input** when no other client controls it. If another client (for example the `poimen` console) holds the controller, the tab is read-only and says why, with an explicit **Take over** button, never automatic. The mode button in the status bar toggles input and read-only per tab. Cmd+click (macOS) or Ctrl+click opens links, and Shift bypasses a pane application's mouse mode for selecting text. Dropped files type their quoted paths, and a pasted image is saved and its path typed.

## Layouts and templates

A new space gets a **layout**: tabs and splits with a name and an optional command per terminal. poimen picks it in this order:

1. `layout` of the project file `.poimen/config.json` in the repository.
2. The **template** that matches the directory or the `origin` remote (`spaceTemplates`), which also sets the default agent and model.
3. `spaces.layout`, your own default. On the first start poimen writes one for you: an agent pane per installed agent CLI (`claude`, `copilot`) and a shell pane.
4. The built-in default.

Named layouts (`layouts`) can be shared by templates and edited under **Settings > Workspace > Layouts**: tabs with nested child tabs, a split tree per tab, and a preview. *Save current tab as layout...* in a tab's menu stores the tab you arranged. **Templates** has its own page, and both can be exported and imported under **Settings > General > Export & import**.

## Project files and trust

`.poimen/config.json` in a repository can hold a layout, presets, a `setup` and `teardown` command and named commands that show up in the command palette as *Project: ...* and *Preset: ...*. Because these run commands, poimen runs them only after you **trust** the exact file: a dialog lists every command and offers *Trust and run*, *Run without project commands* and *Cancel*. A changed file revokes the trust. **Settings > Workspace > Trusted projects** lists the trusted roots with a *Revoke* button. The format is in [docs/project-config.md](https://github.com/GravionLabs/poimen/blob/main/docs/project-config.md).

## Changes view

*Show changes* in a space's menu opens the diff of the working tree against the base branch (or against `HEAD`): files with counts, side by side or inline, with syntax highlighting. You can comment on lines and send the comments to an agent of the space, stage files, commit and push from there. Push is never forced and is refused on `main` and `release/*`.
