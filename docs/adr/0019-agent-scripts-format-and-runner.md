# ADR 0019: Agent scripts: format and runner

- Status: Proposed
- Date: 2026-11-25
- Issues: #1091, Feature #1089
- Builds on: [ADR 0007](0007-plugin-system-with-sandboxed-worker-and-view-contexts.md), [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md), [ADR 0017](0017-script-environment-for-runs.md)
- Supersedes in part: [ADR 0017](0017-script-environment-for-runs.md) ("poimen stores and lists no scripts and has no registry"), for agent scripts only

## Context

An **agent script** is a saved, named agent call: a prompt with a harness, a model, the tools the agent may use, inputs and an output target. The user keeps it as a file and runs it from the palette, a button, the command line or later a schedule (#1095). Nixe (GravionLabs/nixe#296) wants the same thing for notes, so the format and the code that runs a script are shared.

What poimen has today does not cover it:

- `commands[]` and task modes (`tasks.modes[]`) are entries of poimen's project or user config. They start an agent with a prompt, but have no inputs other than the issue placeholders, no output target and no tool limits. Nixe cannot read them.
- [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md) runs an interactive harness session in a pane. It needs `poimen-server` and hook reports, and leaves the print mode open as "a later, separate decision for harnesses without hooks".
- [ADR 0017](0017-script-environment-for-runs.md) keeps programs such as the deliver loop as Node processes and says poimen stores and lists no scripts.
- The harness templates (`poimen-core/src/harness.rs`, `Harness`) already know a headless call (`printArgs`, `-p {prompt}` for `claude` and `copilot`), but have no way to pass the tools an agent may use.
- The trust check (`poimen-core/src/trust.rs`) hashes `.poimen/config.json` (and the deprecated `.agents/herdr-layout.json`) only (#255). Files the project file refers to are not covered.

Both harness CLIs restrict tools in print mode: Claude Code with `--tools` (the tools that exist) and `--allowedTools` (the tools that run without a prompt), Copilot CLI with `--available-tools` and `--allow-tool`. In print mode neither can ask: a tool that no rule allows is denied.

## Decision

### Format

One Markdown file per script: YAML frontmatter between `---` lines, then the prompt as the body.

```markdown
---
name: summarize-file
description: Summary of a file in five bullet points
harness: claude
model: sonnet
tools: [Read]
inputs:
  audience:
    description: Who reads the summary
    default: developers
output:
  target: show
timeout: 5m
---

Summarize {file} for {audience} in five bullet points.
```

| Key           | Type                                                  | Default                     | Example                     |
| ------------- | ----------------------------------------------------- | --------------------------- | --------------------------- |
| `format`      | integer                                               | `1`                         | `format: 1`                 |
| `name`        | string `[a-z0-9][a-z0-9-]*`, at most 64 characters    | the file name without `.md` | `name: summarize-file`      |
| `description` | string                                                | none                        | `description: Summary …`    |
| `harness`     | string, a harness id                                  | `claude`                    | `harness: copilot`          |
| `model`       | string, one model of the harness                      | the harness's default       | `model: sonnet`             |
| `tools`       | array of strings, tool names of the harness           | `[]` (no tools)             | `tools: [Read, Grep]`       |
| `inputs`      | map name to input (below)                             | `{}`                        | see the example             |
| `output`      | object: `target`, `path`                              | `{ target: show }`          | `{ target: file, path: … }` |
| `timeout`     | duration: a number with `s`, `m` or `h`, at most `2h` | `10m`                       | `timeout: 90s`              |
| `triggers`    | array, reserved for #1095                             | `[]`                        | (#1095)                     |

An input is `{ description?: string, default?: string, required?: boolean }` (`required` defaults to `true` without a `default`, else `false`). Its name is `[a-z][a-z0-9_]*` and must not be one of the built-in variables.

`tools` is the whole permission of a script. The listed tools are the only ones that exist for the agent and they run without a prompt; every other tool is denied. There is no key for the permission mode and no way for a script to skip the permission checks (`--dangerously-skip-permissions`, `--allow-all`, `bypassPermissions`): print mode cannot ask, so the list decides. A tool entry is a tool name of the harness (`Read`, `shell`) or a rule in the harness's own syntax (`Bash(git status)`, `shell(git:*)`). The runner does not translate tool names between harnesses; how it splits a rule into the tool that exists and the rule that allows it is under "Harness call".

The schema of the frontmatter (JSON Schema 2020-12; the repository of the runner keeps it as `agent-script.schema.json`, versioned with `format`):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Agent script frontmatter (format 1)",
  "type": "object",
  "properties": {
    "format": { "const": 1, "default": 1 },
    "name": {
      "type": "string",
      "pattern": "^[a-z0-9][a-z0-9-]*$",
      "maxLength": 64
    },
    "description": { "type": "string" },
    "harness": { "type": "string", "minLength": 1, "default": "claude" },
    "model": { "type": "string", "minLength": 1 },
    "tools": {
      "type": "array",
      "items": { "type": "string", "minLength": 1 },
      "default": []
    },
    "inputs": {
      "type": "object",
      "propertyNames": {
        "pattern": "^[a-z][a-z0-9_]*$",
        "not": { "enum": ["file", "selection", "dir"] }
      },
      "additionalProperties": {
        "type": "object",
        "properties": {
          "description": { "type": "string" },
          "default": { "type": "string" },
          "required": { "type": "boolean" }
        }
      },
      "default": {}
    },
    "output": {
      "type": "object",
      "properties": {
        "target": {
          "enum": ["show", "file", "append", "replace"],
          "default": "show"
        },
        "path": { "type": "string", "minLength": 1 }
      },
      "if": {
        "required": ["target"],
        "properties": { "target": { "enum": ["file", "append"] } }
      },
      "then": { "required": ["path"] },
      "default": { "target": "show" }
    },
    "timeout": {
      "type": "string",
      "pattern": "^[1-9][0-9]*[smh]$",
      "default": "10m"
    },
    "triggers": { "type": "array", "default": [] }
  }
}
```

Tolerance works at the level of files, as the project config (`poimen-core/src/project.rs`) does at the level of entries: an invalid script is dropped from the list with a warning, the other scripts of the directory load. Inside one script nothing is dropped, because a dropped key would change what the agent may do.

- An unknown key is kept and gives a warning naming the file and the key; a later `format` can use it.
- A key with an invalid value makes the script **invalid**: it is listed with the file, the key and the reason, and it never runs. There is no fallback to a default, because a dropped `tools` or `timeout` would change what the agent may do.
- A file without frontmatter, with frontmatter that is not YAML, with an empty body or with a `format` the runner does not know is invalid in the same way.
- Two scripts of one directory with the same `name` are both invalid.

### Inputs

The body may use these variables:

| Variable      | Value                                                                            |
| ------------- | -------------------------------------------------------------------------------- |
| `{file}`      | the path of the file the run is started on (the open file, the file of a button) |
| `{selection}` | the selected text in that file                                                   |
| `{dir}`       | the working directory of the run                                                 |
| `{<input>}`   | a declared input; the host asks for it at start, the CLI takes `--input k=v`     |

A built-in variable the host cannot give (no open file, no selection) is an error before the harness starts, as is a missing required input. A placeholder that is neither built-in nor declared makes the script invalid (as `agent_actions.rs` does). `{{` and `}}` stand for literal braces.

Escaping: the expansion is one pass over the body; a value that contains `{x}` is not expanded again. The prompt is passed to the harness as one argv element, never through a shell, so quotes, `$()`, `;` and newlines in a value are text (the rule of `harness.rs`). The values are data inside the prompt, not instructions: the runner does not mark them up, and a script that reads text of others must expect prompt injection and keep its `tools` small (#1095 sets the rules for runs nobody watches). The expanded prompt is at most 256 KiB; a larger one is an error before the start.

### Output

A run returns one record:

```json
{
  "status": "ok",
  "exitCode": 0,
  "durationMs": 41200,
  "text": "…",
  "changedFiles": ["notes/a.md"]
}
```

- `status`: `ok`, `error` (the harness exited non-zero), `failed` (the harness could not be started: program missing, spawn error), `timeout`, `cancelled` (a signal or the host), `refused` (not started by the runner: invalid script, unknown script name, missing input, a built-in variable the host cannot give, a prompt over the size limit, a run of the same script already running, a trust hash that does not match, a harness without `toolsArgs` or `isolateArgs`). A harness that exits non-zero or is ended by a signal the runner did not send is `error`. When the harness ended `ok` but the output target fails (the `file` exists, a write error), the status is `error` with `"outputError": "<reason>"` and `text` still set, so nothing is lost.
- `exitCode`: the harness's exit code, absent when it did not start.
- `text`: the answer of the agent (the harness's print output).
- `changedFiles`: in a git working tree, the paths that are modified or untracked at the end (`git status --porcelain`) and whose content hash differs from the start (a file already dirty at the start counts only when the agent changed it again); absent outside a repository. It reports, it does not prove that the agent made the change: another process in the same directory shows too.

Output targets the format defines; a host offers a subset and says which:

| Target    | Effect                                                                                                                      | poimen | Nixe | CLI |
| --------- | --------------------------------------------------------------------------------------------------------------------------- | ------ | ---- | --- |
| `show`    | the host shows `text` (CLI: stdout)                                                                                         | yes    | yes  | yes |
| `file`    | `text` is written to `path` (relative to `{dir}`, variables expanded); an existing file is an error, nothing is overwritten | yes    | yes  | yes |
| `append`  | `text` is appended to `path`, which is created when missing                                                                 | yes    | yes  | yes |
| `replace` | `text` replaces `{selection}` in `{file}`                                                                                   | later  | yes  | no  |

A target the host does not offer falls back to `show` with a note. The target applies only when `status` is `ok`. A `path` that leaves `{dir}` (absolute, or `..` above it) makes the script invalid.

### Runner

The runner is a Rust crate with a CLI in a repository of its own, **GravionLabs/agent-scripts**, used by poimen and Nixe.

|                          | (a) own repository, crate and CLI                                                  | (b) crate inside poimen, Nixe depends on it                  | (c) one implementation per app              |
| ------------------------ | ---------------------------------------------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------- |
| One format, one behavior | yes                                                                                | yes                                                          | no: two parsers and two harness calls drift |
| Release and versioning   | own tags (`v<x.y.z>`), own changelog; apps pin a tag                               | tied to poimen's releases; Nixe pins a poimen commit         | per app                                     |
| Without either app       | the `agent-scripts` binary from the release archives, plus a logged-in harness CLI | a poimen build or `cargo install` from the poimen repository | not possible                                |
| Cost                     | one more repository and release pipeline                                           | Nixe pulls in the poimen workspace                           | double work for every change                |

Why (a): the same reason as agent-events. A format that two apps and the command line share is better kept in a place that neither app owns, so a script runs from cron or CI with nothing but the binary and the harness. The cost is a second release pipeline, which the GravionLabs/ci workflows already provide.

- **Repository:** GravionLabs/agent-scripts; crate `agent-scripts` (library), binary `agent-scripts`. It holds the schema, fixtures (valid and invalid scripts) and the runner.
- **CLI:** `agent-scripts run <path|name> [--dir <d>] [--input k=v]… [--file <f>] [--selection <text>|--selection-file <f>] [--json]`, `agent-scripts list [<dir>]`, `agent-scripts validate <path>`. `--json` prints the result record, else `text`. Exit codes: 0 `ok`, 2 usage error (unknown flag, missing input), 3 `failed`, 4 `error`, 5 `timeout`, 6 every other `refused`, 130 and 143 `cancelled` by SIGINT and SIGTERM (on Windows 130 for Ctrl-C and Ctrl-Break). `--expect-sha256 <hash>` makes the runner refuse a file whose content has another hash ("Storage and trust"). The runner, not the schema, enforces the `2h` limit of `timeout`.
- **Versioning:** semver of the crate; the `format` key versions the files. A runner reads every `format` it knows; a breaking change to the format is a new `format` number, never a changed meaning of an old one.
- **In the apps:** poimen and Nixe depend on the crate as a git dependency pinned to a tag (`agent-scripts = { git = "…", tag = "v0.1.0" }`); Renovate updates it. poimen's crates otherwise use `path` dependencies; this is the first git dependency of the workspace. poimen also ships the `agent-scripts` binary of that tag next to `poimen-server` (as it ships `poimen-pty-host`, also in the SSH server archive of ADR 0013), because a pane runs the CLI (below); the app uses the library for listing and validating.
- **Runs:** the runner writes one agent-events run per script run (run start, one task, its end) when it runs in a git working tree (a space of a repository always is; a CLI run outside a repository writes none), as the deliver loop does, so the Runs view of poimen shows it without poimen writing events (ADR 0016 "Relation to agent-events").

### Harness call

The runner calls the harness **in print mode**: a headless process, the prompt as an argument, the answer on stdout, the exit code as the end.

Why not the interactive sessions of [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md): they need `poimen-server` and the hook reports, so neither Nixe nor cron nor CI could run a script. A script is one call with a defined result, not a session someone steers. The cost: nobody can answer the agent while it runs. A question it cannot settle with its tools ends the run; the script's prompt and tools must be written for that.

- **Harness definitions:** the runner has built-in definitions for `claude` and `copilot` with the fields of `poimen-core/src/harness.rs` that print mode needs (`command`, `modelArgs`, `printArgs`), plus two new ones: `toolsArgs` (the tools that exist, `{tools}` replaced by the comma-joined, de-duplicated tool names: `--tools {tools}` and `--available-tools {tools}`) and `allowToolArgs` (repeated per entry, `{tool}` replaced by the entry as written: `--allowedTools {tool}` and `--allow-tool {tool}`). The tool name of a rule entry is the part before its `(` (`Bash(git status)` gives `Bash`), so `toolsArgs` only ever gets names, and the rule narrows what the allowed tool may do. The runner's user config (`agent-scripts/config.json` in the user config directory, `harnesses` keyed by id as in poimen) replaces a built-in by id; a host may pass its own definitions, which win. A harness without `toolsArgs` cannot run a script (`refused`): the runner never starts an agent whose tools it cannot limit. An empty `tools` passes the harness's form of "no tools" (`--tools ""` for claude; the copilot form is verified with #1092).
- **Isolation from the repository's harness settings:** the harness must not read allow rules, permission modes or hooks from the repository it runs in (for claude `.claude/settings.json` and `.claude/settings.local.json` of `{dir}`), because they are not part of the trusted script and could widen a rule entry. A third field, `isolateArgs`, carries this: `--setting-sources user` for claude. For copilot #1092 finds the flag that keeps repository config out; until a harness has `isolateArgs`, it cannot run a script (`refused`). The user's own harness settings still apply: they are the user's, as the login is.
- **Argv:** `command`, then `isolateArgs`, `modelArgs` when a model is set, `toolsArgs`, `allowToolArgs` per entry, then `printArgs`. Expanded per element, never through a shell.
- **Working directory:** `{dir}`: the directory the host or `--dir` gives, else the current directory.
- **Timeout:** `timeout` of the script. At the timeout the runner ends the harness's process tree (SIGTERM, SIGKILL after 5 s; a job object on Windows) and returns `timeout`.
- **One run at a time:** a second run of the same script (its canonical path) is `refused` while one runs, in any directory. The runner holds an OS file lock under its state directory, keyed by the hash of the path; the lock goes with the process, so a crashed run leaves none behind.
- **Login:** the harness runs with the user's existing login. The runner has no API key and no API client.

### poimen as a host

A run started from the app (palette, button, #1094) runs in a new terminal pane of the space, placement `panel` ([ADR 0010](0010-terminal-panel-for-shells-in-the-app.md)), named after the script. The pane runs `agent-scripts run <path> --expect-sha256 <hash> --dir <space dir> …` (the shipped binary), so the user sees the output, can end the run with Ctrl-C, and the run shows in Runs. The pane stays after the run like every terminal ("Non-destructive"). On a remote space it runs on that machine through the server there (#1097).

### Storage and trust

- **Project:** `.agents/scripts/*.md` in the repository root, one level, no subfolders. `.agents/` is the neutral folder that the skills and `.agents/config.json` already use, so poimen, Nixe and the CLI find the same scripts.
- **User:** `scripts/*.md` in the runner's user config directory (`~/.config/agent-scripts/scripts/` on Linux and macOS, `%APPDATA%\agent-scripts\scripts\` on Windows).
- **Nixe:** a vault folder that Nixe's own ADR (GravionLabs/nixe#296) names; the runner takes any directory.
- **Precedence by name:** a project script replaces the user script of the same name, as task modes merge (`tasks::for_project`). Both are listed with their source.
- **Trust:** a project script runs from a host only while the SHA-256 of its content equals the hash the user trusted for that file. poimen stores it as `trustedScripts` in the user `config.json`: the repository root, mapped to the relative path, mapped to the hash; a new or changed script asks again and the dialog shows its frontmatter and prompt, from the same read whose hash it stores. The host checks the hash it trusted and passes it to the runner (`--expect-sha256`); the runner reads the file once, refuses it when that read has another hash, and runs the prompt of that read. A file changed between the check and the run is therefore refused, not run (the rule of `trust.rs`: what was trusted is what runs). A trusted project file does not trust its scripts, and a trusted script does not trust the project file. User scripts need no trust.
- **The CLI** checks no trust: a person who types `agent-scripts run` (or puts it in their own cron) runs that file on purpose, as with any program. The trust check is the hosts' job, because a host starts scripts it found in a repository.

### Relation to what exists

Everything below stays; agent scripts are an addition.

- **`commands[]`:** palette entries that type a command or send a prompt to the space's agent, inside an interactive session. They stay. A `commands[]` entry with `target: agent` is the right tool to prompt a running agent; a script starts a call of its own.
- **Task modes (`tasks.modes[]`):** start an agent or a command for an issue, in a worktree. They stay; a scheduled task mode is #1000.
- **TypeScript scripts (#742, #748, ADR 0016, ADR 0017):** programs that drive sessions, such as the deliver loop. They stay. An agent script is not a program: it has no control flow, only one call.
- **Plugins (ADR 0007):** extend the app in a sandbox. They stay; a plugin may later run a script through the runner, which needs its own decision.
- **ADR 0017:** its sentence "poimen stores and lists no scripts and has no registry" no longer holds for agent scripts: poimen lists the scripts of `.agents/scripts/` and the user directory, and edits them in the app (#1093). It still has no registry, and for every other script the sentence stands. ADR 0017's "no triggers, no schedules, no autostart" is left to #1095.

## Consequences

- One format and one runner for poimen, Nixe, the command line, cron and CI; a fix lands once.
- A new repository with its own release pipeline; poimen and Nixe pin a tag and follow its releases. poimen's workspace gets its first git dependency. poimen ships one more binary (`agent-scripts`) next to the server, on every machine.
- A script cannot ask and cannot be answered. Scripts that need a dialog stay task modes or interactive sessions.
- A script's tools are a list of harness tool names, so a script written for `claude` does not run under `copilot` unchanged. Translating tool names is left out on purpose until two harnesses share a script in practice.
- A harness without a way to limit tools cannot run scripts. Today that is no harness; a new one needs `toolsArgs` first.
- Trust grows a second store (`trustedScripts`) and a dialog per script; a repository with many scripts asks per file, once.
- `changedFiles` depends on git and only reports.
- Follow-ups: the repository and the runner (#1092), the Scripts view (#1093), the palette and button (#1094), triggers (#1095, #1096), remote runs (#1097), Nixe's ADR (GravionLabs/nixe#296).

## Alternatives

- **Interactive sessions as in ADR 0016.** The user could answer the agent, but every run would need `poimen-server` and hooks; Nixe, cron and CI could not run a script.
- **Both modes, chosen per script.** Two code paths and two result shapes before one has been used.
- **A crate inside poimen (b) or one implementation per app (c).** See the table: (b) ties the runner's releases to poimen's, (c) lets two implementations of one format drift apart.
- **`.poimen/scripts/`.** Covered by the folder of the project file, but a poimen name for files that Nixe and the CLI read too.
- **One trust entry for the project file and all its scripts.** Fewer dialogs, but every change to any script asks for all of them, and the dialog grows with the repository.
- **A permission-mode key (`acceptEdits`).** Shorter scripts, but a second way to grant tools next to the list, which print mode cannot narrow at run time.
- **JSON or TOML files.** Easier to parse, but the prompt is the main part of a script and reads and diffs best as Markdown, as `SKILL.md` files do.
