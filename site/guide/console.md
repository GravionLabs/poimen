# Console

The `poimen` binary is a second client of the same `poimen-server` the app uses. It shows and steers the same spaces, tabs and terminals from a shell: over ssh, in a terminal next to the app, or on a machine without a desktop. Everything you do in it goes through the server, so the app and the console always show the same layout, focus and agent status.

The binary has two faces:

- **`poimen`** without arguments opens the [TUI](#the-tui), a full-screen text UI.
- **`poimen <command>`** runs one [CLI command](#the-cli) and exits, for scripts and agents.

For agents there is also `poimen mcp`, an MCP server; it has a page of its own: [MCP server](/guide/mcp).

## Setup

### Where the binary is

[Installation](/guide/installation#console) lists it per system. In short: every app installer ships `poimen`; on Linux the app's `.deb` and `.rpm` install `/usr/bin/poimen`.

### The server must run

The console talks to a running `poimen-server` and never starts one itself. Next to the app this is automatic: the app starts the server, and it keeps running after you close the app.

On a machine without the app (a server, a VM) install the `poimen-cli` package of the release (`.deb` or `.rpm`, it conflicts with the app package). It holds the console `poimen`, the server `poimen-server` and its PTY host `poimen-pty-host`, all in `/usr/bin`. It starts no server: start one per user yourself.

```sh
nohup poimen-server >/dev/null 2>&1 &   # one per user; a second start exits
poimen doctor
```

Installing a newer package over a running server keeps its terminals; the new server runs after you stop the old one and start it again.

Without the package (another system, or no root), download `poimen-<version>-<target>.tar.gz` (the console only) and `poimen-server-<version>-<target>.tar.gz` (Linux, x86_64 and aarch64; it holds `poimen-server` and `poimen-pty-host`, which must stay in the same directory):

```sh
mkdir -p ~/.local/bin
tar -xzf poimen-server-0.32.1-x86_64-unknown-linux-musl.tar.gz -C ~/.local/bin
nohup ~/.local/bin/poimen-server >/dev/null 2>&1 &
```

The app can also install the server on a machine for you over ssh, see [Remote machines](/guide/remote-machines).

### Which server it talks to

Inside a poimen terminal the console uses the server of that terminal (`POIMEN_SOCKET`). Everywhere else it uses the default socket of your user. A remote machine's server is reached by running `poimen` on that machine (`ssh -t host poimen` for the TUI, `ssh host poimen ls` for a command), not from your laptop.

### Check the setup

`poimen doctor` tells you what is missing. See [Doctor](#poimen-doctor).

## The TUI

```sh
poimen
```

The screen has three parts:

- **Sidebar** (left): the spaces, the tabs of a space that has more than one terminal, and the terminals with their agent status, as in the app. Spaces of linked git worktrees sit below the space of their main checkout. A terminal that turns **blocked** while it is not on screen rings; one that turns **done** counts as unread until you look at it.
- **Tab bar and panes**: the tabs of the focused space, and the split tree of the focused tab with borders.
- **Status line** (bottom): the focused terminal and its mode (`control`, `observing`, `exited with <code>`), how far it is scrolled back and how many terminals wait to be seen. After the prefix it lists the keys; it also shows questions (take over, close) and config warnings.

The focused pane is attached as the **controller**: your keys go to it, and its size follows your terminal window. The other visible panes **observe**. If another client (the app, another TUI, `poimen attach --control`) already controls the focused terminal, the TUI observes it and asks `take over? [y/N]`. It never takes over without your answer, and when someone takes it from you, it falls back to observing.

Leaving the TUI never ends anything: detach only releases the attachments. Terminals, agents and the server keep running, and you get them back with the next `poimen` or in the app.

### Keys

Every TUI key follows a **prefix**, `Ctrl+B` by default (as in tmux). Press the prefix, release it, then the key. Every key without the prefix goes to the focused terminal unchanged, so the shortcuts of Claude Code, vim and others keep working. Pressing the prefix twice sends it once to the terminal.

| Keys | Action | `console.keys` name |
| :-- | :-- | :-- |
| prefix `d` | Detach: leave the TUI, everything keeps running. | `detach` |
| prefix `:` | Open the [command palette](#command-palette). | `palette` |
| prefix `N` | Open the [notification list](#notifications). | `notifications` |
| prefix `b` | Show or hide the sidebar. | `sidebar` |
| prefix `Down` / `Up` | Next or previous terminal of the sidebar. | `next` / `previous` |
| prefix `c` | New tab with a new terminal in the focused space. | `new_tab` |
| prefix `n` / `p` | Next or previous tab. | `next_tab` / `previous_tab` |
| prefix `&` | Close the focused tab and its terminals. | `close_tab` |
| prefix `%` | Split the focused pane, new terminal on the right. | `split_right` |
| prefix `"` | Split the focused pane, new terminal below. | `split_down` |
| prefix `x` | Close the focused pane's terminal. | `close_pane` |
| prefix `h` `j` `k` `l` | Focus the pane to the left, below, above, to the right. | `focus_left` … `focus_right` |
| prefix `H` `J` `K` `L` | Move the border of the focused pane by 5 %. | `resize_left` … `resize_right` |

Closing a tab or a pane ends its terminals; this is the one place in the TUI where something stops, and only on your key.

### Mouse

- A click on a terminal in the sidebar or on a pane focuses it.
- The wheel scrolls the scrollback of the pane under the pointer. When the program in the terminal asks for the mouse (or for alternate scrolling on its full-screen view), the wheel goes to that program instead. Typing scrolls back to the bottom.

### Command palette

Prefix `:` opens a list of the actions that have no key of their own. Type to filter (every word must appear in the entry), move with `Up`/`Down` or `Ctrl+P`/`Ctrl+N`, `Enter` runs the entry, `Esc` closes the palette, `Ctrl+U` clears the input.

| Entry | What it does |
| :-- | :-- |
| `Space: New` | Asks for a directory and creates a space there with the layout of the project file, template or default, as in the app. The input starts at the parent of the focused space; `Tab` completes directories, `~` is your home. |
| `Space: Rename`, `Space: Close` | Rename or close the focused space (closing asks first). |
| `Tab: New` | A new tab in the focused space. |
| `Terminal: Rename`, `Terminal: Close` | Rename or close the focused terminal (closing asks first). |
| `Task: Start from issue` | Reads the open issues of the space's GitHub repository through `gh`, lets you filter and pick one and, when there are several, a task mode. Then it creates the task worktree and a space on it that runs the mode's command, like the [task picker](/guide/github) of the app. |
| `Project: <name>` | Runs a command of the space's [project file](/guide/spaces) (`(agent)` marks an agent command). |
| `Preset: <name>` | Opens a preset of the project file. |

The palette shows only what fits: without a focused space only `Space: New`.

**Trust.** When a project file has commands you have not trusted yet, the palette shows them with their hash and asks: `t` trusts them (and remembers the hash), `w` creates the space without the commands, `Esc` or `n` cancels. This is the same trust check as in the app; a changed file asks again.

### Notifications

Prefix `N` lists what happened while you were elsewhere: agents that are blocked, done or in error. `Up`/`Down` (or `k`/`j`) move, `Enter` focuses the terminal of the entry, `Esc` or `q` closes the list. Opening a terminal clears its ring and unread mark. The console sends no system notifications.

### Configuration

The TUI reads the user config `~/.config/poimen/config.json`, the same file as the app ([Settings](/guide/settings)). It reads the file once at start; restart the TUI after a change.

```json
{
  "console": {
    "prefix": "C-a",
    "keys": {
      "split_right": "|",
      "split_down": "-",
      "detach": "q"
    }
  },
  "sidebar": {
    "sort": "attention",
    "groupWorktrees": true
  },
  "colorSchema": "my-wezterm-scheme"
}
```

- **`console.prefix`**: the prefix key. Default `C-b`.
- **`console.keys`**: the keys after the prefix, by the names in the [key table](#keys). Keys you leave out keep their default.
- **Key syntax**: `C-a` or `ctrl+a` (Ctrl), `M-x` or `alt+x` (Alt), `S-tab` (Shift), a single character (`d`, `%`, `|`), or a name: `up`, `down`, `left`, `right`, `pageup`, `pagedown`, `home`, `end`, `enter`, `tab`, `esc`, `backspace`, `space`. Case matters for characters (`H` is not `h`).
- **Invalid entries** warn in the status line at start and keep the defaults: an unknown key or action keeps that one default; two actions on one key, or an action on the prefix, keep all default keys.
- **`sidebar.rows`, `sidebar.sort`, `sidebar.groupWorktrees`** work as in the app. The row tokens `branch`, `dirty`, `sync` and `pr` stay empty in the TUI for now.
- **`colorSchema`**: a scheme of your own in `~/.config/poimen/themes/` colors the panes. With a bundled scheme (or none) the TUI keeps the colors of the terminal it runs in.

### Inside a poimen terminal

You can run the TUI inside a terminal that poimen itself manages, for example in a shell tab of the app. It never attaches to the terminal it runs in, so it cannot loop into itself; that terminal shows in the sidebar but cannot be focused.

## The CLI

Each command makes one request to the server and exits. Read commands take `--json` for scripts. `poimen --help` prints the overview, `poimen --version` the version.

Terminal and space ids are short strings such as `t42` and `s9`. `poimen ls` shows them; inside a poimen terminal your own id is in `$POIMEN_TERMINAL_ID`.

### `poimen ls`

```sh
poimen ls [--json]
```

All spaces with their tabs and terminals: id, name, directory; per tab `tab`, its id and name, and under it its terminals (a terminal in no tab, such as an ended one, stands right under the space); per terminal id, name, state (`running`, `exited`, `exited(<code>)`) and agent (`<kind>:<status>` or `-`).

```text
s9  poimen  /home/me/src/poimen
  tab tb3  claude
    t36  claude  running  claude-code:working
  tab tb4  zsh
    t42  zsh  running  -
```

`--json` prints one array of spaces, each with its `terminals` and `tabs` (the split trees), in the server's shapes ([protocol](https://github.com/GravionLabs/poimen/blob/main/docs/protocol.md)).

### `poimen status`

```sh
poimen status <terminal> [--json]
```

One terminal, the same line as in `ls`, plus the agent's last message when there is one (for a blocked agent: what it asks). `--json` prints the terminal object.

### `poimen new`

```sh
poimen new <space> [--name <name>] [-- <command> [args..]]
```

Creates a terminal in a space and prints its id. `<space>` is an id or an exact name; when several spaces share the name, use the id. Without a command it starts your shell; everything after `--` is the command, taken as it is.

```sh
poimen new poimen --name build -- cargo build --release
id=$(poimen new s9) && poimen send "$id" --enter "git status"
```

### `poimen space`

```sh
poimen space new <dir> [--name <name>] [--trust | --without-commands]
poimen space rename <space> <name>
poimen space close <space> [--yes]
```

`new` makes a space for a directory, with the layout the app would build, and prints its id (the name defaults to the directory's name). When the project file has commands you have not trusted, nothing is made: the command exits with 1 and prints the commands and their hash. Run it again with `--trust` to run them (stored for that content, as in the app) or `--without-commands` to build the panes without them. `close` ends the space's terminals; it asks nothing but is refused while one of them runs an agent, unless you pass `--yes`. `<space>` is an id or an exact name.

### `poimen tab`

```sh
poimen tab new <space> [--name <name>] [-- <command> [args..]]
poimen tab rename <tab> <name>
poimen tab close <tab> [--yes]
poimen tab split <terminal> right|down [--ratio <r>] [-- <command> [args..]]
poimen tab focus <tab> [<terminal>]
```

`new` opens a tab in a space and prints the tab id; without a command it runs your shell. `split` splits the pane of a terminal (the new one is right of or below it; `--ratio` is the share the old pane keeps, above 0 and below 1) and prints the new terminal's id. `close` ends the tab's terminals, refused while one runs an agent unless `--yes`. `focus` makes the tab (and the terminal in it) the focused one for every client. `<tab>` is an id or an exact name; when several tabs share the name, use the id.

### `poimen send`

```sh
poimen send <terminal> [--enter] [--] <text..>
```

Types the text into the terminal as it is, as if you had typed it. The words are joined with single spaces. `--enter` presses Enter after it. Put `--` before text that starts with a dash.

### `poimen prompt`

```sh
poimen prompt <terminal> [--] <text..>
```

Prompts the agent in a terminal. Unlike `send` it checks the agent first and is **refused** (exit 1) while the agent is working or blocked, or when no agent has reported in that terminal. After sending it waits up to 10 seconds for the agent to start and prints `started: agent is working`, or `sent, but the agent did not report any activity yet`. Agents report their status through hooks, see [Agents](/guide/agents).

### `poimen attach`

```sh
poimen attach [--control [--takeover]] <terminal>
```

Shows one terminal in your terminal, without the TUI around it.

- By default it **observes**: you see the output and your keys go nowhere.
- `--control` sends your input and makes your window size the terminal's size. When another client controls it, the command fails.
- `--takeover` (only with `--control`) takes the control from that client.

`Ctrl+B d` detaches (the configured `console.prefix` when it is a Ctrl+letter key); `Ctrl+B Ctrl+B` sends one `Ctrl+B`. When it ends it says why: detached, the terminal exited (with its code), the server detached you (for example another client took over), or the connection ended.

### Remote machines: `--machine` and `poimen machines`

```sh
poimen --machine <name> [<command>]
poimen machines [--json]
```

`--machine <name>` before any command (or alone, for the TUI) talks to the `poimen-server` of a machine from `machines` in `config.json` instead of the local one. It connects the way the app does, with `ssh -T -o BatchMode=yes <target> poimen-server stdio`, so the key must work without a prompt and the machine needs `poimen-server` (see [Remote machines](/guide/remote-machines)). The local server is not started. `poimen mcp` and `poimen doctor` stay local and refuse `--machine`.

`poimen machines` lists the configured machines and whether their server answers (`connected`, `offline` with ssh's error, or `incompatible` with both protocol versions). When a command cannot run, it says why in the app's words (a missing server, an older or newer server, a key or host key problem) and prints the command to try; poimen runs nothing itself.

### `poimen doctor`

```sh
poimen doctor [--json] [--bundle <file>]
```

Checks the setup, one line per check, `ok`, `warning` or `error`: the console, the server (answers, version, protocol, outdated or incompatible), the `poimen-pty-host` next to it, `server.json` (parses, counts, an interrupted save or a damaged copy), the agent CLIs `claude`, `copilot` and `gemini` with their hooks and MCP entry, and `gh` and `git`. It works without a running server and fixes nothing itself (hooks: [Agent hooks from a shell](#agent-hooks-from-a-shell)).

- `--json` prints one JSON document (`checks[]` with `id`, `status`, `detail`).
- `--bundle <file>` also writes a local JSON file for a bug report: the report, the shape of `server.json` (ids and counts, no names, directories, commands or messages) and the last 200 KiB of the server, hook and host logs. The file is private, never overwrites an existing one and is never uploaded. Read it before you send it.

Exit code 0 when no check is an error, else 1.

### Exit codes

| Code | Meaning |
| :-- | :-- |
| 0 | Done. |
| 1 | The request failed: no server, unknown terminal or space, a refused prompt, a doctor error. The reason is on stderr, prefixed `poimen:`. |
| 2 | Usage error: unknown command or option, a missing argument. The usage follows. |

### Scripting examples

Wait until an agent is done, then show its last message:

```sh
until poimen status t36 --json | jq -e '.agent.status == "done"' >/dev/null; do sleep 5; done
poimen status t36
```

All blocked agents:

```sh
poimen ls --json | jq -r '.[].terminals[] | select(.agent.status == "blocked") | "\(.id) \(.name)"'
```

For agents that lead other agents, the [MCP server](/guide/mcp) is the better tool: it waits on the server's events instead of polling.

## Agent hooks from a shell

Hooks are installed by the server binary, not by `poimen`:

```sh
poimen-server integration install claude    # or copilot, gemini
poimen-server integration status claude
```

The same step registers the [MCP server](/guide/mcp) for Claude Code and Copilot CLI. `poimen doctor` shows what is installed.

## What the console does not do (yet)

- The GitHub, Changes, Flow and Runs views exist only in the app. The console starts tasks from issues, but shows no pull requests or diffs.
- The sidebar has no git tokens (`branch`, `dirty`, `sync`, `pr`) yet.
- No settings dialog: the TUI reads `config.json` and nothing else, and only at start.
- No machine switching: the console talks to the server of the machine it runs on.
- Hook installation is in `poimen-server integration`, not in `poimen`.
- The CLI has no commands for spaces and tabs (create, rename, close, split); use the TUI or the app, or the `start_agent` tool of the [MCP server](/guide/mcp).
