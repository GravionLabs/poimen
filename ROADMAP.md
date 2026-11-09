# Roadmap

Order of work and release assignment for the open issues. A bare `#<n>` is an issue of this repository.

<!-- roadmap:begin intro -->
Updated 2026-11-09. Last release `v0.14.0`.
<!-- roadmap:end -->

## Where we are

<!-- roadmap:begin where -->
| Epic / Feature                                                              | State                                                                              | Milestone     |
| --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ------------- |
| #1 [Epic] Workbench: IDE-style shell for agent sessions                     | open: #183, #186, #318                                                             | not scheduled |
| #2 [Epic] Agent coordination: see and steer agents from poimen              | open: #11, #170, #184, #185                                                        | not scheduled |
| #4 [Epic] Delivery: CI, releases and distribution                           | open: #16                                                                          | not scheduled |
| #11 [Feature] Remote spaces with tasks                                      | open: #278, #279, #280, #281, #282, #283, #284                                     | M14, M15      |
| #16 [Feature] Distribution                                                  | open: #45                                                                          | M16           |
| #170 [Feature] poimen console: TUI, CLI and attach                          | open: #175, #176, #177, #510, #511, #512, #513, #514, #515, #516, #517, #518, #519 | M13, M14      |
| #183 [Feature] Detect listening ports and open them in the browser          | no open children                                                                   | not scheduled |
| #184 [Feature] Prompt editor with file mentions and drag and drop           | no open children                                                                   | not scheduled |
| #185 [Feature] One task to several agents and compare the results           | no open children                                                                   | not scheduled |
| #186 [Feature] GitHub issues, milestones and pull requests in the app       | open: #238, #239, #338, #339, #340, #341, #342, #343                               | M16           |
| #318 [Feature] Map directories to GitHub and Azure DevOps organizations     | no open children                                                                   | not scheduled |
| #582 [Feature] Named layouts with a visual editor, templates reference them | no open children                                                                   | not scheduled |
| #583 [Feature] MCP server for agent sessions                                | no open children                                                                   | not scheduled |
<!-- roadmap:end -->

<!-- Generated sections are inserted at the two slots below by `scripts/New-Roadmap.ps1`:
     "Done in M<n> (x.y.z)" (one per closed milestone, with its issue table) and one plan section per open M<n>
     (table: order, issue, title, size, depends on / decision). -->

<!-- roadmap:begin done M1 -->
## Done in M1 (0.2.0)

| Issue | Title                                                                 |
| ----- | --------------------------------------------------------------------- |
| #19   | [PBI] feat(layout): activity bar, resizable sidebar and main area     |
| #20   | [PBI] feat(sidebar): spaces tree with agents and terminals            |
| #21   | [PBI] feat(tabs): editor tabs for open panes                          |
| #22   | [PBI] feat(layout): status bar                                        |
| #27   | [PBI] feat(settings): settings file with theme and terminal font      |
| #60   | [Bug] fix: sidebar ignores a second click on the same row             |
| #62   | [Bug] fix: font change from settings leaves terminal size out of sync |
<!-- roadmap:end -->

<!-- roadmap:begin done M2 -->
## Done in M2 (0.3.0)

| Issue | Title                                                                                    |
| ----- | ---------------------------------------------------------------------------------------- |
| #67   | [Bug] fix(sidebar): compact rows and herdr-style info in the spaces tree                 |
| #70   | [Bug] fix(sidebar): terminal rows show host name and full path                           |
| #73   | [Bug] fix(sidebar): uneven row heights, duplicated agent title                           |
| #75   | [Bug] fix(app): titles Spaces and Poimen, tab header with workspace, clearer active row  |
| #76   | [PBI] feat(layout): poimen and herdr version in the status bar                           |
| #77   | [PBI] feat(tabs): input mode by default when no other client controls the pane           |
| #80   | [Bug] fix(terminal): last character clipped at the right edge after toggling the sidebar |
| #81   | [Bug] fix(terminal): mouse clicks do not reach the pane application in input mode        |
| #85   | [PBI] feat(layout): flat mode button instead of a switch in the status bar               |
| #87   | [PBI] feat(layout): theme button in the toolbar (light, dark, system)                    |
| #88   | [Bug] fix(terminal): links in the terminal output are not clickable                      |
<!-- roadmap:end -->

<!-- roadmap:begin done M3 -->
## Done in M3 (0.4.0)

| Issue | Title                                                                                               |
| ----- | --------------------------------------------------------------------------------------------------- |
| #23   | [PBI] feat(tabs): split editor groups                                                               |
| #24   | [PBI] feat(layout): restore tabs and layout on restart                                              |
| #26   | [PBI] feat(app): menu bar and editable keyboard shortcuts                                           |
| #32   | [PBI] feat(panes): new terminal, split and close pane from the UI                                   |
| #33   | [PBI] feat(agents): start an agent in a pane and rename it                                          |
| #35   | [PBI] feat(terminal): mouse events, selection and copy/paste                                        |
| #36   | [PBI] feat(terminal): clickable links open in the system browser                                    |
| #71   | [PBI] feat(sidebar): configurable rows of the spaces tree (sidebar.rows)                            |
| #91   | [Bug] fix(terminal): Ctrl+click does not open links on Linux and Windows                            |
| #93   | [Bug] fix(layout): tab header and terminal stay dark in light mode                                  |
| #96   | [Bug] fix(terminal): programs asking for the terminal colors get dark colors in the light theme     |
| #97   | [Feature] Spaces from directories                                                                   |
| #98   | [PBI] feat(spaces): create a space from a directory                                                 |
| #99   | [PBI] feat(explorer): directory tree view in the activity bar                                       |
| #100  | [PBI] feat(spaces): configurable pane layout for a new space                                        |
| #101  | [PBI] feat(explorer): create a space from a directory in the explorer                               |
| #110  | [Feature] Color schemes for app and terminal                                                        |
| #111  | [PBI] docs(adr): ADR 0002 color schemes                                                             |
| #112  | [PBI] feat(terminal): full ANSI palette, selection and cursor colors in xterm                       |
| #113  | [PBI] feat(theme): color scheme registry with the herdr schemes and chrome derived from the palette |
| #114  | [PBI] feat(theme): light/dark pair in colorSchema follows the theme setting                         |
| #115  | [PBI] feat(theme): colorSchema "herdr" follows the herdr theme                                      |
| #116  | [PBI] feat(theme): user color schemes from WezTerm TOML files                                       |
| #117  | [PBI] feat(settings): full Ghostty palette over the color scheme                                    |
| #118  | [PBI] feat(settings): settings view with color scheme picker and live preview                       |
| #120  | [PBI] feat(sidebar): split layout with an Agents view and a toggle in the Spaces header             |
| #139  | [Bug] fix(settings): selected color scheme is unreadable in dark schemes                            |
| #140  | [PBI] feat(sidebar): splitter between Spaces and Agents in the split layout                         |
| #141  | [PBI] feat(settings): manage the explorer roots in the settings view                                |
| #142  | [PBI] feat(settings): edit the pane layout of a new space in the settings view                      |
| #147  | [Bug] fix: Add folder… duplicates an explorer root under the home directory                         |
<!-- roadmap:end -->

<!-- roadmap:begin done M4 -->
## Done in M4 (0.5.0)

| Issue | Title                                                                        |
| ----- | ---------------------------------------------------------------------------- |
| #39   | [PBI] feat(plugins): command palette over plugin actions                     |
| #40   | [PBI] feat(plugins): plugin actions in menus and the pane context menu       |
| #41   | [PBI] feat(plugins): plugin panes open as tabs                               |
| #42   | [PBI] docs(adr): native views for herdr plugins                              |
| #43   | [PBI] feat(deliver): native deliver board panel                              |
| #102  | [PBI] feat(agents): output panel with live subagent transcripts              |
| #104  | [PBI] feat(agents): flow graph of runs, skill calls and subagents            |
| #211  | [PBI] feat(app): start the herdr server automatically when it is not running |
| #213  | [Bug] fix: a failed herdr start stays shown after herdr connects             |
<!-- roadmap:end -->

<!-- roadmap:begin done M5 -->
## Done in M5 (0.6.0)

| Issue | Title                                                                                         |
| ----- | --------------------------------------------------------------------------------------------- |
| #28   | [PBI] feat(notify): attention ring and unread badges                                          |
| #29   | [PBI] feat(notify): system notification that jumps to the tab                                 |
| #30   | [PBI] feat(notify): dock badge with the waiting count                                         |
| #31   | [PBI] feat(notify): notification center                                                       |
| #34   | [PBI] feat(agents): send a prompt without focusing the terminal                               |
| #161  | [PBI] build: use Node.js 26                                                                   |
| #166  | [Bug] fix(terminal): mouse selection in Claude Code question dialogs lands on the wrong cells |
| #187  | [PBI] docs(adr): leaving herdr for native plugin views                                        |
| #215  | [PBI] ci(release): build the app installers and attach them to the GitHub release             |
| #216  | [PBI] docs(site): landing page with downloads and installation guide on GitHub Pages          |
| #217  | [PBI] build: use Rust 1.99.1                                                                  |
| #218  | [Bug] fix: the Tauri crate does not build on Windows (Unix-only socket and process APIs)      |
| #219  | [PBI] feat(agents): Flow tab and subagent views for GitHub Copilot CLI panes                  |
| #221  | [Bug] fix(settings): adding an explorer folder fails on Windows                               |
| #229  | [PBI] docs(adr): GitHub views and agent harnesses native in poimen                            |
| #267  | [Bug] fix: Windows auto start cannot find herdr (Unix-only PATH resolution)                   |
| #268  | [PBI] fix: release review 0.6.0 findings (Windows paths, notification wording, docs)          |
| #270  | [PBI] feat(sidebar): close a space                                                            |
| #272  | [PBI] feat(status-bar): ? button with keyboard shortcuts overview                             |
| #273  | [PBI] feat(sidebar): group the context menu by plugin                                         |
| #274  | [PBI] feat(command-palette): compact JetBrains-style popup                                    |
<!-- roadmap:end -->

<!-- roadmap:begin done M6 -->
## Done in M6 (0.7.0)

| Issue | Title                                                                               |
| ----- | ----------------------------------------------------------------------------------- |
| #289  | [PBI] docs(site): stop publishing the ADRs and update the deliver board description |
| #290  | [PBI] feat(sidebar): close a space with an icon left of the chevron                 |
| #292  | [Bug] fix(command-palette): popup is centred on the window instead of the terminal  |
| #293  | [PBI] feat(prompt-bar): show the prompt bar only in read-only agent tabs            |
<!-- roadmap:end -->

<!-- roadmap:begin done M7 -->
## Done in M7 (0.7.1)

| Issue | Title                                                                              |
| ----- | ---------------------------------------------------------------------------------- |
| #298  | [PBI] docs: describe the 0.6.1 changes in README and site features                 |
| #302  | [Bug] fix: docs misdescribe the close-space confirmation and the palette placement |
<!-- roadmap:end -->

<!-- roadmap:begin done M8 -->
## Done in M8 (0.8.0)

| Issue | Title                                                                                           |
| ----- | ----------------------------------------------------------------------------------------------- |
| #46   | [PBI] feat(app): auto-update via tauri-plugin-updater                                           |
| #188  | [PBI] feat(tasks): task modes in the settings                                                   |
| #230  | [PBI] feat(github): shared gh client with repository detection and cache                        |
| #231  | [PBI] feat(github): pull request view with checks and review state                              |
| #232  | [PBI] feat(github): issues tree over native sub-issues with linked PRs                          |
| #233  | [PBI] feat(github): current milestone with progress                                             |
| #234  | [PBI] refactor(plugins): remove the herdr plugin integration and the plugin-based deliver board |
| #235  | [PBI] feat(agents): harness registry for Claude Code and Copilot CLI with model choice          |
| #249  | [PBI] feat(sidebar): dirty state and ahead/behind next to the branch                            |
| #250  | [PBI] feat(sidebar): linked pull request with checks per space                                  |
| #254  | [PBI] feat(project): .poimen/config.json with layout, setup, teardown, commands and presets     |
| #255  | [PBI] feat(project): trust a repository before running its commands                             |
| #301  | [PBI] feat(settings): split the settings view into categories with a tree and search            |
| #317  | [Bug] fix(app): child processes open console windows on Windows                                 |
<!-- roadmap:end -->

<!-- roadmap:begin done M9 -->
## Done in M9 (0.9.0)

| Issue | Title                                                                                |
| ----- | ------------------------------------------------------------------------------------ |
| #256  | [PBI] feat(project): run the setup command in a new space                            |
| #259  | [PBI] feat(project): task modes per repository                                       |
| #265  | [PBI] chore: migrate legacy x.y.z milestones to M<n>                                 |
| #321  | [PBI] feat(github): recognize M<n> milestone titles for the current milestone        |
| #325  | [PBI] feat(status-bar): help and update link on the far right, items separated by \| |
| #330  | [PBI] chore: back-merge main into release/next after v0.8.0                          |
<!-- roadmap:end -->

<!-- roadmap:begin done M10 -->
## Done in M10 (0.10.0)

| Issue | Title                                                                                              |
| ----- | -------------------------------------------------------------------------------------------------- |
| #150  | [PBI] refactor(backend): Backend trait over the herdr client                                       |
| #151  | [PBI] feat(server): poimen-server with PTY sessions and socket API                                 |
| #152  | [PBI] feat(server): headless terminal state and attach snapshot                                    |
| #153  | [PBI] feat(backend): poimen backend in the app                                                     |
| #154  | [PBI] feat(agents): agent status from Claude Code hooks                                            |
| #155  | [PBI] feat(agents): agent status from GitHub Copilot CLI hooks                                     |
| #156  | [PBI] feat(agents): start an agent and send a prompt on the poimen backend                         |
| #171  | [PBI] feat(server): answer terminal queries while no app is attached                               |
| #173  | [PBI] feat(backend): poimen as the default backend                                                 |
| #189  | [PBI] feat(tasks): issue picker dialog                                                             |
| #190  | [PBI] feat(tasks): create the worktree for an issue                                                |
| #191  | [PBI] feat(tasks): start a task in a new space                                                     |
| #192  | [PBI] feat(tasks): clean up merged task worktrees                                                  |
| #193  | [PBI] feat(tasks): start a task from an issue link in a terminal                                   |
| #236  | [PBI] feat(github): worktree for a pull request                                                    |
| #242  | [PBI] feat(changes): diff of a worktree against its base                                           |
| #243  | [PBI] feat(changes): Changes tab with file list and CodeMirror merge view                          |
| #244  | [PBI] feat(changes): syntax highlighting in the diff                                               |
| #332  | [PBI] ci: allow squash and merge on release/**                                                     |
| #350  | [PBI] feat(settings): color scheme on the Appearance page                                          |
| #351  | [PBI] docs(roadmap): add ROADMAP.md with the plan for M10 to M13                                   |
| #354  | [Bug] fix(github): branch pull request query has an unbalanced brace                               |
| #355  | [Bug] fix(spaces): creating a space silently focuses an existing one                               |
| #356  | [Bug] fix(spaces): closing a space gives no feedback and flashes a disconnect                      |
| #357  | [PBI] chore(skills): install the stack skills per project                                          |
| #360  | [Bug] fix(settings): notification checkboxes are cramped on one line                               |
| #371  | [Bug] fix(notify): taskbar icon notification count keeps growing                                   |
| #373  | [PBI] docs(roadmap): update ROADMAP.md for M10 to M14                                              |
| #376  | [Bug] fix(tabs): overflowing tabs cannot be reached with the mouse                                 |
| #392  | [Bug] fix(build): pnpm tauri dev cannot determine which binary to run                              |
| #397  | [Bug] fix(spaces): a space in a parent directory swallows new spaces of its repositories           |
| #401  | [Bug] fix(backend): poimen backend does not report the agent and its session (icons, Output, Flow) |
| #403  | [PBI] feat(icon): terminal panel around the crook in the app icon                                  |
| #406  | [Bug] fix(agents): the poimen backend never reports done                                           |
| #407  | [Bug] fix(ui): herdr wording on the poimen backend                                                 |
| #408  | [Bug] fix(docs): stale statements about the poimen backend                                         |
| #409  | [Bug] fix(backend): ensure_poimen_server is registered but never used                              |
| #410  | [Bug] fix(agents): the pane dialog offers agent kinds the poimen backend cannot track              |
| #411  | [Bug] fix(spaces): a task space without terminals is not found on the poimen backend               |
| #412  | [PBI] feat(server): local sockets via interprocess (named pipes on Windows)                        |
| #416  | [PBI] feat(server): run the poimen backend on Windows                                              |
| #423  | [Bug] fix(backend): poimen-server and its hooks use the transient AppImage mount path              |
| #425  | [PBI] chore: back-merge main into release/next after v0.9.0                                        |
| #426  | [Bug] fix(agents): agent arguments are quoted for sh on Windows shells                             |
| #427  | [Bug] fix(backend): the hooks offer never returns after Not now, even for dead hooks               |
| #428  | [Bug] fix(docs): README still describes shared behavior for herdr or macOS only                    |
| #429  | [Bug] fix(backend): the hooks status check stages the server on the AppImage                       |
<!-- roadmap:end -->

<!-- roadmap:begin done M11 -->
## Done in M11 (0.11.0)

| Issue | Title                                                                               |
| ----- | ----------------------------------------------------------------------------------- |
| #245  | [PBI] feat(changes): line comments sent to the agent as one prompt                  |
| #246  | [PBI] feat(changes): show review findings as comments in the diff                   |
| #247  | [PBI] feat(changes): stage, commit and push from the Changes tab                    |
| #251  | [PBI] feat(sidebar): latest agent notification text per space                       |
| #257  | [PBI] feat(project): teardown before worktree cleanup and on demand                 |
| #258  | [PBI] feat(project): custom commands and presets in the command palette             |
| #264  | [PBI] docs(site): screenshots of the Flow tab and the deliver board                 |
| #349  | [PBI] refactor(tasks): task modes start agents through the harness registry         |
| #359  | [PBI] feat(status-bar): Claude usage 5h, 7d, 1d and 1m from tamias                  |
| #361  | [PBI] feat(settings): terminal and UI font family and size on the Appearance page   |
| #370  | [PBI] feat(spaces): rename a space                                                  |
| #430  | [PBI] ci: check the server crates without the Tauri libraries and run jobs per area |
| #432  | [PBI] docs: manual acceptance checklist per OS and package                          |
| #433  | [PBI] ci(release): pre-release from release/next for the manual acceptance          |
| #438  | [PBI] feat(server): tokens per terminal via terminal.report_tokens and a report CLI |
| #443  | [Feature] Space templates per directory and remote                                  |
| #444  | [PBI] feat(spaces): space templates per directory with agent and layout             |
| #445  | [PBI] feat(spaces): match space templates by GitHub owner or Azure DevOps project   |
| #446  | [PBI] feat(settings): Spaces page to edit space templates                           |
| #450  | [PBI] feat(explorer): filter the directories by name                                |
| #454  | [PBI] feat(agents): token usage per node in the flow view                           |
| #456  | [PBI] feat(terminal): larger default terminal font on Windows                       |
| #471  | [PBI] chore: back-merge main into release/next after v0.10.0                        |
| #473  | [Bug] fix(settings): row token notification is missing from the Rust ROW_TOKENS     |
<!-- roadmap:end -->

<!-- roadmap:begin done M11.1 -->
## Done in M11.1 (0.13.0)

| Issue | Title                                                                                                       |
| ----- | ----------------------------------------------------------------------------------------------------------- |
| #439  | [PBI] feat(deliver): Deliver panel with live progress of running deliver loops                              |
| #455  | [Bug] fix(terminal): mouse clicks are ignored by Copilot CLI on Windows with the poimen backend             |
| #477  | [Bug] fix(release): poimen-server reports 0.1.0 after the release and the status bar versions are unlabeled |
| #485  | [Bug] fix(agents): Copilot status of some spaces is never updated on Windows                                |
| #486  | [Bug] fix(agents): Flow and Output keep the previous session after /clear                                   |
| #487  | [Bug] fix(panel): Flow and Output empty states still say Claude Code only                                   |
| #497  | [Bug] fix(build): initial bundle exceeds the 1.5 MB warning budget                                          |
| #504  | [Bug] fix: terminal does not scroll with the wheel without a selection                                      |
| #524  | [Bug] fix(templates): no default space template for spaces without a matching template                      |
| #527  | [Bug] fix(server): hook test fails with a broken pipe when the hook exits early                             |
<!-- roadmap:end -->

<!-- roadmap:begin done M12 -->
## Done in M12 (0.14.0)

| Issue | Title                                                                                      |
| ----- | ------------------------------------------------------------------------------------------ |
| #172  | [PBI] docs(adr): poimen console with herdr scope next to the app                           |
| #237  | [PBI] feat(github): review agent action with diff-only mode and posting on GitHub          |
| #334  | [PBI] feat(github): sub-pages with a left navigation and a shared repository filter        |
| #335  | [PBI] feat(github): readable density, status badges and grouped rows                       |
| #336  | [PBI] feat(github): pull request detail pane                                               |
| #337  | [PBI] feat(github): issue detail pane                                                      |
| #344  | [PBI] feat(github): Inbox sub-page with what needs me                                      |
| #345  | [PBI] feat(settings): agent actions for issues and pull requests in the config             |
| #346  | [PBI] feat(github): run agent actions at the repository root                               |
| #347  | [PBI] feat(github): run agent actions in a worktree                                        |
| #348  | [PBI] feat(github): badge for running agent actions on issues and PRs                      |
| #480  | [PBI] feat(deliver): Deliver panel reads progress events instead of status.json            |
| #481  | [PBI] feat(agents): Events tab in the bottom panel next to Output, colored by tone         |
| #482  | [PBI] docs(release): the RC test runs beside the release PR instead of gating it           |
| #483  | [PBI] ci(release): channel from the branch, rc on release/next and beta elsewhere          |
| #488  | [PBI] chore(deps): update helix-ui and Angular to the latest 22.x                          |
| #489  | [PBI] feat(explorer): open a terminal in a folder                                          |
| #490  | [PBI] feat(explorer): fetch or pull a repository and fetch all repositories below a folder |
| #491  | [Feature] Space overview with panes, sessions and configuration                            |
| #492  | [PBI] feat(spaces): space overview as an editor tab from the space row                     |
| #493  | [PBI] feat(spaces): configuration section in the space overview                            |
| #500  | [Bug] fix(backend): an outdated poimen-server keeps running after an update                |
| #505  | [Bug] fix(release): Linux pre-release installers fail on the rpm version                   |
| #521  | [PBI] feat(spaces): split the space row into two lines                                     |
| #522  | [PBI] feat(release): SHA256SUMS and build provenance for the installers                    |
| #526  | [PBI] feat(templates): create the default space template and layout on the first start     |
| #537  | [Bug] fix(deliver): panel shows a running loop as finished after one PBI session ended     |
| #538  | [Bug] fix(deliver): delivered entries link to the issue instead of the pull request        |
| #539  | [Bug] fix(panes): renaming a pane does not work reliably                                   |
| #544  | [PBI] feat(deliver): reducer and panel read the agent-events core                          |
| #554  | [Feature] Reorder spaces and tabs by drag and drop in the sidebar                          |
| #555  | [PBI] feat(sidebar): reorder spaces by drag and drop                                       |
| #556  | [PBI] feat(sidebar): reorder tabs of a space by drag and drop                              |
| #567  | [PBI] ci: run the Rust jobs only on Rust changes, auto-merge                               |
| #568  | [PBI] refactor: pages.yml uses the ci VitePress workflow                                   |
| #575  | [PBI] docs(adr): terminal ownership in a stable PTY host                                   |
<!-- roadmap:end -->

<!-- roadmap:slot done -->

<!-- roadmap:begin plan M13 -->
## M13

Progress: 46 % (6 of 13 issues closed).

| Order | Issue | Title                                                                                | Size | Depends on / decision       |
| ----- | ----- | ------------------------------------------------------------------------------------ | ---- | --------------------------- |
| 1     | #502  | [Bug] fix(terminal): Claude Code's classic renderer overdraws its output             | M    | needs decision              |
| 2     | #510  | [PBI] feat(server): tabs and split tree per space in the protocol                    | M    |                             |
| 3     | #540  | [Bug] fix(agents): agent status goes idle while a deliver loop it started is running |      | needs refinement            |
| 4     | #574  | [PBI] feat(server): PTY host so a server update keeps the terminals (ADR 0009)       | L    | needs decision              |
| 5     | #590  | [PBI] build(release): RC builds install side by side as Poimen RC                    | M    |                             |
| 6     | #417  | [PBI] chore(release): manual check of the poimen backend on Windows                  | S    | release test, does not gate |
| 7     | #600  | [PBI] test: release candidate test of M013 on macOS, Windows, Linux                  |      | release test, does not gate |
<!-- roadmap:end -->

<!-- roadmap:begin plan M14 -->
## M14

Progress: 0 % (0 of 14 issues closed).

| Order | Issue | Title                                                                               | Size | Depends on / decision       |
| ----- | ----- | ----------------------------------------------------------------------------------- | ---- | --------------------------- |
| 1     | #176  | [PBI] feat(cli): poimen attach for one terminal full screen                         | M    |                             |
| 2     | #177  | [PBI] build: rename the app binary to poimen-app                                    | S    |                             |
| 3     | #175  | [PBI] feat(cli): poimen CLI to list and steer terminals and agents                  | M    | blocked by #177             |
| 4     | #278  | [PBI] feat(server): stdio mode and Linux release builds of poimen-server            | M    |                             |
| 5     | #511  | [PBI] feat(server): space.create applies the resolved layout with a trust handshake | M    | blocked by #510             |
| 6     | #512  | [PBI] feat(protocol): client kinds on the event subscription                        | S    |                             |
| 7     | #513  | [PBI] feat(app): tabs and layout from the server                                    | M    | blocked by #510, #511       |
| 8     | #514  | [PBI] feat(app): split panes in a terminal tab                                      | M    | blocked by #513             |
| 9     | #515  | [PBI] feat(tui): poimen opens the TUI with sidebar, one pane and a prefix key       | L    | blocked by #176, #177, #510 |
| 10    | #516  | [PBI] feat(tui): tabs and splits with prefix keys                                   | M    | blocked by #515             |
| 11    | #517  | [PBI] feat(tui): command palette, project entries and task start                    | M    | blocked by #511, #515       |
| 12    | #518  | [PBI] feat(tui): attention marks and notifications, app has priority                | M    | blocked by #512, #515       |
| 13    | #519  | [PBI] build: ship the poimen binary in the installers and on the PATH               | S    | blocked by #175, #177       |
| 14    | #588  | [PBI] test: release candidate test of M014 on macOS, Windows, Linux                 |      | release test, does not gate |
<!-- roadmap:end -->

<!-- roadmap:begin plan M15 -->
## M15

Progress: 0 % (0 of 8 issues closed).

| Order | Issue | Title                                                                              | Size | Depends on / decision       |
| ----- | ----- | ---------------------------------------------------------------------------------- | ---- | --------------------------- |
| 1     | #279  | [PBI] feat(remote): machines in the settings and an SSH connection per machine     | M    | blocked by #278             |
| 2     | #280  | [PBI] feat(sidebar): remote machines and their spaces in the sidebar               | M    | blocked by #279             |
| 3     | #281  | [PBI] feat(remote): machine check for prerequisites                                | S    | blocked by #279             |
| 4     | #282  | [PBI] refactor(tasks): move worktree logic into a shared crate the server can call | S    | blocked by #278             |
| 5     | #283  | [PBI] feat(tasks): start a task on a remote machine                                | M    | blocked by #280, #282       |
| 6     | #284  | [PBI] feat(tasks): clean up merged task worktrees on a remote machine              | S    | blocked by #283             |
| 7     | #586  | [PBI] refactor(app): backend-neutral snapshot wire shape and model                 | M    | blocked by #513             |
| 8     | #589  | [PBI] test: release candidate test of M015 on macOS, Windows, Linux                |      | release test, does not gate |
<!-- roadmap:end -->

<!-- roadmap:begin plan M16 -->
## M16

Progress: 0 % (0 of 16 issues closed).

| Order | Issue | Title                                                                                                  | Size | Depends on / decision       |
| ----- | ----- | ------------------------------------------------------------------------------------------------------ | ---- | --------------------------- |
| 1     | #45   | [PBI] feat(bundle): signed and notarized macOS bundle on release                                       | M    |                             |
| 2     | #239  | [PBI] feat(github): start a task from the issues tree                                                  | S    |                             |
| 3     | #338  | [PBI] feat(github): review, draft and close actions on a pull request                                  | M    |                             |
| 4     | #339  | [PBI] feat(github): merge a pull request, re-run failed checks and set reviewers, labels and assignees | M    | blocked by #338             |
| 5     | #238  | [PBI] feat(github): fix failing checks of a pull request with an agent                                 | M    | blocked by #339             |
| 6     | #340  | [PBI] feat(github): edit an issue from its detail pane                                                 | M    | blocked by #339             |
| 7     | #341  | [PBI] feat(github): create issues and sub-issues and change the parent                                 | M    | blocked by #340             |
| 8     | #342  | [PBI] feat(github): multi-select and bulk actions in the issues tree                                   | M    | blocked by #340             |
| 9     | #343  | [PBI] feat(github): create, edit and close milestones                                                  | S    | blocked by #338             |
| 10    | #501  | [PBI] feat(agents): opt-in classic Claude Code renderer in poimen terminals                            | S    | blocked by #502             |
| 11    | #548  | [PBI] feat(deliver): milestone panel for runs on a GitHub milestone                                    |      |                             |
| 12    | #580  | [PBI] feat(settings): polish of the Spaces page                                                        | S    |                             |
| 13    | #581  | [PBI] feat(settings): export and import settings as JSON                                               | M    |                             |
| 14    | #584  | [PBI] feat(deliver): Runs view with filters and event timeline                                         | M    |                             |
| 15    | #585  | [PBI] feat(deliver): expandable milestone overview with issue states                                   | M    | blocked by #584             |
| 16    | #587  | [PBI] test: release candidate test of M016 on macOS, Windows, Linux                                    |      | release test, does not gate |
<!-- roadmap:end -->

<!-- roadmap:slot plan -->

## Later

<!-- roadmap:begin later -->
| Issue | Title                                                                  | Why later                             |
| ----- | ---------------------------------------------------------------------- | ------------------------------------- |
| #174  | [PBI] refactor(backend): remove the herdr backend                      | blocked by #515, #574                 |
| #183  | [Feature] Detect listening ports and open them in the browser          | no milestone                          |
| #184  | [Feature] Prompt editor with file mentions and drag and drop           | no milestone                          |
| #185  | [Feature] One task to several agents and compare the results           | no milestone                          |
| #318  | [Feature] Map directories to GitHub and Azure DevOps organizations     | no milestone                          |
| #499  | [PBI] feat(updater): shared update channels (stable/rc) for Tauri apps | needs decision                        |
| #582  | [Feature] Named layouts with a visual editor, templates reference them | needs decision; blocked by #513, #514 |
| #583  | [Feature] MCP server for agent sessions                                | needs decision; blocked by #175       |
<!-- roadmap:end -->

## Risks

- **#510 blocks most of M14** (#511, #513, #514, #515 and everything after the TUI start). It is refined (2026-11-09) and should go first.
- **#574 (PTY host) is size L** with a new process type, a host protocol and Windows ConPTY. Split it into PBIs before it starts; #174 (herdr removal) waits for it.
- **Protocol 4 is a breaking change** (ADR 0006): a server left running from an older version needs the confirmed restart until #574 lands.
- **Overlaps to resolve in refinement:** #548 and #585 (two milestone panels), #499 and #590 (RC update channel vs. RC without updater).
- Write further risks here by hand; the script never touches this section.
