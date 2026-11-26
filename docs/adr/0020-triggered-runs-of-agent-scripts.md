# ADR 0020: Triggered runs of agent scripts

- Status: Accepted
- Date: 2026-11-25
- Issues: #1095, Feature #1090
- Builds on: [ADR 0008](0008-remote-machines-over-ssh.md), [ADR 0012](0012-mcp-server-for-agent-sessions.md), [ADR 0014](0014-hibernate-idle-agents.md), [ADR 0019](0019-agent-scripts-format-and-runner.md)
- Supersedes in part: [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md) ("No persistence": "A run starts on request only."), for agent scripts only; [ADR 0017](0017-script-environment-for-runs.md) ("No triggers, no schedules, no autostart. poimen adds no process that runs a script on its own."), for the schedules of agent scripts and of command task modes

## Context

[ADR 0019](0019-agent-scripts-format-and-runner.md) defines agent scripts, runs them in print mode with only the listed tools, and reserves the frontmatter key `triggers`. A user starts every run. Nothing in poimen starts an agent without a user action: [ADR 0016](0016-poimen-hosts-the-sessions-of-scripted-runs.md) says "A run starts on request only." and adds no scheduled task, service, autostart entry or timer.

The pieces a scheduler can use exist. The app starts `poimen-server` detached (`poimen_server::ensure_running`); the server keeps running after the app window closes and is not started at login. Connections to remote machines are held by the app (`src-tauri/src/machines.rs`), not by the local server.

A run nobody watches is a security question. Nobody sees what the agent reads or writes while it happens, and a file or an issue comment it reads can carry instructions (prompt injection).

## Decision

### Owner decisions

Of 2026-11-20:

- Scripts and bots run only in the desktop apps, never on iOS.
- The local `poimen-server` fires a schedule also while the app window is closed.
- On a machine connected over SSH a run may execute in the `poimen-server` of that machine, but it is started from the desktop side. The remote server holds no schedule and starts nothing by itself.

### What this replaces

**ADR 0016**, section "No persistence": the sentence **"A run starts on request only."** no longer holds for agent scripts: an agent script with a schedule may be started by the timer of the local `poimen-server`. The rule stays for everything that is not an agent script: a session of a scripted run (CLI, host module, MCP tool) starts on request only.

**ADR 0017**, "Where scripts live, how they are trusted, how they start": the sentences **"No triggers, no schedules, no autostart."** and **"poimen adds no process that runs a script on its own."** (left to #1095 by ADR 0019) no longer hold for two things: schedules of agent scripts, and the command task modes that the user lists in `schedules[]` (#1000, decided there by the owner). The second is the user's own program, such as the deliver loop; it is not an agent script and not a run of ADR 0016. The sessions such a program starts are requested by it and keep the permissions of ADR 0016: with no app connected, a start in another space is refused. Event triggers and autostart stay excluded. A `schedules` key of a project file starts nothing.

This decision adds a timer in `poimen-server` and the persisted state it needs. It adds no service, autostart entry or scheduled task of the operating system.

### Scheduler

One scheduler in `poimen-server` serves agent scripts (#1096) and command task modes (#1000). Whichever is built first builds it.

- **Definition.** A cron expression of five fields (minute, hour, day of month, month, day of week) and an IANA time zone. Both are required. For a script: `triggers: [{ schedule: "<cron>", timezone: "<tz>" }]` in the frontmatter; an invalid entry makes the script invalid ([ADR 0019](0019-agent-scripts-format-and-runner.md) "Format"). A local time that daylight saving skips fires at the first minute after the gap; a repeated one fires once, the first time.
- **Which scripts.** Project scripts of a trusted hash (`trustedScripts`, ADR 0019). Registering needs the **schedule consent** below. A changed or untrusted file does not run and is shown as waiting for trust; removing or untrusting a script removes its schedule. A user script has no repository and so no working directory: its `triggers` are not registered in the first version, and the list says so.
- **Registration.** The server keeps the registrations (repository, file, trusted hash) in `server.json`, so they hold after a restart without an open space. At every due time it reads the schedule from the file again and starts the run with `--expect-sha256`, so only what was trusted runs.
- **Clock.** Due times are compared with the wall clock, so sleep of the machine counts as downtime. A due time that was handled is persisted before the run starts and is never handled again, also when the clock goes backwards or the server crashes during the start. A new schedule counts from its registration and has nothing to catch up.
- **Missed runs.** After downtime (server stopped, machine asleep or rebooted) a schedule with missed due times runs **once**, at the first server start or wake, when the newest missed time is at most the **grace time of 12 hours** old. Otherwise it is skipped and recorded. Older due times are never replayed.
- **No autostart.** `poimen-server` is not started at login. Schedules fire while the server runs; the app, or any client that starts the server, brings the catch-up.
- **No overlap.** A script, or a schedule of #1000, never has two runs at once. A due time while the last run is still running is skipped and recorded, not queued. For scripts the lock of the runner (ADR 0019 "One run at a time") also covers a run started by hand.
- **Where.** A script run starts as in ADR 0019 "poimen as a host", in a new terminal of the space of the repository, with the repository root as `{dir}`. Without a space the server creates one: for a command mode with the user's layout (#1000), for a script with only the terminal of the run, so that no layout pane starts an agent or a command with full rights unwatched. Nobody is there to give inputs: a script with a required input without a default, or one that uses `{file}` or `{selection}`, is not scheduled, and the list shows why.

- **Script runs.** The result is the status of the runner's record (ADR 0019 "Output"), which the server reads from the exit code of the runner in the terminal.
- **Command modes (#1000).** The server starts the command as the process of the new terminal, not as text typed into a shell (a command given as a shell string runs in the user's shell as that process). The run ends when that process exits, and the exit code is its result; this is what "never two at once" and the last result use. The server keeps the run in `server.json` and the Runs view lists it from there with the trigger "schedule"; poimen writes no agent-events for it, and a run file the command writes itself shows as usual. The ended terminal stays in its tab, with its output and exit code, until the user closes it (#1305). Like every exited terminal it is not kept across a server restart: the next server drops its tab. A run that ends while no server runs is reported to the next server by its host and keeps its tab there; when that host is gone too, the rule for ended terminals from the history applies (ADR 0015).

### Remote machines

The schedule of a script in a remote space is held by the **local** server; the remote server stores no schedule and has no timer for scripts.

- The app reads the scripts of the remote repository over the connection of [ADR 0008](0008-remote-machines-over-ssh.md) and, when the user trusted them (per machine and repository), registers them with the local server.
- At the due time the local server announces it to the app. The app, which holds the connection, starts the run in the remote `poimen-server` with the trusted hash and the limits of a triggered run, and reports start and end back. The local server hands a due time to one client only.
- While no desktop app is connected to the machine (app closed, machine offline or incompatible), the due time stays pending. It is **caught up once on the next connect** when it is still within the grace time, else skipped and recorded. Several missed due times give one run.
- A run whose end the app did not see (lost connection) is read again after the reconnect. Until then the lock of the runner on that machine refuses an overlapping run.

### Limits and consent

- **Schedule consent:** the trust dialog of a script with `triggers` shows the frontmatter, so also the schedule and the tools, and says that the script then runs without a user action, also while the app is closed, with the limited tools below. The server registers a schedule only for a hash trusted with that statement; a hash trusted without it (before the scheduler, or only for manual runs) is asked again. A changed file asks again. There is no confirmation per run.
- **Runs per day:** at most **24** triggered runs per script in any 24 hours by default; the user may set `triggers.maxRunsPerDay` (1 to 96) in the user `config.json`. Every start counts, whatever its end; manual runs do not. A due time over the limit is skipped and recorded.
- **Global pause:** `triggers.paused` in the user `config.json`, switched in the settings. While paused nothing starts, on no machine; due times in a pause are skipped and not caught up. A running run is not ended.
- A single schedule can be switched off in the app without editing the file; this state is stored in the user's settings.
- Both limits apply to the schedules of #1000 too. A project file or a script cannot set or raise them.
- `timeout` of the script (ADR 0019) bounds the duration of a run.

### Tools of a triggered run

**Every triggered run counts as exposed** to prompt injection: it may read content the user did not write (files, issue comments), and nobody watches. It gets **read tools and writes inside its working directory only: no shell, and no writes outside** (no `gh` comments, no push), **even when the script lists them**. Manual runs keep the listed tools.

- The effective tools are the **intersection** of the script's `tools` and a fixed list per harness. A trigger never adds a tool. For `claude` the list is `Read`, `Grep`, `Glob` (read) and `Edit`, `Write` (write). Everything else is dropped: shell, subagents, every MCP tool (also `poimen mcp`), and network tools, because a request can carry data out. The `copilot` list is fixed when its tool names are verified; until then a copilot script is not scheduled.
- Read and write rules are limited to `{dir}` after symlinks are resolved. Writes are also denied, at any depth and whether file or directory, to every path with a component `.git`, `.agents`, `.claude`, `.poimen`, `.vscode`, `.github`, `.husky`, `.envrc`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md` or `AGENTS.md` (compared without case): these run code or instruct later agents. The same holds for the `path` of an output target `file` or `append`. A run whose `{dir}` is the home directory or contains the user configuration of poimen, the runner or the harness is refused. A rule of the script that the runner cannot prove to be inside these bounds is dropped.
- No harness settings of the user or the repository may widen this (narrower than ADR 0019 "Isolation", which keeps the user's settings): no hooks, no plugins, no allow rules, no additional directories are loaded and no MCP server is started. The login is still used.
- What the runner itself runs in `{dir}` (`git status` for `changedFiles`) runs without hooks, `fsmonitor` and other commands configured in the repository.
- The runner enforces all of it when the host marks a run as triggered. A harness for which it cannot guarantee these limits refuses the triggered run (`refused`). It never falls back to the manual limits.
- Dropped tools are named in the run and in the script list.
- The schedules of #1000 are **not** under these limits: `schedules[]` exists only in the user config and runs the user's own program with the user's rights.

### A run cannot ask

A triggered run is print mode (ADR 0019), so nobody can answer it. The runner adds a fixed instruction to the prompt: say so when a decision of the user is needed, in a marked last line with the question. A run with that marker, or with a tool call the harness reports as denied, **ends with status `error`** and the reason (`needs-decision` or the denied tool); the output target is not applied. Files already written inside `{dir}` stay.

### Visibility

- **Runs view:** every triggered run shows with the trigger "schedule" and the machine. Skipped due times (grace time, overlap, limit, pause, trust, no connection) have no run; the server records them as the last result of the schedule.
- **Scripts view and settings:** each schedule with its next run, last result, switch, the global pause and the reason when it cannot run.
- **Notifications:** a failed or skipped triggered run goes to the notification center, never a toast. The server keeps what no app has seen, so what happened while the app was closed appears when it next opens.
- **Telling the user that the server runs schedules:** the trust dialog says it; while at least one schedule is active the app shows it in the status bar with the next run; "Quit poimen and stop server" says that schedules stop with the server.

### Event triggers

**None in the first version**; a schedule is the only trigger. An event trigger starts a run on content that others choose, when they choose: anyone who can comment on an issue could start an agent and write its input. It needs rules for who may trigger, deduplication and rate limits, and GitHub events arrive without a hosted service only by polling. Candidates without a hosted service, for #962: polling `gh` (issues, comments, checks) and a file watch. A webhook needs an open port or a hosted relay and is not a candidate.

## Consequences

- poimen starts agents without a user action for the first time. The limits are fixed in the runner and the server, not in the script.
- `poimen-server` gains a scheduler, persisted state in `server.json` and wire requests (with parsing tests, protocol drift rule). The runner of GravionLabs/agent-scripts gains a triggered mode with the limits above and the needs-decision status; #1096 depends on it.
- A script that comments on GitHub, runs tests or pushes cannot do that on a schedule. It writes files in its directory, and the user acts on them.
- The limits do not make the content safe: an injected run can still write wrong or hostile text into files of the repository, also into files that a later build, test or tool executes (such as `package.json` scripts or a `Makefile`), which the denied paths cannot list completely. The user reviews the changes (`changedFiles`).
- Nothing runs while the server is not running, for example after a reboot until the app is opened; a run missed by more than 12 hours is lost.
- A remote schedule depends on the desktop app being connected, although the local one does not.
- Every triggered run leaves a terminal pane ("Non-destructive"); a frequent schedule collects many until the user closes them.
- Follow-ups: #1096 (script schedules), #1000 (scheduled task modes), #1097 (remote runs), #962 (bots and event triggers).

## Alternatives

- **Start the server at login (login item, systemd unit) or use the scheduler of the operating system.** Runs also before the app was opened, but installs persistence per platform that outlives poimen's own control. Rejected; the catch-up covers the usual gap.
- **The app holds the schedules.** Simple for remote machines, but nothing fires with the window closed, against the owner decision.
- **The remote server holds its schedules.** Runs without a desktop, against the owner decision.
- **The script's tool list also for triggered runs.** No second rule, but a trusted script with a shell would run injected commands unwatched.
- **A per-script opt-out of the limits.** The same risk behind one more dialog.
- **Exposed only when the run reads foreign content.** The server cannot know this before the run, and nobody watches in any case.
- **Confirmation in the app per run.** Safe, but then no run happens unattended.
- **Replay every missed run.** A burst of agents after a holiday, mostly on outdated state.
