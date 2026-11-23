# MCP server

`poimen mcp` is an [MCP](https://modelcontextprotocol.io) server for coding agents. With it, an agent running in poimen can look around (which spaces and terminals exist, what an agent is doing, what a terminal shows) and lead other agents: start them, prompt them, wait for them and collect their results.

It is part of the [console binary](/guide/console) and talks to the same `poimen-server` as the app. It runs over stdio (one JSON-RPC message per line) and ends when the agent closes it.

## Setup

### Registered with the hooks

When poimen installs an agent's hooks (the offer on the first start, **Settings > Backend > Server**, or `poimen-server integration install claude|copilot`), it also registers the MCP server:

- **Claude Code**: `mcpServers.poimen` in `~/.claude.json` (user scope).
- **Copilot CLI**: `mcpServers.poimen` in `~/.copilot/mcp-config.json`, with all tools allowed.

An agent reads its MCP config when it starts, so restart one that was already running. `poimen doctor` shows per agent whether the entry is there.

poimen only touches its own entry: an entry named `poimen` with another command or arguments is yours and stays as it is. The RC channel uses `poimen-rc`.

### By hand

For another MCP client, register a stdio server with the console binary and the argument `mcp`:

```json
{
  "mcpServers": {
    "poimen": { "type": "stdio", "command": "/usr/bin/poimen", "args": ["mcp"] }
  }
}
```

The server finds the caller's terminal through `POIMEN_TERMINAL_ID`, which poimen sets in every terminal it starts. Outside a poimen terminal the read tools still work and `whoami` says `in_poimen: false`, but `start_agent`, `start_shell`, `stop_terminal`, `write_terminal` and `prompt_agent` are refused: there is no own space to write into.

## Tools

### Read tools

Read tools work on every space and never ask.

| Tool | Parameters | Returns |
| :-- | :-- | :-- |
| `whoami` | — | The terminal and space this agent runs in. |
| `list_spaces` | — | The spaces: id, name, directory, tab ids. |
| `list_terminals` | `space?` | The terminals, compact: id, space, name, state, agent kind and status, tokens. All spaces without `space`. |
| `get_terminal` | `terminal` | One terminal in full: where the status came from (`osc7501` or the hook), its times, title, progress, tone, hibernation, child records and the last 20 status changes. |
| `read_terminal` | `terminal`, `lines?` | The last lines (default 50, at most 500) of a terminal's screen and scrollback as plain text, and whether it still runs. Cut to 64 KiB with a note. Observes only. |
| `read_agent` | `terminal` | The last assistant message of the agent's transcript (Claude Code, Copilot CLI) with the session id. |
| `wait_agent` | `terminal`, `timeout_s?` | Waits until the agent is done, blocked, in error or gone, or the timeout (default 300 s, at most 3600 s) passed. Returns the status, the question of a blocked agent and its `report_done` report. Uses the server's events, no polling. |
| `list_runs` | `repository`, `status?`, `limit?`, `compact?` | The runs of the repository containing the directory, newest first (default 20). `status` is `running`, `finished` or `needs-you`; `compact` drops the `finished` and `links` lists. |
| `get_events` | `repository`, `run`, `after?`, `type?`, `limit?` | The raw events of one run after a sequence number (default 100, at most 1000), and whether there are more. |

Terminal output can hold secrets: `read_terminal` shows what the terminal shows, in any space.

### Write tools

| Tool | Parameters | Does |
| :-- | :-- | :-- |
| `start_agent` | `harness`, `space?`, `model?`, `prompt?`, `name?`, `worktree?` | Starts a [harness](/guide/agents) (for example `claude`, `copilot`) in a new terminal, in your own space when `space` is absent. With `worktree: {issue, title?, mode?}` or `{branch}` it first creates the task worktree and a new space on it; an existing worktree of that branch is reused unchanged. Returns the terminal, and for a worktree its space, path and branch. |
| `start_shell` | `command`, `cwd?`, `name?`, `space?`, `env?` | Runs a long-running command, such as the deliver loop (`["node", "deliver-loop.mts", "--milestone", "M005"]`), in a new shell pane. `command` is an argument list, never a shell string; `cwd` defaults to the directory of the space, `space` to your own space. Returns the terminal id. The pane stays after the command ends, so you can read the output with `read_terminal` and `get_terminal` (exit code) and follow the work with `get_events` and `list_runs`. Not for short commands: use your own shell for those. |
| `stop_terminal` | `terminal`, `mode?`, `force?` | Ends what a terminal runs, for example the deliver loop you started with `start_shell`. `mode` `interrupt` (default) sends Ctrl-C to the foreground process and waits a few seconds; `close` closes the pane and ends its processes. `close` of a terminal with a running agent is refused unless `force` is true. The answer's `outcome` is `ended`, `running` (it ignored Ctrl-C; try `close`) or `gone` (it had exited already). `get_terminal` shows the state afterwards. |
| `write_terminal` | `terminal`, `text`, `enter?` | Types text into an existing terminal that runs no agent, for example the `shell` pane of a space; `enter` (default false) sends Enter after the text. **The text runs in your shell, with your rights**, so prefer `start_shell` for new work. Control characters other than newline and tab are refused (no signals, no escape sequences; `stop_terminal` interrupts). A terminal with a running agent is refused: use `prompt_agent`. |
| `prompt_agent` | `terminal`, `text` | Types a prompt into another agent. Refused while that agent is working or blocked. |
| `report_done` | `outcome`, `summary`, `files?` | Called once by an agent that another agent started: `outcome` is `succeeded`, `failed` or `cancelled`, `summary` at most three sentences. poimen types one line with it into the starting agent as soon as that one is idle, hands it to its `wait_agent`, and closes the run in the [Runs](/guide/runs) view. |

`start_agent` adds a short instruction to the prompt that tells the new agent to call `report_done` at the end.

### Confirmation

- In the **calling agent's own space**, `start_agent` (without `worktree`), `start_shell`, `stop_terminal` and `prompt_agent` just run.
- In **any other space**, and for every **new worktree space**, poimen asks you in the app and waits up to 60 seconds. A declined or unanswered request, or one while the app is not open, is refused.
- `write_terminal` asks **every time**, also in the calling agent's own space, and the dialog shows the text. The only exception is a terminal the same agent started with `start_shell`. No setting turns the question off.
- Every write shows in the notification center with the terminal that made it.

## Leading other agents

A typical loop of a lead agent:

1. `whoami` and `list_terminals` to see where it is and what runs.
2. `start_agent` per piece of work, best with `worktree: {issue}` so each worker has its own checkout.
3. `wait_agent` on each worker. On `blocked`, answer with `prompt_agent` or ask you.
4. Read the `report_done` report (in the `wait_agent` answer), or `read_agent` / `read_terminal` when there is none.

The sidebar shows which agent started which, and the Flow tab of the lead shows the agents it started.

The short `poimen-lead` skill of [GravionLabs/skills](https://github.com/GravionLabs/skills) teaches an agent this loop: when to split, one worktree per worker, wait instead of polling, how to handle a blocked worker.

## Errors

A tool that fails returns a result with `isError` and the reason (unknown terminal, refused prompt, declined confirmation). An unknown tool or an invalid message is a JSON-RPC error. Tools with a fixed parameter list reject unknown parameters.
