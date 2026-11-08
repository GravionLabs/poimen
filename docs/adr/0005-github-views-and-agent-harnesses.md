# ADR 0005: GitHub views and agent harnesses native in poimen

- Status: Accepted; the section "No plugin system now, one later" superseded in part by [ADR 0007](0007-plugin-system-with-sandboxed-worker-and-view-contexts.md)
- Date: 2026-11-04
- Issues: #229, Feature #186
- Supersedes: [ADR 0004](0004-native-views-for-herdr-plugins.md) (plugin view contract)

## Context

poimen runs the GravionLabs herdr plugins of [GravionLabs/herdr-plugins](https://github.com/GravionLabs/herdr-plugins) through herdr's plugin API: actions from the Plugins menu and the command palette, plugin panes as terminal tabs (#39 to #41), and since #43 a native deliver board fed by the plugin's `data` action (ADR 0004).

That design has three problems:

- **It depends on herdr.** Discovery (`plugin.list`), the action runtime and the panes exist only on the herdr backend. ADR 0003 adds the poimen backend and later drops herdr, so every plugin-based view needs a second path.
- **It needs Node and the plugin install** next to poimen on every machine, including Windows, only to show data that `gh` and `git` already deliver.
- **The interesting features are poimen features.** A pull request dashboard where you pick PRs and start reviews with a chosen agent and model, an issues tree, a diff with comments to the agent: they need poimen's tabs, sidebar, palette, settings and terminal spawning. A TUI or a data-only script cannot provide them, and a plugin API rich enough to do so would be designed before its first external user.

Today only the poimen authors write features. There is no third party asking for an extension point.

## Decision

### No plugin system now, one later

poimen builds GitHub views and the agent choice as built-in features. It gets **no plugin or extension system in this phase**. A plugin system is wanted later; it gets its own ADR once the built-in features show which extension points are stable (views, palette commands, harnesses, task modes).

**None of the currently planned issues is built as a plugin**: Features #179, #180, #181, #182 and #186 and their PBIs are built-in. The later plugin system must not require rewriting them; it may expose their extension points (for example additional harnesses or views) on top.

Built-in features keep a shape that a plugin API can wrap later:

- each feature lives in its own folders (`src/app/github/`, `src-tauri/src/github.rs`, `src/app/changes/`, …), not spread over core files;
- it registers in the existing command palette, editor tabs, activity bar and settings instead of adding its own entry points;
- its behaviour that differs between users or skills is configuration (task modes, prompt templates, reported pane tokens), not code. poimen knows no skill by name (#188).

### GitHub through the `gh` CLI

All GitHub access goes through the `gh` CLI (`gh api` for REST and GraphQL), resolved with `gui_path::resolve_program` and started with `child_path()`; on Windows the program lookup honours `PATHEXT`. poimen stores no token and has no OAuth flow; the user's `gh auth` is the login. One shared client (#230) detects the repository of a directory, caches answers for 30 s (REST with ETag) and reports a missing or logged-out `gh` as one state the views show.

### Harness registry with argv templates

The agents poimen can start (Claude Code, GitHub Copilot CLI) are entries of a harness registry (#235): `command`, `modelArgs`, `promptArgs`, `models`, `defaultModel`. Each is an **argv template, never a shell string**: placeholders expand into single arguments, and the command starts without a shell. That keeps quoting identical on Linux, macOS and Windows, where `.cmd` shims and PowerShell quoting would otherwise differ. `agent_start` takes `args: Vec<String>` instead of a whitespace-split string (`herdr::agent_start_params`). Users can add or override entries in the settings.

### The herdr plugins in poimen

poimen removes its herdr plugin integration (#234): the Plugins menu, plugin actions in context menus and palette, plugin panes as tabs, and the plugin-based deliver board of #43. GravionLabs/herdr-plugins is **not archived**; it stays maintained for herdr users. poimen only stops depending on it.

| herdr plugin | In poimen | Where |
|---|---|---|
| `link-trim` | Core, already built | `src/app/terminal-view/links.ts` |
| `command-palette` | Core, already built | `src/app/core/command-palette.ts`; own commands from the project config #258 |
| `agent-notify` | Core, already built | Attention, Feature #8 (`src/app/core/attention.ts`) |
| `repo-open` | Core, already built | Explorer roots and spaces |
| `layout-bootstrap` | Core, moved to the project config | `.poimen/config.json` layout #254, behind the trust check #255 |
| `deliver-view` | Core | Agents tree ordered by state and reported pane tokens; no deliver-specific code |
| `deliver-board` | GitHub, Feature #186 | Issues tree #232, pull request view #231, milestone #233; sidebar PR per space #250 |
| `review-popup` | Reviews, Feature #181 | Findings as comments in the Changes diff #246; reviews started from the PR view #237 |
| `deliver-starter` | Tasks, Feature #179 | Task modes #188, issue picker #189, worktree and space #190, #191, issue links #193 |
| `worktree-cleanup` | Tasks, Feature #179 | Clean up merged task worktrees #192, teardown first #257 |
| `deliver-log` | Configuration | A terminal preset of the project config #258, or a task mode without worktree (#188) |

### ADR 0004

The plugin view contract of ADR 0004 (the `data` action, its envelope and the native views over plugins) is superseded. Its candidate criteria remain a useful reading of which data deserves a native view; the views themselves are now built in.

## Consequences

- One implementation per feature, on both backends, without Node or installed plugins. The GitHub views need only `gh` and `git`.
- poimen re-implements what the plugins did (PR matching, the 30 s cache, review parsing). Accepted: the plugin code is TypeScript for Node, and poimen needs it in Rust and Angular anyway; recorded `gh` fixtures keep the behaviour comparable.
- deliver-specific behaviour only works when configured (task modes, prompt templates). A fresh install shows neutral defaults.
- Users of the herdr plugins in poimen lose them when #234 ships. The built-in replacements in 0.7.0 (#230 to #233) come first; #234 waits for the issues tree #232.
- A third party cannot extend poimen until the later plugin ADR. If such a request comes earlier, it is the trigger for that ADR, not for a quick hook.
- Epic #3 and #163 (plugin panes opened by other clients) are closed when this ADR is accepted.

## Alternatives considered

- **Build the features as poimen extensions now.** An extension API (manifest, sandbox, lifecycle, versioning) before its first external user would be shaped by guesses, and every feature would pay for the indirection. Rejected for now; deferred to the later plugin ADR.
- **Keep the herdr plugins and add native views per plugin (ADR 0004).** Bound to herdr and Node, two places per feature. Rejected.
- **Own GitHub token or OAuth app.** Token storage per platform and a second login next to `gh`. Rejected; `gh` is already required by the deliver workflow.
- **Shell strings for harness commands.** Simple on Unix, breaks on Windows quoting and `.cmd` shims, and turns prompts into injection risks. Rejected.
