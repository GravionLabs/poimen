# Project config

`.poimen/config.json` at the repository root describes how poimen treats the project. Schema:
[project-config.schema.json](project-config.schema.json). Parsing is tolerant: an invalid entry is
dropped with a warning naming the file and the key, and the rest loads. Layout pane commands and
`setup` run when a space is created (after you trusted the repository, see Trust); `teardown`,
`commands[]` and `presets[]` are only parsed and validated so far.

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
- `teardown`: a command.
- `tasks`: task modes of this repository, same schema as `tasks` in the user `config.json`
  (`modes[]`, `worktreeDir`). They merge by `name` over the user's modes (a project mode replaces
  the user's mode of the same name, new ones are added) and `worktreeDir` overrides. They count as
  commands for the trust question; for an untrusted repository only the user's modes are offered,
  with a note. `get_task_modes(dir)` returns the merged result.
- `commands[]`: `name` (unique), `command`, `target` (`terminal` or `agent`).
- `presets[]`: `name` (unique) and `panes[]` exactly as in `layout`.

## Commands

A command is a string typed into the user's shell, or an object
`{ "default", "windows", "macos", "linux" }`. The key of the current platform wins, `default` is
the fallback. When neither exists, the entry is skipped with a warning.

## Layout resolution

For a new space: `layout` of `.poimen/config.json`, else the deprecated
`.agents/herdr-layout.json` (still read until 1.0; a warning is shown each time a space is
created), else `spaces.layout` of the user config, else the built-in default. An invalid project
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
