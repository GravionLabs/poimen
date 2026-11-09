# ADR 0006: poimen console with herdr scope next to the app

- Status: Proposed
- Date: 2026-11-08
- Issues: #172, Feature #170
- Supersedes: [ADR 0003](0003-own-terminal-backend-without-herdr.md) (in part: the TUI and CLI exclusion and "the app owns layout")
- Builds on: [ADR 0003](0003-own-terminal-backend-without-herdr.md), [ADR 0005](0005-github-views-and-agent-harnesses.md)

## Context

ADR 0003 gave poimen its own terminal backend, `poimen-server`, and left a TUI and a CLI out of scope. The model it chose is flat: spaces (name, cwd) with terminals. Tabs and splits stayed in the poimen UI, under the rule "the backend owns processes, the app owns layout".

What the code does today:

- `poimen-server` (protocol 3, `docs/protocol.md`) knows `space.*` and `terminal.*`, attach with snapshot and sequence numbers, agent status and tokens. Only the space list persists (`server.json`).
- `PoimenBackend::map_snapshot` turns every terminal into a tab with one pane, in herdr's wire shape. `split` ignores direction and ratio and only creates another terminal in the space.
- The app's editor area (`core/editor-tabs.ts`) is a tree of editor groups with split sizes. It is persisted as UI state (`UiState`) and mixes terminal tabs with GitHub, Changes, Flow and Deliver tabs.
- `layout::resolve_origin` and `layout::apply` build a new space's panes (project file `.poimen/config.json`, deprecated `.agents/herdr-layout.json`, template, `spaces.layout`, default). `trust::gate` decides `Run`/`Strip`/`Ask` against `trustedProjects`. `templates`, `task_start`, `tasks`, `worktree`, `project` and `github` sit in the app crate as well. None of them imports Tauri; they depend on the `Backend` trait, `settings`, `harness`, `gui_path`, `git` and `remote`.
- Agents can no longer steer other agents on the poimen backend, since `herdr agent prompt` has no counterpart (ADR 0003, Consequences).

The owner wants poimen to be usable from a terminal with the scope of herdr: a TUI with spaces, agent status, tabs, splits, detach and a prefix key, and a CLI for scripts and agents. An earlier plan for #170 (CLI and single-terminal attach only) is replaced. The decisions are listed in #170.

## Decision

poimen gets a console next to the app: one `poimen` binary with a TUI and CLI subcommands. It is a second client of `poimen-server` over the same documented protocol and `poimen-client`. To give app and TUI the same tabs and splits, the layout moves into the server.

### Binaries and packaging

| Binary | Role | On the `PATH` |
|---|---|---|
| `poimen-app` | Tauri GUI (today `poimen`) | no |
| `poimen` | TUI (no arguments) and CLI (subcommands) | yes, the only one |
| `poimen-server` | daemon, hooks (`hook`, `report`) | no |

- Display name `Poimen` everywhere (`productName`), identifier `dev.gravionlabs.poimen` unchanged.
- The app bundle ships `poimen` next to `poimen-server` (sidecar, like today's `externalBin`). Linux packages: `poimen` (app, server and console) and `poimen-cli` (console and server only, for machines without a desktop). macOS: the app offers to link `poimen` into `/usr/local/bin`; Windows: the installer adds the console directory to the user `PATH`.
- New crate `poimen-console` in `src-tauri/crates/` builds the `poimen` binary (ratatui and crossterm live only there).

### CLI

- `poimen ls`, `poimen new <space> [-- cmd]`, `poimen send <terminal> <text>`, `poimen prompt <terminal> <text>` (same rules as `agent.prompt`: refused while working or blocked), `poimen status <terminal>`, `poimen attach <terminal>`; `--json` on every read command (#175, #176).
- Hook installation moves from `poimen-server` to the CLI: `poimen integration install|uninstall|status claude|copilot`. The hooks keep calling `poimen-server hook ...`, so the daemon stays the only binary the hooks need. `poimen-server integration` stays one release as a deprecated alias (amends #154/#155).
- Socket: `POIMEN_SOCKET` inside a poimen terminal, else the default socket path of `poimen-protocol` (Windows: the named pipe and its marker file).

### Attach, size and keys

- `poimen attach` observes by default; `--control` takes input, `--takeover` takes over an existing controller (explicit, as ADR 0003 requires). Snapshot first, then raw bytes; the local terminal is in raw mode and restored on detach, exit and signals.
- Size rule (#152): the controller sets the PTY size; observers render at that size (clipped or padded).
- Prefix key like herdr and tmux: one prefix, then a key for splits, tabs, focus, palette and detach. Default `Ctrl+B`, configurable in `~/.config/poimen/config.json` under `console.prefix` and `console.keys`. Pressing the prefix twice sends it to the terminal. Every other key goes to the focused terminal unchanged, so agent TUIs and editors keep their shortcuts. Detach is `prefix d` in the TUI and in `poimen attach`.

### Layout in the server

The server owns tabs and the split tree per space, persisted with the spaces in `server.json`. Terminals are still not persisted. After a server restart, a tab's leaves refer to terminals that are gone; the server drops those leaves and empty tabs.

- New object `Tab`: `{tab_id, space_id, name, root}`. `root` is a node: `{kind: "terminal", terminal_id}` or `{kind: "split", split_id, direction: "right"|"down", ratio, children: [node, node]}`. `split_id` is assigned by the server, unique within it, stable for the life of the split, persisted with the tree and never reused (the id counter is persisted too). `Space` gains `tabs: [tab_id]` in order and `active_tab`; `Tab` also carries `active_terminal`, the focused leaf `tab.focus` sets (amended 2026-11-09, #510).
- Requests: `tab.create {space_id, name?, terminal_id?}` (a new terminal when none is given), `tab.rename`, `tab.close` (closes its terminals), `tab.move {tab_id, index}`, `tab.split {terminal_id, direction, ratio?, cwd?, name?, command?}` (creates a terminal and splits the leaf), `tab.resize_split {tab_id, split_id, ratio}`, `tab.focus {tab_id, terminal_id?}`.
- `terminal.create` without a tab creates a tab for it, so existing callers keep working. Closing a terminal, or its exit, removes its leaf and collapses the split; the last terminal of a tab closes the tab.
- Events: `tab.created`, `tab.changed` (the full tab after any tree, name, order or focus change), `tab.closed`; `space.changed {space}` carries the new `tabs` order and `active_tab`, which `tab.changed` alone cannot (amended 2026-11-09, #510). A layout change and its events happen under the same lock as the terminal change.
- Focus is shared: the last client that focuses wins, the same as herdr. A client that only views (the app with a different tab open) does not send focus.
- Splits are addressed by `split_id`, not by a path in the tree (amended 2026-11-09, #510): app and TUI change the same tree concurrently, and a path shifts when another client splits or closes a pane. An unknown `split_id` is an error. `tab.move` is part of the first protocol 4 PBI (#510), so the tab order lives in the server and no longer in the app's UI state.
- `PROTOCOL_VERSION` goes to 4; the new shapes get parsing tests in `poimen-protocol`. A client checks the version through `ping`.

#### Migration of the app

- `map_snapshot` maps server tabs one to one. A tab's split tree becomes herdr's `pane.list` with one pane per leaf, so `Snapshot` and `toSnapshot` keep their shape. `PoimenBackend::split` calls `tab.split` with direction and ratio instead of creating a loose terminal.
- The app's editor groups stay UI state. They hold app-only tabs (GitHub, Changes, Flow, Deliver) that the server never sees. A server tab is one editor tab, and its split tree renders inside it from the server state. The app no longer keeps split sizes of terminals of its own.
- Existing spaces after the upgrade: the server creates one tab per terminal that has no tab (today's mapping), so nothing changes visibly until a user splits.

### Who applies a space's layout

The server, through `space.create {name, cwd, layout_from?, trust?}`. The precedence of today's `create_space_with` stays: task command (root pane) > project file > deprecated `.agents/herdr-layout.json` > template > `spaces.layout` > default. The server reads the project files once into `trust::Sources` and runs `layout::resolve_origin`, `templates::resolve` and `trust::gate` on that read. It then builds the tree with the new tab requests internally. A `Backend` call per pane is no longer needed. `layout::apply` moves into the server and builds the tree directly.

Trust handshake:

1. `space.create` without `trust` and a project file whose commands are not trusted answers `{trust_required: TrustRequest}` (`root`, `files`, `hash`, `commands`) and creates nothing.
2. The client asks the user (app: `trust-dialog`; TUI: a prompt with the commands) and sends `space.create` again with `trust: {kind: "trust", hash}` or `{kind: "without"}`.
3. `Gate::Run { store: Some(hash) }` makes the server write the hash to `trustedProjects` of the user config, through `settings::trust_project` (comment-keeping, moved to `poimen-core`), as the app does today. `trustedProjects` stays the single source; a hash that no longer matches asks again.

### Shared code: `poimen-core`

New Tauri-free crate `src-tauri/crates/poimen-core`, used by the app, the console and the server:

- Moves: `layout`, `trust`, `templates`, `remote`, `project` (parse, load, entries, setup/teardown plans), `tasks` (modes, `TaskVars`), `task_start` (`plan`, `space_label`), `worktree`, `github` (client, `open_issues`, `level_of`), `git` (read-only helpers), `harness`, `gui_path` (child `PATH`), and the parts of `settings` these need (`parse_tasks`, `parse_space_templates`, `trustedProjects`, harnesses, `console.*`).
- Stays in the app crate: Tauri commands, watchers that emit Tauri events, `usage`, `transcripts`, `copilot` (transcripts), `changes`, `changes_git`, `deliver`, `updater`, `schemes`, `hooks.rs` (it runs `poimen integration` through `Exec`), the `Backend` implementations.
- Functions that today take `&dyn Backend` (`project::start_tab`, `task_start::start_tab`, `worktree::cleanup`'s close step) take a small trait of `poimen-core` instead. `poimen-client` implements it; the app's backend trait delegates to it.
- `poimen-server` depends on `poimen-core` only for layout, trust, templates and project parsing. Setup runs in a terminal of the new space, as today (`project::start_setup`); teardown and task cleanup stay client-side.

### TUI

- Layout: sidebar on the left (spaces with agent status dot, name, branch and the `sidebar.rows` tokens of the app), tab bar on top, the split tree of the active tab below, status line at the bottom. The sidebar can be hidden (`prefix b`).
- Rendering: per visible leaf an `alacritty_terminal` grid in the client, fed by `terminal.attach` (snapshot, then bytes), drawn with ratatui. The TUI attaches as controller to the focused leaf and as observer to the others. Moving the focus moves control, but never takes over a controller another client holds without `--takeover` or a confirm prompt.
- Palette (`prefix p`): create, close and rename of spaces, tabs and terminals; task start (issue list from `github::open_issues`, task modes, `worktree::task_worktree`, then `space.create`); project entries (`Project: <name>`, `Preset: <name>`, through `trust::gate`); setup and teardown with the same trust handshake.
- Notifications: blocked and done marks on rows and tabs, as `core/attention.ts` does in the app; a notification list (`prefix n`); system notifications under `notifications.blocked` / `notifications.done`. The app has priority: the TUI sends a system notification only while no app client is connected.
- App-only, never in the TUI: GitHub views, Changes, Flow, Deliver, settings view, color scheme editing.
- Windows from the start: crossterm on the Windows console, named pipe transport of `poimen-client`; raw mode and resize events through crossterm; a ConPTY in pass-through mode on the server side, as today. Known gaps go to the manual acceptance (`docs/manual-acceptance.md`).

### Client kinds

`events.subscribe` takes an optional `client: {kind: "app"|"tui"|"cli", pid}`. The server keeps the kinds of the subscribed connections, reports them in `ping` as `clients: [{kind, pid}]` and emits `server.clients_changed {clients}`. The TUI uses this for the notification priority. Nothing else depends on it.

### Order of work

1. `poimen-core` extraction (no behavior change).
2. Protocol 4: tabs and split tree, persistence, app migration.
3. `poimen` CLI (#175) and `poimen attach` (#176).
4. Server-side layout and trust handshake.
5. TUI: sidebar, tabs, splits, keys; then palette, task start, project entries, notifications.

### Unchanged

- The herdr backend is not affected by this ADR. It stays selectable but is in maintenance only: bugfixes, no feature parity with the poimen backend. It is removed with #174 once the PTY host (#574) and the TUI (#515) are released and poimen was the default for one release without problems (decision 2026-11-09, see #169).
- `poimen-server` is never stopped by a client. Closing the TUI, or detaching, closes only the view; terminals and agents keep running.
- Tests stay read-only toward the user's server; console tests run against a fake server on a temporary socket.

## Consequences

- poimen covers herdr's core in a terminal: SSH sessions, quick looks and agents steering agents (`poimen prompt`) work without the app.
- One layout for app and TUI. A split made in the TUI shows in the app and the other way round.
- The rule "the backend owns state" now includes tabs and splits again; ADR 0003's "the app owns layout" is superseded. The app's editor groups remain UI state for app-only tabs.
- Protocol 4 is a breaking change between app and server versions. The app and the server are released together (one workspace version). A server left running from an older version answers `ping` with protocol 3, which `poimen_server::ping` treats as not answering. Since poimen never stops a server, the upgrade path for such a server (a user-confirmed restart that loses its terminals) is part of the protocol 4 work.
- The server reads the user config (`trustedProjects`, templates, `spaces.layout`) and writes `trustedProjects`. Two writers of `config.json` (app and server) must use the same comment-keeping writer and re-read before writing.
- `poimen-core` is a large refactor of the app crate before any visible feature; it has to keep every existing test green.
- A third binary to sign, notarize (#45) and package on three OSs; `poimen-cli` is a new Linux package.
- The TUI duplicates some app logic in Rust that the frontend has in TypeScript (attention marks, sidebar row tokens). Pure parts move into `poimen-core` where the app can call them through commands; the rest stays duplicated and is tested on both sides.

## Alternatives

- **CLI and single-terminal attach only** (the earlier scope of #170). Small, but it does not replace herdr in a terminal: no sidebar, no tabs, no splits.
- **Layout per client.** No protocol change, but app and TUI show different arrangements of the same space, and a split made in one client is a loose terminal in the other.
- **Server-rendered cells.** A protocol stream of finished cells per pane. Simpler client, but a larger protocol, more server work and a second rendering path next to the raw byte stream the app already uses.
- **Modal keys** (a navigation mode like vim). More powerful, but harder to learn and unlike herdr. **Direct shortcuts without a prefix** collide with agent TUIs and editors.
- **tmux or dtach style raw passthrough only.** No own rendering, but no sidebar with agent status and no splits across the server's terminals.
- **Task start and worktrees as server requests.** The daemon would run `gh` and `git` with the user's login. Keeping them in `poimen-core` on the client side keeps the server small and its failure modes local.
- **No console.** Nothing to build, but poimen stays tied to a desktop session, and agents keep no way to steer other agents on the poimen backend.
