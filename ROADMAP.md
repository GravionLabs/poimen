# Roadmap

Order of work and release assignment for the open issues. A bare `#<n>` is an issue of this repository.

<!-- roadmap:begin intro -->
Updated 2026-11-05. Last release `v0.9.0`.
<!-- roadmap:end -->

## Where we are

<!-- roadmap:begin where -->
| Epic / Feature                                                                     | State                                                                                                            | Milestone     |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------- |
| #1 [Epic] Workbench: IDE-style shell for herdr sessions                            | open: #64, #148, #169, #180, #182, #183, #186, #318                                                              | not scheduled |
| #2 [Epic] Agent coordination: see and steer agents from poimen                     | open: #11, #170, #179, #181, #184, #185                                                                          | not scheduled |
| #4 [Epic] Delivery: CI, releases and distribution                                  | open: #16                                                                                                        | not scheduled |
| #11 [Feature] Remote spaces with tasks                                             | open: #277, #278, #279, #280, #281, #282, #283, #284                                                             | M13           |
| #16 [Feature] Distribution                                                         | open: #45                                                                                                        | not scheduled |
| #64 [Feature] Usage budget from tamias in poimen                                   | open: #359                                                                                                       | not scheduled |
| #148 [Feature] Own terminal backend without herdr                                  | no open children                                                                                                 | not scheduled |
| #169 [Feature] Switch to the poimen backend and remove herdr                       | open: #173, #174                                                                                                 | not scheduled |
| #170 [Feature] poimen console: CLI and attach                                      | open: #172, #175, #176, #177                                                                                     | not scheduled |
| #179 [Feature] Start a task from a GitHub issue                                    | open: #349                                                                                                       | not scheduled |
| #180 [Feature] Space details in the sidebar: branch, PR, ports, last notification  | open: #251                                                                                                       | not scheduled |
| #181 [Feature] Review agent changes with comments to the agent                     | open: #246, #247                                                                                                 | not scheduled |
| #182 [Feature] Project config: setup scripts, custom commands and terminal presets | open: #257, #258                                                                                                 | not scheduled |
| #183 [Feature] Detect listening ports and open them in the browser                 | no open children                                                                                                 | not scheduled |
| #184 [Feature] Prompt editor with file mentions and drag and drop                  | no open children                                                                                                 | not scheduled |
| #185 [Feature] One task to several agents and compare the results                  | no open children                                                                                                 | not scheduled |
| #186 [Feature] GitHub issues, milestones and pull requests in the app              | open: #237, #238, #239, #334, #335, #336, #337, #338, #339, #340, #341, #342, #343, #344, #345, #346, #347, #348 | not scheduled |
| #318 [Feature] Map directories to GitHub and Azure DevOps organizations            | no open children                                                                                                 | not scheduled |
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

<!-- roadmap:slot done -->

<!-- roadmap:begin plan M10 -->
## M10

Progress: 74 % (20 of 27 issues closed).

| Order | Issue | Title                                                                         | Size | Depends on / decision |
| ----- | ----- | ----------------------------------------------------------------------------- | ---- | --------------------- |
| 1     | #354  | [Bug] fix(github): branch pull request query has an unbalanced brace          | S    |                       |
| 2     | #355  | [Bug] fix(spaces): creating a space silently focuses an existing one          | M    |                       |
| 3     | #356  | [Bug] fix(spaces): closing a space gives no feedback and flashes a disconnect | M    |                       |
| 4     | #360  | [Bug] fix(settings): notification checkboxes are cramped on one line          | S    |                       |
| 5     | #371  | [Bug] fix(notify): taskbar icon notification count keeps growing              | M    |                       |
| 6     | #373  | [PBI] docs(roadmap): update ROADMAP.md for M10 to M14                         | S    |                       |
| 7     | #376  | [Bug] fix(tabs): overflowing tabs cannot be reached with the mouse            | M    |                       |
<!-- roadmap:end -->

<!-- roadmap:begin plan M11 -->
## M11

Progress: 8 % (1 of 12 issues closed).

| Order | Issue | Title                                                                             | Size | Depends on / decision |
| ----- | ----- | --------------------------------------------------------------------------------- | ---- | --------------------- |
| 1     | #246  | [PBI] feat(changes): show review findings as comments in the diff                 | M    |                       |
| 2     | #247  | [PBI] feat(changes): stage, commit and push from the Changes tab                  | M    |                       |
| 3     | #251  | [PBI] feat(sidebar): latest agent notification text per space                     | M    |                       |
| 4     | #257  | [PBI] feat(project): teardown before worktree cleanup and on demand               | M    |                       |
| 5     | #258  | [PBI] feat(project): custom commands and presets in the command palette           | M    |                       |
| 6     | #264  | [PBI] docs(site): screenshots of the Flow tab and the deliver board               | S    |                       |
| 7     | #349  | [PBI] refactor(tasks): task modes start agents through the harness registry       | M    |                       |
| 8     | #350  | [PBI] feat(settings): color scheme on the Appearance page                         | S    |                       |
| 9     | #359  | [PBI] feat(status-bar): Claude usage 5h, 7d, 1d and 1m from tamias                | M    |                       |
| 10    | #361  | [PBI] feat(settings): terminal and UI font family and size on the Appearance page | M    |                       |
| 11    | #370  | [PBI] feat(spaces): rename a space                                                | S    |                       |
<!-- roadmap:end -->

<!-- roadmap:begin plan M12 -->
## M12

Progress: 0 % (0 of 15 issues closed).

| Order | Issue | Title                                                                               | Size | Depends on / decision       |
| ----- | ----- | ----------------------------------------------------------------------------------- | ---- | --------------------------- |
| 1     | #172  | [PBI] docs(adr): poimen console next to the app                                     | S    |                             |
| 2     | #173  | [PBI] feat(backend): poimen as the default backend                                  | S    |                             |
| 3     | #176  | [PBI] feat(cli): poimen attach for one terminal full screen                         | M    | blocked by #172             |
| 4     | #177  | [PBI] build: rename the app binary to poimen-app                                    | S    | blocked by #172             |
| 5     | #175  | [PBI] feat(cli): poimen CLI to list and steer terminals and agents                  | M    | blocked by #172, #177       |
| 6     | #334  | [PBI] feat(github): sub-pages with a left navigation and a shared repository filter | M    |                             |
| 7     | #335  | [PBI] feat(github): readable density, status badges and grouped rows                | M    | blocked by #334             |
| 8     | #336  | [PBI] feat(github): pull request detail pane                                        | M    | blocked by #334             |
| 9     | #337  | [PBI] feat(github): issue detail pane                                               | M    | blocked by #336             |
| 10    | #344  | [PBI] feat(github): Inbox sub-page with what needs me                               | S    | blocked by #335             |
| 11    | #345  | [PBI] feat(settings): agent actions for issues and pull requests in the config      | M    |                             |
| 12    | #346  | [PBI] feat(github): run agent actions at the repository root                        | M    | blocked by #345, #337, #336 |
| 13    | #347  | [PBI] feat(github): run agent actions in a worktree                                 | S    | blocked by #346             |
| 14    | #237  | [PBI] feat(github): review agent action with diff-only mode and posting on GitHub   | S    | blocked by #347             |
| 15    | #348  | [PBI] feat(github): badge for running agent actions on issues and PRs               | S    | blocked by #346             |
<!-- roadmap:end -->

<!-- roadmap:begin plan M13 -->
## M13

Progress: 0 % (0 of 9 issues closed).

| Order | Issue | Title                                                                              | Size | Depends on / decision |
| ----- | ----- | ---------------------------------------------------------------------------------- | ---- | --------------------- |
| 1     | #277  | [PBI] docs(adr): remote machines over poimen-server via SSH                        | S    |                       |
| 2     | #278  | [PBI] feat(server): stdio mode and Linux release builds of poimen-server           | M    | blocked by #277       |
| 3     | #279  | [PBI] feat(remote): machines in the settings and an SSH connection per machine     | M    | blocked by #278       |
| 4     | #280  | [PBI] feat(sidebar): remote machines and their spaces in the sidebar               | M    | blocked by #279       |
| 5     | #281  | [PBI] feat(remote): machine check for prerequisites                                | S    | blocked by #279       |
| 6     | #282  | [PBI] refactor(tasks): move worktree logic into a shared crate the server can call | M    | blocked by #278       |
| 7     | #283  | [PBI] feat(tasks): start a task on a remote machine                                | M    | blocked by #280, #282 |
| 8     | #284  | [PBI] feat(tasks): clean up merged task worktrees on a remote machine              | S    | blocked by #283       |
<!-- roadmap:end -->

<!-- roadmap:begin plan M14 -->
## M14

Progress: 0 % (0 of 10 issues closed).

| Order | Issue | Title                                                                                                  | Size | Depends on / decision |
| ----- | ----- | ------------------------------------------------------------------------------------------------------ | ---- | --------------------- |
| 1     | #45   | [PBI] feat(bundle): signed and notarized macOS bundle on release                                       | M    |                       |
| 2     | #174  | [PBI] refactor(backend): remove the herdr backend                                                      | M    | blocked by #173, #277 |
| 3     | #239  | [PBI] feat(github): start a task from the issues tree                                                  | S    | blocked by #346       |
| 4     | #338  | [PBI] feat(github): review, draft and close actions on a pull request                                  | M    | blocked by #336       |
| 5     | #339  | [PBI] feat(github): merge a pull request, re-run failed checks and set reviewers, labels and assignees | M    | blocked by #338       |
| 6     | #238  | [PBI] feat(github): fix failing checks of a pull request with an agent                                 | M    | blocked by #347, #339 |
| 7     | #340  | [PBI] feat(github): edit an issue from its detail pane                                                 | M    | blocked by #337, #339 |
| 8     | #341  | [PBI] feat(github): create issues and sub-issues and change the parent                                 | M    | blocked by #340       |
| 9     | #342  | [PBI] feat(github): multi-select and bulk actions in the issues tree                                   | M    | blocked by #340       |
| 10    | #343  | [PBI] feat(github): create, edit and close milestones                                                  | S    | blocked by #335, #338 |
<!-- roadmap:end -->

<!-- roadmap:slot plan -->

## Later

<!-- roadmap:begin later -->
| Issue | Title                                                              | Why later             |
| ----- | ------------------------------------------------------------------ | --------------------- |
| #148  | [Feature] Own terminal backend without herdr                       | no milestone          |
| #183  | [Feature] Detect listening ports and open them in the browser      | no milestone          |
| #184  | [Feature] Prompt editor with file mentions and drag and drop       | no milestone          |
| #185  | [Feature] One task to several agents and compare the results       | blocked by #179, #181 |
| #318  | [Feature] Map directories to GitHub and Azure DevOps organizations | no milestone          |
<!-- roadmap:end -->

## Risks

- None known yet. Write the risks here by hand; the script never touches this section.
