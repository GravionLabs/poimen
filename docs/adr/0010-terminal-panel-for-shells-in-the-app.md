# ADR 0010: Terminal panel for shells in the app

- Status: Proposed
- Date: 2026-11-13
- Builds on: [ADR 0006](0006-poimen-console-with-herdr-scope.md)

## Context

ADR 0006 moved tabs and split trees into `poimen-server`, so the app and the TUI show the same layout. The app renders a split tab inside one editor tab (`src/app/tab-layout/`, #514). The layouts that build a space put agents and shells into the same tab: the seeded layout (`templates::seeded`) puts the agents side by side and a `shell` pane below them (ratio 0.7), and the built-in `default_layout` puts `agent` left and `tests` and `shell` right.

In the app this has costs:

- The shell belongs to the tab. Opening Changes, a GitHub view, Deliver or another agent's tab hides it, although "read the diff while the tests run" is a common case.
- A split to the right takes width from the agent. Claude Code and Copilot CLI are TUIs that need it.
- The common case, "an agent and one shell in the same directory", needs a layout file, the layout precedence (project file > template > `spaces.layout` > default), seeding and trust.
- Each leaf has its own stream, size, focus (Alt+arrows) and context menu.

The app already has a bottom panel (`src/app/bottom-panel/`: Output, Flow, Events, Log) next to the editor area, the same place VS Code keeps its terminals.

The TUI has no such panel. There a split is the only way to see several terminals at once. The rule "the backend owns state" forbids the app from deciding on its own, in `UiState`, which terminal goes into the panel.

## Decision

Shells go into a terminal panel at the bottom of the app by default. Agents stay editor tabs. Splits stay in the server, in the TUI and, as an option, in the app.

### Placement in the server

- `Tab` gets `placement: "editor" | "panel"`, default `editor`. It is persisted in `server.json` with the tab, and a missing field reads as `editor`, so existing spaces and older stored files keep their look.
- `tab.create` and `tab.split` take an optional `placement`; a new request `tab.place {tab_id, placement}` moves a tab between the two. `tab.changed` carries the new value. A split stays inside its tab: a leaf has no placement of its own.
- `PROTOCOL_VERSION` goes up by one; the field gets a parsing test in `poimen-protocol` and is documented in `docs/protocol.md`.

### Layout files

- A `PaneSpec` without `split` other than the root pane may set `"panel": true`. It then becomes a tab of its own with `placement: "panel"` instead of a leaf of the root tab. `panel` together with `split` is a parse error that names the pane.
- The seeded layout becomes: the agents as today (side by side in the root tab) and `shell` as a panel tab. `default_layout` gets the same change for `shell` and `tests`. Existing `spaces.layout`, templates and project files keep their splits unchanged; nothing is rewritten.

### App

- The bottom panel gets a "Terminal" tab. It lists the panel tabs of the active space (one sub-tab each, named by the tab), renders the active one with `terminal-view` and has a "New terminal" button (`tab.create` with `placement: "panel"` and the space's `cwd`).
- Editor tabs show only `editor` tabs. "Move to panel" and "Move to editor" in the tab's context menu send `tab.place`.
- Which panel tab is active and the panel height are UI state, the same as today's bottom panel; which tabs are panel tabs is not.
- The panel keeps the streams of its tabs attached while it is hidden, like an editor tab in the background, so switching does not cost a new snapshot.
- Split rendering in the editor (#514) stays for users who want several terminals in view.

### TUI

The TUI shows the panel tabs of the active space as a bottom area below the active editor tab, with a key to show or hide it. Until that is built it lists them as ordinary tabs, so no terminal is out of reach.

## Consequences

- A shell stays visible while the editor shows a diff, a pull request or another agent. Agents get the full width.
- The common case needs no layout: a new space has its agent in the editor and a shell in the panel.
- Layout stays server state, so the placement survives restarts and is the same for the app, the TUI and later clients.
- App and TUI differ in how they draw a panel tab. The model they read is the same.
- The bottom panel holds more tabs. The terminal and Flow or Events cannot be seen at the same time.
- Two agents side by side still need a split or editor groups; this ADR does not change that.
- Protocol change between app and server, released together as with every protocol bump.

## Alternatives

- **Keep splits as the only model.** Already built and the same in the TUI, but the shell disappears on every editor tab switch and the agent loses width. Rejected for the default; kept as an option.
- **Panel only, splits removed from the app.** Simpler UI, but the TUI needs splits, the layout files already use them and #514 is shipped. Removing them breaks existing layouts.
- **Panel assignment in the app's `UiState`.** No protocol change, but it breaks "the backend owns state": the assignment would be lost on another machine or client, and the TUI could not see it.
- **One fixed shell per space in the panel, without tabs.** Too narrow: dev servers, tests and shells usually run side by side.
