# Agents

## Status from hooks

Claude Code, GitHub Copilot CLI and Gemini CLI report what they do through hooks that call `poimen-server hook`. poimen shows the result as **working**, **blocked**, **error**, **done** or **idle**:

- A finished turn reports **done**, which stays until the next prompt. A new session reports **idle**.
- A permission request reports **blocked** with the tool. A failed turn reports **error** and counts as waiting like blocked until the next working report.
- An agent that stays `working` without any hook report for `agents.silentAfter` minutes (default 10) is marked **silent**, because a hanging call looks like work in the terminal.

On the first start poimen offers to install the hooks. Later, **Settings > Backend > Server** shows a table per agent (CLI, hooks, MCP) with *Repair*, *Install hooks* and *Add MCP server*. From a shell: `poimen-server integration install|uninstall|status claude|copilot|gemini`. An agent reads its hooks when it starts, so one that was already running needs a restart. Tools that have no hooks can report through `poimen-server report agent --state working|blocked|idle`.

## Attention

- A pane that turns **blocked** while you are not looking rings on its tab and sidebar row. One that turns **done** counts as unread. Space rows sum their panes. Viewing the tab clears both.
- The dock badge counts the waiting agents. `CmdOrCtrl+Shift+A` jumps to the next one, and the status bar shows how many wait.
- The notification center keeps the last status changes. poimen sends no system notifications.
- Optional sounds for blocked and done (`sounds`, under **Settings > Appearance**), a tone of your own or a built-in one.

## Prompts and starting agents

- **Start agent...** in a pane's menu starts one of your `harnesses` in a shell pane.
- A read-only agent tab has a prompt bar above the terminal that sends a prompt without taking the controller. It waits for the agent and refuses while it works or waits at an approval.
- From a shell, `poimen prompt <terminal> <text>` does the same, see [Console and MCP](/guide/console).
- Agents can start and prompt other agents themselves through the [MCP server](/guide/console#mcp-server-for-agents). The sidebar then shows which agent started which, and the Flow shows the agents an agent started.

## Subagents and the Flow tab

The **Output panel** (`CmdOrCtrl+J`) follows the agent of the active tab and shows what its subagents do, read from the session transcripts of Claude Code and GitHub Copilot CLI, read-only. The **Flow** tab draws the run as a graph: the main agent, every skill call and every subagent, nested under the one that called them, with status, duration, tool calls and tokens per node. Agents started through the MCP tools appear as their own nodes; a click focuses their terminal. A Flow can also open as an editor tab.

<Screenshot src="flow-subagents-dark.png" light="flow-subagents-light.png" alt="The Flow tab with a run of one main agent, subagents for implementing, reviewing and fixing, and the skills implementation and code-review nested under them" caption="A run with subagents and skills nested under the agent that called them." />

## Tokens and Claude limits

An agent row shows the tokens its session used (hover for input, output, cache read and cache write); the space row shows the sum. With `usage.claude` on, the status bar shows how close you are to your Claude limits, for example `5h 62 %`, with every window and its reset time in the tooltip.

## Keeping agents running

- **Keep awake.** `agents.keepAwake` keeps the computer from sleeping while a local agent works or a run is running.
- **Hibernation.** `agents.hibernateAfter` (minutes, off by default) lets the server end the process of an agent that has been done for that long and resume its session on your next attach. The terminal and its shell stay. It applies only to agents whose harness defines how to exit and resume, and never while the agent works, is blocked or someone controls the terminal.
- **Classic renderer.** `agents.claude.classicRenderer` makes Claude Code write to the normal screen in poimen terminals, so scrollback, selection and search work.
