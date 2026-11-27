# Glossary

The terms the UI uses, from the outside in: Space › Pane › Session.

- **Space**: a named group of panes, usually one directory or repository (a worktree for a task). A row in the sidebar. Clicking it opens the space overview tab (`poimen:space:<id>`; palette "Show Space Overview"): the panes with kind, label, session status and last activity, and the space actions New pane, Open in explorer, Open PR and Delete space. The backend calls it a workspace.
- **Pane**: one terminal in a space. It is either a plain terminal or an agent pane (a terminal that runs an agent). Pane actions: Focus (open its tab), Restart (start an agent in the pane again), Rename, Close, and for agent panes Flow and Output.
- **Session**: the run of an agent in a pane, with the status the backend reports from the agent hooks (`idle`, `working`, `blocked`, `done`, `unknown`). A plain terminal has no session status beyond `unknown`/`idle`.

## Delivery runs

The terms of agent runs and of poimen as their host. The delivery model itself is described in the [skills overview](https://gravionlabs.github.io/skills/overview).

- **Run**: one execution that reports its progress as [agent-events](https://github.com/GravionLabs/agent-events), a file `<run>.jsonl` of JSON lines: a deliver loop, a lead run, or the report of a started agent. The Runs view groups runs by their subject (for example a milestone). A run is not a Session: a session is the agent in one pane.
- **Lead run**: the run in which a lead agent records an orchestration over several repositories (`record_run`, run id `lead-<uuid>`): plan, members, dependencies, grant and decisions, so another session can resume it ([ADR 0018](adr/0018-scoped-write-grant-and-lead-runs-for-orchestration.md)).
- **Origin**: where a run comes from, in its `run.started` event: `repo` (`owner/repo`, else the repository's directory), `branch`, `worktree` and `host`, each optional. poimen attributes a run to a repository by it.
- **Event directory**: the one directory per user that holds the run files: `AGENT_EVENTS_DIR`, else `%LOCALAPPDATA%\agent-events` on Windows and `$XDG_STATE_HOME/agent-events` or `~/.local/state/agent-events` elsewhere. The older `<git-common-dir>/.agents/events` of a repository is still read for one minor version of agent-events.
- **Triage**: the judgment step of an Objective run: a session (`deliver-triage`) that the deliver loop starts for one attention event and that either takes one of a fixed list of actions or prepares the decision for the owner ([skills ADR 0011](https://github.com/GravionLabs/skills/blob/release/next/docs/adr/0011-objective-runs-in-three-layers-loop-code-deliver-triage-poimen-as-host.md); planned, not built yet). It is not part of poimen, and not the `github-triage` skill, which sorts issues filed by others.

