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

<Screenshot src="sidebar-dark.png" alt="The poimen window with the Spaces sidebar: three workspaces, api-gateway working, web-app blocked and docs-site done, each with its panes and agent status" caption="The poimen window: spaces on the left with working, blocked and done agents (demo data)." />

## Download

<Downloads />

New here? Read the [installation guide](/guide/installation): herdr is a prerequisite, and the installers are not code-signed yet.

## Examples

### Spaces and live terminals

The sidebar lists the herdr workspaces as a tree of spaces with their panes. Each row shows the agent, its status, the branch and the directory, and you choose what a row shows. A pane opens as an editor tab with one live terminal (xterm.js) for the active tab. By default it opens for input; if another client such as the herdr terminal UI already controls the pane, the tab falls back to read-only and offers an explicit *Take over*.

<Screenshot src="tabs-dark.png" light="tabs-light.png" alt="Editor tabs with a live terminal of a Claude Code session; the IN badge marks input tabs" caption="Editor tabs with live terminals; IN and RO mark input and read-only tabs (demo data, follows the site theme)." />

### Agent status

Statuses come from herdr. A pane that turns blocked while you are not looking rings on its tab and sidebar row, one that turns done counts as unread, and viewing the tab clears both. "Next waiting agent" jumps to the next blocked agent, and poimen can send a macOS notification and show the number of waiting agents on the dock icon.

<Screenshot src="blocked-dark.png" alt="A blocked agent: its tab and sidebar row are ringed and the terminal shows the question Claude Code asks, with a 1 waiting count in the status bar" caption="A blocked agent rings on its tab and sidebar row; the status bar counts the waiting agents (demo data)." />

### Subagents and the Flow tab

The Output panel (`Cmd/Ctrl+J`) reads the session transcripts of Claude Code and GitHub Copilot CLI, read-only, and lists what the subagents of the active agent are doing. The **Flow** tab draws the run as a graph, laid out automatically: the main agent, every skill call and every subagent, nested under the one that called them, with status per node.

<Screenshot src="flow.svg" alt="Placeholder for a screenshot of the Flow tab" caption="The Flow tab of a run with parallel subagents (screenshot pending, see #264)." />

### Deliver board

With the herdr `deliver-board` plugin installed, a native tab shows one table per repository: the open PBIs, their claim, labels, branch and pull request with check state, and a button to start the deliver run.

<Screenshot src="deliver.svg" alt="Placeholder for a screenshot of the Deliver board tab" caption="The native Deliver board (screenshot pending, see #264)." />

### Color schemes

Light, dark or follow the system. Choose a bundled scheme, use the one of your herdr configuration, or drop WezTerm color scheme files into `~/.config/poimen/themes/`; the window chrome is derived from the scheme.

<Screenshot src="schemas-dark.png" light="schemas-light.png" alt="The settings view with the appearance switch and the color scheme picker listing dark and light schemes with their palettes" caption="Appearance and color scheme in the settings view (demo data, follows the site theme)." />

## Learn more

- [Features](/guide/features) in detail
- [Installation](/guide/installation) per platform
- [Decisions](/adr/): the architecture decision records
- [Source and issues on GitHub](https://github.com/GravionLabs/poimen)
