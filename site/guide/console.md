# Console and MCP

The `poimen` binary is a console client of the same `poimen-server` the app uses. It shows and steers the same terminals from a shell, over ssh or on a machine without a desktop. [Installation](/guide/installation#console) says where it lives per system.

## The TUI

`poimen` without arguments opens a text UI: a sidebar with spaces, tabs and terminals with their agent status, and the selected terminal attached as controller. When another client controls it, the TUI observes and asks before it takes over. Splits, tabs, a command palette, the notification list and new spaces and tasks work like in the app.

The keys follow a prefix, `Ctrl+B` by default (`console.prefix`; twice sends it to the terminal):

| Keys | Action |
| :-- | :-- |
| prefix `d` | Detach. The terminals keep running. |
| prefix `b` | Show or hide the sidebar. |
| prefix `Down` / `Up` | Next or previous terminal. |
| prefix `N` | Notifications. Enter focuses the terminal. |

`console.keys` rebinds them. The TUI follows `colorSchema` and uses your terminal's colors otherwise.

## The CLI

| Command | What it does |
| :-- | :-- |
| `poimen ls [--json]` | Spaces and terminals with agent status. |
| `poimen status <terminal> [--json]` | One terminal. |
| `poimen new <space> [--name n] [-- cmd..]` | A new terminal in a space (by id or exact name). Prints its id. |
| `poimen send <terminal> [--enter] <text..>` | Types text into a terminal. |
| `poimen prompt <terminal> <text..>` | Prompts an agent. Refused while it works or waits. |
| `poimen attach [--control [--takeover]] <terminal>` | Attaches this terminal to another, `Ctrl+B d` detaches. |
| `poimen doctor [--json] [--bundle <file>]` | Checks the setup, see below. |

Usage errors exit 2, failures 1. Inside a poimen terminal the commands talk to the server of that terminal.

### Doctor

`poimen doctor` prints one line per check, `ok`, `warning` or `error`: the console, the server (answers, version, protocol), the PTY host next to it, `server.json`, the agent CLIs with their hooks and MCP entry, and `gh` and `git`. `--bundle <file>` writes a local JSON file for a bug report with the report, the shape of `server.json` (no names, directories or messages) and the tails of the logs. It stays on your disk, is never uploaded and never overwrites a file. Read it before you send it.

## MCP server for agents

`poimen mcp` is a stdio MCP server. The step that installs an agent's hooks also registers it for Claude Code (`~/.claude.json`) and Copilot CLI (`~/.copilot/mcp-config.json`), so a session can look around in poimen and lead other agents. An agent that is already running reads its MCP config when it starts.

| Tool | Use |
| :-- | :-- |
| `list_spaces`, `list_terminals` | Spaces with tabs, terminals with agent status. |
| `whoami` | The terminal the agent runs in. |
| `list_runs` | Runs of a repository. |
| `start_agent` | Starts a harness in a new terminal, optionally in a new task worktree. |
| `prompt_agent` | Types a prompt into another agent. |
| `wait_agent` | Waits until an agent is done, blocked, in error or gone. |
| `read_agent` | The last message of an agent's transcript. |
| `report_done` | A started agent reports its outcome and summary to the agent that started it. |

Write tools act freely in the calling agent's own space. For another space poimen asks you in the app and waits up to 60 seconds; a declined or unanswered call is refused. Every write shows in the notification center with the calling terminal.

To lead other agents with these tools, install the short `poimen-lead` skill of [GravionLabs/skills](https://github.com/GravionLabs/skills): when to split, one worktree per worker, wait instead of polling, handle a blocked worker.
