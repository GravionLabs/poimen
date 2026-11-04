# Project config

`.poimen/config.json` at the repository root describes how poimen treats the project. Schema:
[project-config.schema.json](project-config.schema.json). Parsing is tolerant: an invalid entry is
dropped with a warning naming the file and the key, and the rest loads. Nothing in this file runs
yet; it is only parsed and validated.

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
- `setup`, `teardown`: a command.
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
