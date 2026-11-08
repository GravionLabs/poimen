# Glossary

The terms the UI uses, from the outside in: Space › Pane › Session.

- **Space**: a named group of panes, usually one directory or repository (a worktree for a task). A row in the sidebar. Clicking it opens the space overview tab (`poimen:space:<id>`; palette "Show Space Overview"): the panes with kind, label, session status and last activity, and the space actions New pane, Open in explorer, Open PR and Delete space. The backend calls it a workspace.
- **Pane**: one terminal in a space. It is either a plain terminal or an agent pane (a terminal that runs an agent). Pane actions: Focus (open its tab), Restart (start an agent in the pane again), Rename, Close, and for agent panes Flow and Output.
- **Session**: the run of an agent in a pane, with the status the backend reports from the agent hooks (`idle`, `working`, `blocked`, `done`, `unknown`). A plain terminal has no session status beyond `unknown`/`idle`.
