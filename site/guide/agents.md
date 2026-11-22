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
- A program can end with a **warning**: *done* with `tone=warning` in its [OSC 7501](https://github.com/GravionLabs/poimen/blob/main/docs/protocol.md) report means it ended without success and without an error, and you should look. Its dot has its own shape and color, the tooltip shows the message, and it counts as waiting like blocked and error.
- **Toasts and the notification center.** A short message (a toast) tells the result of something you did not watch, such as copied, pushed, exported or a failed settings write, and why it failed. A state that lasts, such as disconnected or read-only, stays inline where it applies. Agent events (waiting, done, failed) go to the notification center only, never to a toast.
- Optional sounds for blocked and done (`sounds`, under **Settings > Appearance**), a tone of your own or a built-in one.

<Screenshot src="blocked-dark.png" light="blocked-light.png" alt="A blocked agent: the space card is marked waiting: permission and the terminal shows the question Claude Code asks" caption="A blocked agent is marked on its card and its row; the status bar counts the waiting agents (demo data)." />

## Prompts and starting agents

- **Start agent...** in a pane's menu starts one of your `harnesses` in a shell pane.
- **Resume session...** in the menu of a space, of a shell pane and in the command palette lists the earlier Claude Code and Copilot CLI sessions of the directory (start, last activity, first prompt). Picking one starts the harness with its `resumeArgs` in a new tab, or in the pane at its prompt. A harness without `resumeArgs` is not offered, and a space on a remote machine shows the entry disabled with the reason.
- A read-only agent tab has a prompt bar above the terminal that sends a prompt without taking the controller. It waits for the agent and refuses while it works or waits at an approval.
- From a shell, `poimen prompt <terminal> <text>` does the same, see [Console](/guide/console#poimen-prompt).
- Agents can start and prompt other agents themselves through the [MCP server](/guide/mcp). The sidebar then shows which agent started which, and the Flow shows the agents an agent started.

## Subagents and the Flow tab

The **Output panel** (`CmdOrCtrl+J`) follows the agent of the active tab and shows what its subagents do, read from the session transcripts of Claude Code and GitHub Copilot CLI, read-only. The **Flow** tab draws the run as a graph: the main agent, every skill call and every subagent, nested under the one that called them, with status, duration, tool calls and tokens per node. Agents started through the MCP tools appear as their own nodes; a click focuses their terminal. A Flow can also open as an editor tab.

<Screenshot src="flow-dark.png" light="flow-light.png" alt="The Flow tab: the main agent connected to two skill nodes and a started agent node" caption="The Flow of an agent that started another agent (node Started agent) and called two skills (demo data)." />

<Screenshot src="flow-subagents-dark.png" light="flow-subagents-light.png" alt="The Flow tab with a run of one main agent, subagents for implementing, reviewing and fixing, and the skills implementation and code-review nested under them" caption="A run with subagents and skills nested under the agent that called them." />

## Tokens and Claude limits

An agent row shows the tokens its session used (hover for input, output, cache read and cache write); the space row shows the sum. With `usage.claude` on, the status bar shows how close you are to your Claude limits, for example `5h 62 %`, with every window and its reset time in the tooltip.

## Keeping agents running

- **Keep awake.** `agents.keepAwake` keeps the computer from sleeping while a local agent works or a run is running.
- **Hibernation.** `agents.hibernateAfter` (minutes, off by default) lets the server end the process of an agent that has been done for that long and resume its session on your next attach. The terminal and its shell stay. It applies only to agents whose harness defines how to exit and resume, and never while the agent works, is blocked or someone controls the terminal. An agent that got input after it was done, for example text typed into its input box, is not hibernated either; it is again after its next done and the idle time.
- **Classic renderer.** `agents.claude.classicRenderer` makes Claude Code write to the normal screen in poimen terminals, so scrollback, selection and search work.
