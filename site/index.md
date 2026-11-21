---
layout: home
hero:
  name: poimen
  text: A desktop window for your terminal agents
  tagline: Watch and steer your AI coding agent sessions with editor tabs, menus and a status sidebar. poimen-server keeps the herd, poimen is the window you watch it through.
  image:
    src: /logo.png
    alt: poimen
  actions:
    - theme: brand
      text: Download
      link: /download
    - theme: alt
      text: Installation
      link: /guide/installation
    - theme: alt
      text: Guide
      link: /guide/getting-started
features:
  - title: Spaces sidebar
    details: Every space with its terminals, agent status, branch and worktree, filterable by name, agent or branch.
  - title: Live terminals in tabs
    details: A pane opens as an editor tab with a real terminal. Closing a tab never closes the pane or its agent.
  - title: Agent status and attention
    details: Blocked and done agents ring on their tab and sidebar row, with unread counts, notifications and a dock badge.
  - title: Subagents and Flow
    details: Read-only view of the subagents of Claude Code and GitHub Copilot CLI, and the run drawn as a graph.
  - title: GitHub and tasks
    details: Pull requests, issues and milestones in one tab. Start a task from an issue in its own worktree with an agent.
  - title: Runs
    details: "Follow deliver loops and other agent runs: what works now, what needs you, and what finished."
  - title: Remote machines
    details: Spaces and agents of another computer next to yours, over ssh, without an open port.
  - title: Console and MCP
    details: The same terminals from a shell or a phone with the poimen TUI and CLI, and an MCP server so agents can lead other agents.
  - title: Color schemes
    details: Bundled and imported WezTerm color schemes, light and dark, with a chrome derived from the scheme.
---

## What is poimen

*Poimen* (ποιμήν) is Greek for shepherd. `poimen-server` runs your terminals and AI coding agents as persistent terminals that outlive the app. poimen is a desktop app (Tauri, with an Angular frontend) that shows those terminals the way an editor shows files: a sidebar of spaces, tabs for the panes, a status bar, a command palette and menus.

poimen does not keep any pane state of its own. `poimen-server`, bundled with the installers, is the backend and the only source of truth: poimen reads its snapshots and events and attaches to its terminals. Closing poimen only closes the view; the terminals and their agents keep running.

<Screenshot src="sidebar-dark.png" light="sidebar-light.png" alt="The poimen window with the Spaces sidebar as cards: poimen working, billing-api waiting for permission, docs done and infra idle, each with its panes and agent status, and two remote machines below" caption="The poimen window: space cards on the left with working, waiting and done agents, and remote machines below (demo data, follows the site theme)." />

## Download

<Downloads />

New here? Read the [installation guide](/guide/installation): no other tool is needed, and the installers are not code-signed yet.

## Examples

### Spaces and live terminals

The sidebar lists the spaces as a tree of spaces with their panes. Each row shows the agent, its status, the branch and the directory, and you choose what a row shows. A pane opens as an editor tab with one live terminal (xterm.js) for the active tab. By default it opens for input; if another client already controls the terminal, the tab falls back to read-only and offers an explicit *Take over*.

<Screenshot src="tabs-dark.png" light="tabs-light.png" alt="An editor tab with a live Claude Code session next to a shell in a split" caption="A pane opens as an editor tab with a live terminal; a tab with several panes shows its split (demo data, follows the site theme)." />

### Agent status

Statuses come from the agent hooks that report to `poimen-server`. A pane that turns blocked while you are not looking rings on its tab and sidebar row, one that turns done counts as unread, and viewing the tab clears both. "Next waiting agent" jumps to the next blocked agent, and the dock icon shows the number of waiting agents.

<Screenshot src="blocked-dark.png" light="blocked-light.png" alt="A blocked agent: the space card is marked waiting: permission and the terminal shows the question Claude Code asks, with a 1 waiting count in the status bar" caption="A blocked agent is marked on its space card and row; the status bar counts the waiting agents (demo data, follows the site theme)." />

### Subagents and the Flow tab

The Output panel (`Cmd/Ctrl+J`) reads the session transcripts of Claude Code and GitHub Copilot CLI, read-only, and lists what the subagents of the active agent are doing. The **Flow** tab draws the run as a graph, laid out automatically: the main agent, every skill call and every subagent, nested under the one that called them, with status per node.

<Screenshot src="flow-dark.png" light="flow-light.png" alt="The Flow tab in the bottom panel: the main agent node connected to a github-issue skill, an implementation skill and a started agent node named review" caption="The Flow tab draws a run as a graph: the main agent, the skills it calls and the agents it started, with status per node (demo data, follows the site theme)." />

A longer run shows the nesting: the main agent starts subagents (implement, review, fix), and each subagent lists the skills it called. A node shows its status, duration, tool calls, token usage and the context size at its start.

<Screenshot src="flow-subagents-dark.png" light="flow-subagents-light.png" alt="The Flow tab with a run of one main agent, subagents for implementing, reviewing and fixing, and the skills implementation and code-review nested under them, each with status, duration, tool calls and tokens" caption="A run with subagents and skills nested under the agent that called them (running nodes are outlined, follows the site theme)." />

### Color schemes

Light, dark or follow the system. Choose a bundled scheme, or drop WezTerm color scheme files into `~/.config/poimen/themes/`; the window chrome is derived from the scheme.

<Screenshot src="schemas-dark.png" light="schemas-light.png" alt="The settings view with the appearance switch and the color scheme picker listing dark and light schemes with their palettes" caption="Appearance and color scheme in the settings view (demo data, follows the site theme)." />

## Learn more

- [Getting started](/guide/getting-started), then the guide pages for [spaces](/guide/spaces), [agents](/guide/agents), [GitHub](/guide/github), [runs](/guide/runs), [remote machines](/guide/remote-machines), the [console and MCP](/guide/console) and [settings](/guide/settings)
- [Installation](/guide/installation) per platform
- [Source and issues on GitHub](https://github.com/GravionLabs/poimen)
