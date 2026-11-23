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
  `down`), `split.ratio` (0 to 1, default 0.5). Instead of `panes[]` it may hold `tabs[]` (tabs with
  a `command` or a `root` split tree, nested `children[]` and an app-only `place`, as in a named
  layout; #611). The commands of every tab are listed for trust like those of `panes[]`.
  The space overview edits `layout` and `presets[]` with the layout editor of the Layouts page
  (JSON stays under "Advanced"; one layout can be exported to and imported from a JSON file); a
  save changes the file hash, so a layout or preset that adds a command asks for trust again.
  Presets are palette entries of the project and are not offered as layouts in the settings.
  Named layouts, tabs and nested tabs: [ADR 0011](adr/0011-named-layouts-referenced-by-templates.md).
  `place` (#657) is `{direction, of?, ratio?}`: `direction` `right` or `down`, `of` the name of a
  tab of the same layout (default the parent tab; the first tab for a top-level tab) and `ratio`
  the share `of` keeps (default 0.5). The Tauri app opens that tab in an editor group split off the
  group of `of`; tabs with the same `place` share the group, a tab without `place` opens in the
  group of its parent and a top-level one in the main group (only in a space whose layout uses
  `children[]` or `place`). The server stores the hint as `Tab.place` (`of` as a tab id) and does
  nothing with it. **The TUI (`poimen`) ignores it**: every tab stays a tab of the sidebar. An
  unknown `of`, a cycle or an invalid direction is a warning and the tab opens without placement.
  Moving tabs between groups later changes nothing on the server.
- `setup`: a command that runs once, visibly, in a tab labelled `setup` of a newly created space
  (in the space directory). It does not run when an existing space is focused or when the
  repository is not trusted. If it cannot be started, the space stays and a warning names the
  reason. By default the agent pane starts in parallel; it does not wait for the setup
  (`setupPolicy`, below).
- `setupPolicy`: `"start"` (default, as before) or `"wait"`. With `"wait"` the command of the root
  pane (the first pane of the first tab, usually the agent) is typed only after `setup` exited 0.
  The setup tab reports its end through a marker file in the temp directory, so this needs a setup
  typed into a shell that understands `&&`/`||` (sh, bash, zsh, fish, PowerShell 7). If the setup
  fails (or does not end within 30 minutes) the root pane stays at its prompt and shows a notice
  instead. It has no effect without a `setup`, in an untrusted project or for later panes. Any
  other value is a warning and counts as `"start"`.
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
- `presets[]`: `name` (unique) and `panes[]` exactly as in `layout`, or `tabs[]` (#655: tabs with a
  `command` or a `root` split tree and `children[]` tabs, as in a named layout). Each is a palette
  entry `Preset: <name>` that builds new tabs at the end of the focused space, a child tab below
  its parent; a `tabs[]` preset is built by `poimen-server` (protocol 7).

The entries are read for the focused space and again when the palette opens. While the project
file is not trusted they show a lock; running one opens the trust dialog (see Trust).

## Detected scripts

A repository without a project file still gets palette entries from what it already declares.
Detectors live in `poimen_core::detect` (one per source; `package.json` is the first, `Cargo.toml`,
`justfile` and others can follow). The entries are tagged "Scripts" in the palette and have their
own Scripts tab; each is `Script: <name>` with the command line of the script as its description.

- Source: the `package.json` at the repository root of the focused space (as `commands[]` is read).
  A missing or invalid file yields no entries and no error. Scripts of workspace packages are not
  listed.
- Package manager: the `packageManager` field, else the lock file (`pnpm-lock.yaml`,
  `package-lock.json`, `yarn.lock`, `bun.lock`/`bun.lockb`), else npm. The command is
  `<pm> run <name>`.
- Running: opens a tab named after the script in the directory of the focused space and types the
  command. The backend builds it from the repository; the frontend only names the entry.
- Trust: there is no hash trust. A detected entry runs only on an explicit pick in the palette and
  nothing runs automatically; the entry shows the command line that will run.
- A `commands[]` entry with the name of a detected script wins; the detected one is not shown.
- A script whose name contains a control character is not listed (the command is typed into a terminal).
- Local spaces only; a remote space shows no detected entries.

## Skills

The skills of the repository are palette entries too, in their own Skills tab; each is
`Skill: <name>` with the description of the skill. They are detected by `poimen_core::detect::skills`
and are not commands: nothing runs in a tab.

- Source: `.agents/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md` at the repository root of the
  focused space. `name` and `description` come from the front matter (plain, quoted or folded
  text). A skill in both directories is listed once (`.agents/skills` first). A file without a
  closed front matter, or with a `name` that is empty or has characters other than letters,
  digits, `.`, `_`, `:` and `-`, is skipped. User skills (`~/.claude/skills`) are not listed:
  the agent has them in its own `/` menu.
- Running: sends `/<name>` as a prompt to an agent of the focused space whose harness takes slash
  skills (`slashSkills` in the `harnesses` registry: `true` for `claude` and `copilot`, a custom
  harness sets it in `config.json`). With several such agents the agent picker asks, as for a
  `commands[]` entry with `target: "agent"`; with none a notice says "the space has no agent that
  runs skills". Skill arguments are not sent.
- Trust: there is no hash trust; the prompt is sent only on an explicit pick.
- A `commands[]` entry of the same name does not hide a skill.
- Local spaces only; a remote space shows no skills.

## Environment variables

`setup`, `teardown`, `commands[]` and the task command run with these variables:

| Variable               | Value                                                                |
| ---------------------- | -------------------------------------------------------------------- |
| `POIMEN_ROOT_PATH`     | the main checkout of the repository (also from a linked worktree)    |
| `POIMEN_WORKTREE_PATH` | the worktree the space lives in (the same as the root without one)   |
| `POIMEN_SPACE`         | the label of the space                                               |
| `POIMEN_ISSUE`         | the issue number of a task started from the task picker              |
| `POIMEN_BRANCH`        | the branch of that task's worktree                                   |

`poimen-server` sets the first three in every terminal of a space (so the layout panes and the
terminals a `commands[]` entry opens have them too). `POIMEN_ISSUE` and `POIMEN_BRANCH` are only set
in the terminals built when a task creates its space, also on a machine; a `teardown` run by "Clean
up merged tasks" gets `POIMEN_BRANCH` of the worktree, a `teardown` sees `POIMEN_SPACE` as the name
of the worktree directory and never `POIMEN_ISSUE`.

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
