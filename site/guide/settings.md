# Settings

Settings are optional. They live in `~/.config/poimen/config.json` (comments are allowed) and apply without a restart. The gear at the bottom of the activity bar opens the **settings view** with a category tree and a search field (`Cmd/Ctrl+F`). Poimen writes only the entries you change there and keeps the rest of the file and its comments. UI state such as the sidebar width and the open tabs is kept by the app, not in this file.

## Appearance

<Screenshot src="schemas-dark.png" light="schemas-light.png" alt="The settings view with the appearance switch and the color scheme picker listing dark and light schemes with their palettes" caption="Settings > Appearance: the theme switch and the color scheme picker (demo data)." />

- `theme`: `light`, `dark` or `system`. The button in the top bar cycles them.
- `colorSchema`: a bundled scheme, or a pair `{ "light": "...", "dark": "..." }`. Bundled: `poimen` (default), `catppuccin`, `tokyo-night`, `dracula`, `gruvbox`, `one-dark`, `solarized` and more. Your own WezTerm color schemes go into `~/.config/poimen/themes/` and are picked up without a restart.
- `terminal.fontFamily`, `terminal.fontSize` (6 to 72), `terminal.scrollback` (default 10000) and `terminal.ghostty` to take font and colors from your Ghostty config.
- `sounds`: a tone or sound file for blocked and done agents.

## Terminals and keys

- `terminal.autoControl`: `true` (default) opens panes for input when nobody else controls them; `false` opens every tab read-only.
- `shortcuts`: an accelerator per action, for example `"toggleSidebar": "Ctrl+Alt+B"`. An empty string unbinds it. The `?` button in the status bar (`CmdOrCtrl+/`) lists all shortcuts with your bindings, and the command palette is `CmdOrCtrl+Shift+P`.

## Spaces and the sidebar

- `spaces.layout`, `layouts` and `spaceTemplates`: what a new space contains, see [Spaces and layouts](/guide/spaces#layouts-and-templates).
- `explorer.roots` and `explorer.showHidden`.
- `sidebar.layout` (`tree`, `split`, `cards`), `sidebar.sort` (`manual`, `name`, `attention`, `recent`), `sidebar.groupWorktrees` and `sidebar.rows`, which says per row which tokens it shows (`state_icon`, `name`, `agent`, `branch`, `dirty`, `sync`, `pr`, `dir` and tokens your scripts set with `poimen-server report tokens`).

## Agents

- `harnesses`: the agent CLIs poimen can start, with their model and prompt arguments. `claude` and `copilot` are built in.
- `agents.silentAfter`, `agents.keepAwake`, `agents.hibernateAfter` and `agents.claude.classicRenderer`, see [Agents](/guide/agents).
- `usage.claude`: the Claude limits in the status bar.
- `github.agentActions` and `tasks`: see [GitHub and tasks](/guide/github).

## Machines, trust and console

<Screenshot src="settings-server-dark.png" light="settings-server-light.png" alt="Settings, Backend, Server: the version, state, socket and log of poimen-server, and the hook and MCP state of Claude Code and Copilot CLI" caption="Settings > Backend > Server: the server, and the hooks and MCP entries of your agents (demo data)." />

- `machines`: see [Remote machines](/guide/remote-machines).
- `trustedProjects`: managed by the trust dialog and **Settings > Workspace > Trusted projects**, never imported.
- `console.prefix` and `console.keys`: the TUI keys, see [Console and MCP](/guide/console).

**Settings > General > Export & import** moves your settings to another machine as a JSON file. Import shows what it would add, change and remove before it writes anything.

The README keeps the complete list of every key and its rules.
