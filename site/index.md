---
layout: home
hero:
  name: poimen
  text: A desktop window for your herdr agents
  tagline: Watch and steer your AI coding agent sessions with editor tabs, menus and a status sidebar. herdr keeps the herd, poimen is the window you watch it through.
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
      text: Features
      link: /guide/features
features:
  - title: Spaces sidebar
    details: Every herdr workspace with its panes, agent status, branch and worktree, filterable by name, agent or branch.
  - title: Live terminals in tabs
    details: A pane opens as an editor tab with a real terminal. Closing a tab never closes the pane or its agent.
  - title: Agent status and attention
    details: Blocked and done agents ring on their tab and sidebar row, with unread counts, notifications and a dock badge.
  - title: Subagents and Flow
    details: Read-only view of the subagents of Claude Code and GitHub Copilot CLI, and the run drawn as a graph.
  - title: Deliver board
    details: A native tab for the herdr deliver-board plugin, one table per repository with PBIs, branches and pull requests.
  - title: Color schemes
    details: Bundled and imported WezTerm color schemes, light and dark, with a chrome derived from the scheme.
---

## What is poimen

*Poimen* (ποιμήν) is Greek for shepherd. [herdr](https://herdr.dev) runs your terminals and AI coding agents as persistent panes. poimen is a desktop app (Tauri, with an Angular frontend) that shows those panes the way an editor shows files: a sidebar of spaces, tabs for the panes, a status bar, a command palette and menus.

poimen does not keep any pane state of its own. herdr is the backend and the only source of truth: poimen reads its snapshots and events and attaches to terminals through the public `herdr terminal session` CLI. Closing poimen only closes the view; the panes and their agents keep running, and the herdr terminal UI keeps working next to it.

<Screenshot src="spaces.svg" alt="Placeholder for a screenshot of the poimen window: spaces sidebar and an agent tab" caption="The poimen window: spaces on the left, the active pane as a tab (screenshot pending)." />

## Download

<Downloads />

New here? Read the [installation guide](/guide/installation): herdr is a prerequisite, and the installers are not code-signed yet.

## Examples

### Spaces and live terminals

The sidebar lists the herdr workspaces as a tree of spaces with their panes. Each row shows the agent, its status, the branch and the directory, and you choose what a row shows. A pane opens as an editor tab with one live terminal (xterm.js) for the active tab. By default it opens for input; if another client such as the herdr terminal UI already controls the pane, the tab falls back to read-only and offers an explicit *Take over*.

<Screenshot src="tabs.svg" alt="Placeholder for a screenshot of editor tabs with live terminals" caption="Editor tabs with live terminals; IN and RO mark input and read-only tabs (screenshot pending)." />

### Agent status

Statuses come from herdr. A pane that turns blocked while you are not looking rings on its tab and sidebar row, one that turns done counts as unread, and viewing the tab clears both. "Next waiting agent" jumps to the next blocked agent, and poimen can send a macOS notification and show the number of waiting agents on the dock icon.

<Screenshot src="agents.svg" alt="Placeholder for a screenshot of agent status marks in the sidebar" caption="Working, blocked and done agents in the sidebar (screenshot pending)." />

### Subagents and the Flow tab

The Output panel (`Cmd/Ctrl+J`) reads the session transcripts of Claude Code and GitHub Copilot CLI, read-only, and lists what the subagents of the active agent are doing. The **Flow** tab draws the run as a graph, laid out automatically: the main agent, every skill call and every subagent, nested under the one that called them, with status per node.

<Screenshot src="flow.svg" alt="Placeholder for a screenshot of the Flow tab" caption="The Flow tab of a run with parallel subagents (screenshot pending)." />

### Deliver board

With the herdr `deliver-board` plugin installed, a native tab shows one table per repository: the open PBIs, their claim, labels, branch and pull request with check state, and a button to start the deliver run.

<Screenshot src="deliver.svg" alt="Placeholder for a screenshot of the Deliver board tab" caption="The native Deliver board (screenshot pending)." />

### Color schemes

Light, dark or follow the system. Choose a bundled scheme, use the one of your herdr configuration, or drop WezTerm color scheme files into `~/.config/poimen/themes/`; the window chrome is derived from the scheme.

<Screenshot src="schemes.svg" alt="Placeholder for a screenshot of the color scheme settings, light and dark" caption="Color schemes in the settings view (screenshot pending)." />

## Learn more

- [Features](/guide/features) in detail
- [Installation](/guide/installation) per platform
- [Decisions](/adr/): the architecture decision records
- [Source and issues on GitHub](https://github.com/GravionLabs/poimen)
