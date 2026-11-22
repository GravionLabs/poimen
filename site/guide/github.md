# GitHub and tasks

The GitHub item of the activity bar opens a tab with pull requests, issues and milestones of the repositories behind your spaces. It uses the GitHub CLI: run `gh auth login` once. Answers are cached for 30 seconds, and a repository that cannot be read shows its own message.

## Pull requests

The list shows checks, review state, size and age, with filters (review requested, mine, all; by repository; by title or number). A pull request opens a detail pane with its checks, reviews, linked issues and actions: approve, request changes, comment, ready or draft, close or reopen, merge (only when GitHub says it can be merged, with the allowed methods), re-run failed checks and edit reviewers, labels and assignees. A failing check that ran on GitHub Actions has *Show log*: it expands to the last 200 lines of the failed log (colors removed), fetched on the first expand and kept for the head commit. *Open on GitHub* stays. *Open space* or *Check out* creates a worktree for the pull request.

## Checks that finish

For the open pull request of each branch that is checked out in a space, poimen watches the checks while some are pending, never faster than the 30 second cache. When they finish, the notification center gets one entry with the pull request and the space: all passed, or failed with a *Fix checks* button that starts the agent action of that name. Each result is reported once per head commit, the `sounds` setting applies as for done and blocked, and nothing is requested without `gh` or without a pending check.

## Issues

The Issues page shows the open issues of each repository as a tree over GitHub's native sub-issues, with level (epic, feature, PBI, bug) from the label or the title prefix, `[needs-refinement]` and `[needs-decision]` markers, assignees and linked pull requests with their checks. Filter by state, level, label, assignee or text. You can edit title, body, labels, assignees and milestone, close and reopen, create issues and sub-issues, change a parent, and apply edits to several selected issues at once. poimen writes a body only when it is unchanged on GitHub since you loaded it.

## Milestones

The Milestones page lists the open milestones per repository, ordered by version, marks the current one and shows due date, counts and progress. Selecting one filters the issues to it.

## Agent actions

Buttons and the `⋯` menu on issues and pull requests start an agent with a prompt template: *Plan*, *Refine*, *Review*, *Fix checks*, *Resolve conflicts* and *Address review comments* by default. *Resolve conflicts* is offered for a pull request that conflicts with its base: it runs in a worktree and its prompt names the base branch. Like *Fix checks* it is disabled, with the reason, for a pull request from a fork. Shift+click opens a dialog to change the harness, model and whether it runs in a worktree. `github.agentActions` in your config, or the project file, adds your own. Running agents show a badge on the row they work on.

## Starting a task from an issue

**Start task** in the command palette, a repository's menu or a Cmd/Ctrl+click on an issue link in a terminal opens a picker with the open issues of the repository. Choosing one creates a git worktree and branch (`tasks.modes` sets the branch template and directory), opens a space on it and starts an agent with the issue as its prompt. **Clean up merged tasks** removes the worktrees of merged pull requests, never a dirty one, the focused one or a protected branch. With remote machines you can pick the machine to run the task on.
