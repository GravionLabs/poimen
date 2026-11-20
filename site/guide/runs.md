# Runs

The Runs item of the activity bar shows the agent runs of the repositories behind your spaces: deliver loops that work through a milestone, and any other tool that writes agent events. poimen only reads them and never writes.

## Where runs come from

A run is a file `<repository>/.agents/events/<run>.jsonl` in the format of [agent-events](https://github.com/GravionLabs/agent-events). poimen follows these files without polling. Runs of a remote machine arrive through its server.

## The view

- **Milestones** at the top shows each milestone with GitHub's closed and total counts and its issues in states: done, needs you, in progress, blocked and next.
- **Running now** lists runs that are working: the subject, the current task with its step, how long ago the last event was and *Focus terminal*.
- **Needs you** lists the gates a run waits on for a human.
- **History** groups finished runs by subject, newest first. Expanding a group shows a timeline: outcome, pull requests, what was skipped or left open, tokens per run and the events.

*Start loop* on a milestone group starts the task mode `deliver-loop` in the repository's space.

## Status bar and notifications

The status bar shows `▶ M5 7/19` per running loop and turns into a warning when a run needs you or failed. Attentions of runs also appear in the notification center. A run linked to a terminal keeps its pane `working` between turns, so the sidebar does not say `done` while the loop still works.
