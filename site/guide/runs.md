# Runs

The Runs item of the activity bar shows the agent runs of the repositories behind your spaces: deliver loops that work through a milestone, and any other tool that writes agent events. poimen only reads them and never writes.

<Screenshot src="runs-dark.png" light="runs-light.png" alt="The Runs view: a milestone with its progress, the run working on issue 1109 and a gate that needs you" caption="The Runs view: the milestone, the running loop with its current task and what needs you (demo data)." />

## Where runs come from

A run is a file `<repository>/.agents/events/<run>.jsonl` in the format of [agent-events](https://github.com/GravionLabs/agent-events). poimen follows these files without polling. Runs of a remote machine arrive through its server.

## The view

- **Milestones** at the top shows each milestone with GitHub's closed and total counts and its issues in states: done, needs you, in progress, blocked and next.
- **Running now** lists runs that are working: the subject, the current task with its step, how long ago the last event was and *Focus terminal*.
- **Needs you** lists the gates a run waits on for a human.
- **History** groups finished runs by subject, newest first. Expanding a group shows a timeline: outcome, pull requests, what was skipped or left open, tokens per run and the events.

*Deliver milestone* on a milestone group opens a dialog that starts a deliver loop for the milestone. The same dialog opens from the command palette (*Deliver milestone…*, a picker of the open milestones of the active repository) and from *Deliver…* in the row menu of the issues tree (for the marked issues, else the row's issue).

The dialog shows the target (`--milestone M<nnn>` or `--issues 12,15`), the harness (`claude` or `copilot`, empty keeps the loop's default), an optional model and `--max`, the base branch (from `deliver.base` in `.agents/config.json`, else the loop's default) and the exact command line. *Start loop* types `deliver-loop …` into a new tab named `deliver <target>` in the repository's space. poimen only launches it: the loop starts its agent sessions itself, and poimen never starts a loop on its own, on a timer or again after it ended.

- `deliver-loop` must be installed and on your `PATH` (`/skills-manage delivery`); without it the dialog says so and starts nothing.
- A project that is not trusted yet asks you to review and trust it first, because the command can run a copy of the skill that lives in the repository.
- While a loop runs for the repository (or one you just started has not written its run yet) a second start is refused with *Show run*.
- Only spaces on this Mac: a space on a machine has no such action.

## Status bar and notifications

The status bar shows `▶ M5 7/19` per running loop and turns into a warning when a run needs you or failed. Attentions of runs also appear in the notification center. A run linked to a terminal keeps its pane `working` between turns, so the sidebar does not say `done` while the loop still works.
