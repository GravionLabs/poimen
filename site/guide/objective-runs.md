# Objective runs

An **Objective** is an outcome that spans several repositories. The [GravionLabs skills](https://gravionlabs.github.io/skills/overview) define it and deliver the work with `deliver-loop`, one loop per repository ([delivery](https://gravionlabs.github.io/skills/delivery)). poimen is the **host** of such a run: it gives each loop a pane, collects what the loops report and shows what waits for you. It contains no delivery logic, and the loops also run without it.

Today you (or a lead agent) keep the order between the repositories: start the loop of a repository once what it requires is released. A single command for a whole Objective is planned in the skills and does not exist yet.

## Before you start

- A space per repository. A [remote machine](/guide/remote-machines) shows its runs in the app too, but *Deliver milestone…* is for local spaces only, and the MCP run tools read only the runs of the computer the agent runs on.
- `deliver-loop` installed and on your `PATH` (`/skills-manage delivery`).
- A loop that writes the `origin` of its run. poimen sets `AGENT_EVENTS_DIR` in its terminals, and it can place a run of that directory in a repository only through `origin`. A loop that follows the variable but writes no `origin` does not show in the Runs view; this is so for the `deliver-loop` of skills 5.16.0. Until your version writes it, start the loop without the variable, so it writes into the repository (macOS and Linux):

  ```sh
  env -u AGENT_EVENTS_DIR deliver-loop --milestone M005
  ```

## Loops in panes

Each loop runs in a terminal of its repository's space, where you can watch it and stop it.

- **From the app**: *Deliver milestone…* (command palette, a milestone group of the Runs view, or *Deliver…* in the issues tree) opens a tab `deliver <target>` and types the command, see [the Runs view](/guide/runs#the-view). It does not apply the workaround above.
- **By hand**: run `deliver-loop` in a shell pane.
- **Through a lead agent**: an agent uses the [MCP server](/guide/mcp#orchestrating-across-repositories): `create_space` and `start_shell` per repository, one `request_grant` so you confirm the plan once, `wait_any` to wait on all loops, `record_run` to keep the state in a **lead run**.

Closing a tab's view or the app ends no loop; closing the terminal does.

## The Deliver panel (Runs view)

The Runs item of the activity bar shows the runs of all spaces together:

- **Running now**: each running loop, its current task and step, *Focus terminal*.
- **Lead runs**: per lead run its members with their state, current task, blocked sessions, open dependencies, buttons to their terminals and the active grant with *Revoke*.
- **Milestones** and **History**: progress per milestone and the finished runs.

See [Runs](/guide/runs) for the whole view.

## The Events tab

The **Events** tab of the bottom panel lists the events of one run of the active pane's repository: time, task, step and result, colored by their tone. Pick another run in the list; runs of the same subject (a loop starts a new run after every stop) can be shown together with *All runs*. It follows new events until you scroll up.

## What needs you

The loops merge what passes their review and checks, and leave the rest to you. poimen shows it in these places:

- **Needs you** in the Runs view: the attentions of all runs that ask for you, each once, with its links, for example a handed-over pull request or an issue set aside with a question (`needs-decision`). A loop never merges the release pull request: that is always yours.
- The **status bar** turns into a warning, and the **notification center** lists each attention.
- A pane linked to a running run shows **blocked** when the run needs you and has no open task left.
- Confirmations of the MCP tools (a grant, a write into another space) are dialogs in the app.

Answer in the loop's terminal, on GitHub (merge the pull request, remove the label), or let your lead agent prompt the blocked session. What the loop may decide by itself and what always goes to the owner is defined by the skills, not by poimen: see their [delivery reference](https://gravionlabs.github.io/skills/delivery).
