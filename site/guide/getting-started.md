# Getting started

This page takes you from a fresh install to a first agent working in a space. [Installation](/guide/installation) has the steps per system.

## How the pieces fit

poimen is a window. The terminals, and the agents running in them, belong to `poimen-server`, a small background service the installers bundle. poimen starts it when nothing answers and never stops it without asking, so closing the window or a tab leaves every terminal and agent running. The same server also serves the [`poimen` console](/guide/console), so you can watch the same terminals from a shell or over ssh.

## First start

1. Start poimen. The status bar shows **poimen connected** and the sidebar lists your spaces (none yet).
2. If an agent CLI is installed, poimen offers to install its hooks and the MCP entry. The hooks are how poimen knows whether an agent works, waits or is done. *Not now* is remembered; you can do it later under **Settings > Backend > Server**.
3. Create your first space: the **+** button in the Spaces header takes a directory (completes as you type, or use *Browse*) and a label.

A space is one project directory with its terminals. If a repository has a project file (`.poimen/config.json`), poimen asks before it runs anything the file contains, see [Spaces and layouts](/guide/spaces#project-files-and-trust).

## Your first agent

A new space opens with the layout you configured, by default a shell and an agent pane per installed agent CLI. In a shell pane, right-click the pane and pick **Start agent...**, or start `claude` or `copilot` yourself. Once its hooks have reported, the sidebar row shows the agent and its status.

Leave the tab and do something else. When the agent needs you, its tab and sidebar row ring; when it is done, the row counts as unread. `CmdOrCtrl+Shift+A` jumps to the next waiting agent. More in [Agents](/guide/agents).

## Where to go next

- [Spaces and layouts](/guide/spaces): sidebar, tabs, splits, templates and the project file.
- [Agents](/guide/agents): status, attention, prompts, subagents, tokens and hibernation.
- [GitHub and tasks](/guide/github): pull requests, issues, milestones and starting a task from an issue.
- [Runs](/guide/runs): the deliver loop and other agent runs.
- [Remote machines](/guide/remote-machines): spaces and agents on another computer over ssh.
- [Console and MCP](/guide/console): the `poimen` TUI and CLI, and the MCP server for agents.
- [Settings](/guide/settings): the options you will change first.
