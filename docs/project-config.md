# Project config

`.poimen/config.json` at the repository root describes how poimen treats the project. Schema:
[project-config.schema.json](project-config.schema.json). Parsing is tolerant: an invalid entry is
dropped with a warning naming the file and the key, and the rest loads. Layout pane commands and
`setup` run when a space is created (after you trusted the repository, see Trust); `teardown` runs before a
worktree cleanup and from the palette; `commands[]` and `presets[]` are palette entries (below).

```json
{
  "layout": {
    "panes": [
      { "name": "agent", "command": "claude" },
      { "name": "tests", "split": { "from": "agent", "direction": "right", "ratio": 0.6 } }
    ]
  },
  "setup": "pnpm install",
  "teardown": { "default": "docker compose down", "windows": "docker compose down -v" },
  "commands": [
    { "name": "test", "command": "pnpm test", "target": "terminal" },
    { "name": "review", "command": "/review", "target": "agent" }
  ],
  "presets": [
    { "name": "duo", "panes": [{ "name": "a" }, { "name": "b", "split": { "direction": "down" } }] }
  ]
}
```

## Keys

- `layout`: the pane layout of a new space, `panes[]` as in the README (`spaces.layout`): `name`,
  optional `command`, and from the second pane on `split.from`, `split.direction` (`right` or
  `down`), `split.ratio` (0 to 1, default 0.5).
- `setup`: a command that runs once, visibly, in a tab labelled `setup` of a newly created space
  (in the space directory). It does not run when an existing space is focused or when the
  repository is not trusted. If it cannot be started, the space stays and a warning names the
  reason. The agent pane starts in parallel; it does not wait for the setup.
- `teardown`: a command. It runs in the worktree through your shell (`$SHELL -lc`; on Windows `pwsh`, else `powershell.exe`, with `-NoProfile -Command`) before "Clean up merged tasks" removes a worktree, and from the palette entry "Project: Run teardown" (in the directory of the focused space). Timeout 5 minutes, output capped at 1 MB. A failing or timed-out teardown stops the removal of that worktree and shows its output; it does not run when a space closes. In an untrusted project it does not run; the cleanup then asks whether to remove without it.
- `tasks`: task modes of this repository, same schema as `tasks` in the user `config.json`
  (`modes[]`, `worktreeDir`). They merge by `name` over the user's modes (a project mode replaces
  the user's mode of the same name, new ones are added) and `worktreeDir` overrides. They count as
  commands for the trust question; for an untrusted repository only the user's modes are offered,
  with a note. `get_task_modes(dir)` returns the merged result. A mode is an agent (`harness`,
  `model`, `prompt`) or a command as an argv array (`["pwsh", "run.ps1", "{nr}"]`, each element
  expanded on its own, no shell); `cleanup` is an argv array. A string `command` or `cleanup` still
  works for one release with a deprecation warning.
- `github.agentActions[]`: agent actions for issues and pull requests, same schema as
  `github.agentActions` in the user `config.json` (`id`, `label`, `on`, `when`, `prompt`, `run`,
  `harness`, `model`, `batch`). They merge by `id` over the user's and the built-in ones
  (`"disabled": true` removes one); their prompts count as commands for the trust question and are
  only used in a trusted repository. `get_agent_actions(root)` returns the result with the source
  of each entry.
- `commands[]`: `name` (unique), `command`, `target` (`terminal` or `agent`). Each is a palette
  entry `Project: <name>`: `terminal` opens a tab named after it in the directory of the focused
  space and types the command; `agent` sends it as a prompt to the space's agent (a picker when
  there are several).
- `presets[]`: `name` (unique) and `panes[]` exactly as in `layout`. Each is a palette entry
  `Preset: <name>` that builds a new tab in the focused space from its panes.

The entries are read for the focused space and again when the palette opens. While the project
file is not trusted they show a lock; running one opens the trust dialog (see Trust).

## Commands

A command is a string typed into the user's shell, or an object
`{ "default", "windows", "macos", "linux" }`. The key of the current platform wins, `default` is
the fallback. When neither exists, the entry is skipped with a warning.

## Layout resolution

For a new space: `layout` of `.poimen/config.json`, else the deprecated
`.agents/herdr-layout.json` (still read until 1.0; a warning is shown each time a space is
created), else `spaces.layout` of the user config (seeded on the first start), else the built-in default. An invalid project
layout shows a warning naming the file and the space gets a single pane.

## Trust

Commands of a project file (layout pane commands, `setup`, `teardown`, `commands[]`, the panes of
`presets[]`) run only after you trusted that repository. Before a new space would type or run one,
poimen shows every command of the file and asks:

- **Trust and run** stores the SHA-256 of the file content under the repository root in
  `trustedProjects` of the user `config.json`.
- **Run without project commands** builds, names and splits the layout panes but types nothing; the
  question comes again next time.
- **Cancel** creates nothing.

Any change to the file changes its hash and revokes the trust, so the question comes back. With
both `.poimen/config.json` and the deprecated `.agents/herdr-layout.json` present, the hash covers
both. The settings view section "Trusted projects" lists the roots with a "Revoke" button. The
layout of `spaces.layout` in the user config and the built-in default need no trust.

## Editing in the space overview

The overview tab of a space has a Configuration section (opened by the gear icon on the space
row): the name of the space, its directory (read-only) and the `setup`, `teardown` and `layout`
keys of this file. Saving merges only those keys into the existing JSON object, so `commands`,
`presets`, `tasks` and any unknown key stay as they are (a file that is not valid JSON is never
overwritten). A layout must pass the same checks as above; errors show next to the field. Because
a saved file has a new hash, poimen lists its commands and asks for trust first (see Trust);
nothing is written until you confirm.
