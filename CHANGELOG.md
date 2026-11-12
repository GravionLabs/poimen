# Changelog

All notable changes to GravionLabs/poimen are documented here.
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [0.17.0] — 2026-11-12

### Bug Fixes
- Split panes show no divider between terminals (#654) (#658)
- **ui**: Draw a gutter between split panes (#654)
- **ui**: Highlight column dividers on hover and drag (#654)
- **pty-host**: Create the ConPTY without INHERIT_CURSOR (#602)
- **server**: Write host lines whole and kill on a connection of its own (#603)
- **server**: Restart a terminal with its full command and offer it in the split menu
- **ui**: Make split pane divider visible (#665) (#666)
- **installer**: Leave a user PATH at the NSIS string limit untouched (#671)
- **ui**: Keep the tone icon visible and add aria values to the Events column grips (#673)
- **bottom-panel**: Fixed width for the last Events column so header and rows align (#673)
- **notify**: UI freezes after hours because waiting OS notifications poll on the main thread (#676) (#677)
- **notify**: The app sends no system notifications (#676)
- **tui**: The console sends no system notifications (#676)


### Documentation
- **adr**: Record the observed ConPTY result of the host check (#602)
- Poimen sends no system notifications (#676)


### Features
- **pty-host**: Poimen-pty-host binary with the host protocol and output ring (#602) (#659)
- **protocol**: Wire types of the host protocol (#602)
- **pty-host**: Poimen-pty-host binary with the host protocol and output ring (#602)
- **server**: Start terminals in PTY hosts and read them as a client (#603) (#660)
- **server**: Start terminals in PTY hosts and read them as a client (#603)
- **server**: Adopt running PTY hosts after a server update or crash (#604) (#662)
- **server**: Adopt running PTY hosts after a server update or crash (#604)
- **app**: Show terminals on an older host version with a restart (#606) (#663)
- **app**: Show terminals on an older host version with a restart (#606)
- **sidebar**: Path in the first line, branch and git status in the second line of a space (#667) (#670)
- **bottom-panel**: Resizable columns in the Events tab, wider task column (#673) (#675)
- **bottom-panel**: Resizable columns with a header and dividers in the Events tab (#673)


### Refactoring
- **bundle**: PATH hook with native NSIS commands instead of PowerShell (#671) (#674)
- **bundle**: PATH hook with native NSIS commands instead of PowerShell (#671)


## [0.16.0] — 2026-11-11

### Bug Fixes
- **cli**: Enable crossterm windows feature for poimen-console (#176)
- **cli**: -h/--help only counts as an option, not inside commands or text (#175)
- **server**: Stdio test sets Config.home after #511 and #278 met on release/next (#634)
- **protocol**: Send clients_changed only to declared clients (#512)
- **tui**: Parse non-ASCII keys in console.prefix without panicking (#515)
- **tui**: Compare paths without the verbatim prefix (#517)
- **tui**: Compare directories through same_dir in the task tests (#517)
- **flow**: Keep the arrows between sequential skills while a subagent runs (#626) (#643)
- **app**: Tab rename failures keep the terminal rename, protocol 5 for the trust handshake (#513)
- **app**: Keep the split layout when the tab-owning pane closes
- **app**: Resolve the successor leaf before the pane close completes
- **release**: Fill the nfpm variables before packaging the Linux CLI (#649)
- **console**: Wait for the split of the preset tab in the palette test (#649)


### Documentation
- Update CHANGELOG.md for v0.16.0 [skip ci]


### Features
- **cli**: Poimen attach for one terminal full screen (#176) (#627)
- **cli**: Poimen attach for one terminal full screen (#176)
- **cli**: Poimen CLI to list and steer terminals and agents (#175) (#631)
- **cli**: Poimen CLI to list and steer terminals and agents (#175)
- **server**: Stdio mode and Linux musl release builds of poimen-server (#630)
- **server**: Space.create applies the resolved layout with a trust handshake (#633)
- **protocol**: Client kinds on the event subscription (#512) (#636)
- **protocol**: Client kinds on the event subscription (#512)
- **tui**: Poimen opens the TUI with sidebar, one pane and a prefix key (#515) (#637)
- **tui**: Poimen opens the TUI with sidebar, one pane and a prefix key (#515)
- **tui**: Tabs and splits with prefix keys (#516) (#639)
- **tui**: Attention marks and notifications, app has priority (#518) (#641)
- **tui**: Command palette, project entries and task start (#517) (#640)
- **tui**: Command palette, project entries and task start (#517)
- **flow**: Larger zoom controls and layout direction in the Flow panel (#625) (#642)
- **agents**: Run title and picker show subject, current task and progress (#632) (#644)
- **app**: Tabs and layout from the server (#513) (#646)
- **app**: Tabs and layout from the server (#513)
- **server**: Space.apply_layout for rebuild and presets on the poimen backend (#645) (#648)
- **app**: Split panes in a terminal tab (#514) (#647)
- **app**: Split panes in a terminal tab (#514)


### Refactoring
- **core**: Share the color scheme and sidebar.rows parsers with the console (#515)


## [0.15.1] — 2026-11-09

### Bug Fixes
- **release**: Set the released version for every poimen crate in Cargo.lock (#620)


### Documentation
- Update CHANGELOG.md for v0.15.1 [skip ci]


## [0.15.0] — 2026-11-09

### Bug Fixes
- **server**: Print usage for --help and reject unknown arguments (#400) (#595)
- **spaces**: Spaces keep their directory and layout after a server restart (#579) (#598)
- **core**: Strip the verbatim prefix from the worktree test fixture path on Windows (#509)
- **core**: Make the worktree tests portable to Windows (#509)
- **terminal**: Claude Code's classic renderer overdraws its output (#502) (#617)
- **terminal**: Load Unicode 11 widths so Claude's classic renderer stays readable
- **agents**: Pane status stays working while a deliver loop it started is running (#540) (#619)
- **agents**: Keep a pane working while a deliver loop it started runs (#540)
- **agents**: Notify the end of a linked run once (#540)


### Documentation
- **adr**: Record the herdr maintenance rule in ADR 0006 (#591)
- **adr**: Record remote machines over poimen-server via SSH (#277) (#594)
- Renumber the PTY host ADR to 0009, update ROADMAP for M13 to M16
- **adr**: Split_id and tab.move in ADR 0006 (#510)
- ROADMAP after the refinement of #510
- PTY host split into M014.1, decisions in ADR 0009 (#574)
- ROADMAP after the refinement of #502 and #540
- ROADMAP after merging #548 into #585 and rescoping #499
- ROADMAP with the PBIs of #582 and #583
- Update CHANGELOG.md for v0.15.0 [skip ci]


### Features
- **release**: RC builds install side by side as Poimen RC (#590) (#599)
- **server**: Separate stable and rc installations by a build-time channel (#590)
- **app**: Show RC in the title and the status bar of an rc build (#590)
- **server**: Tabs and split tree per space (#510) (#618)


### Refactoring
- **core**: Add the poimen-core crate with project, trust, templates, remote and layout (#508) (#596)
- **core**: Git, worktree, github and task start in poimen-core (#509) (#597)
- **core**: Move git, worktree, github and task start into poimen-core (#509)


## [0.14.0] — 2026-11-08

### Bug Fixes
- **sidebar**: Clear the tab drop marker over a refused row, document the tab order (#556)
- **server**: Ignore a broken pipe in the hook test helper (#527) (#546)
- **deliver**: Reducer and panel read the agent-events core (#576)
- **panes**: A renamed agent pane shows its name (#539) (#577)


### Documentation
- **adr**: Plugin system with sandboxed worker and view contexts (#550) (#551)
- **adr**: Terminal ownership in a stable PTY host (#573)
- Update CHANGELOG.md for v0.14.0 [skip ci]


### Features
- **sidebar**: Reorder spaces by drag and drop (#555) (#569)
- **sidebar**: Reorder tabs of a space by drag and drop (#556) (#571)
- **sidebar**: Reorder tabs of a space by drag and drop (#556)
- **deliver**: Reducer and panel read the agent-events core (#576)


### Refactoring
- **ci**: Pages.yml uses the ci VitePress workflow (#568) (#578)


## [0.13.0] — 2026-11-08

### Bug Fixes
- **github**: Contrast for every badge tone, role=img checks, ready-to-close needs closed issues (#335)
- **backend**: Offer a confirmed restart of an outdated poimen-server (#500) (#560)
- **sidebar**: Keep the space actions on line 1 (#521)
- **deliver**: Panel shows a running loop as finished after one PBI session ended (#537) (#565)
- **deliver**: Keep a running loop running when status.json says done with open items (#537)
- **deliver**: Tone ignores the dropped release item of a premature done (#537)
- **deliver**: Delivered entries link to the pull request, never the issue (#538) (#566)
- **deliver**: Show no pull request link for a delivered entry whose prUrl is not a pull request (#538)
- **deliver**: A delivered entry without a pull request event has no prUrl (#538)


### Documentation
- **release**: Run the RC test beside the release PR (#482) (#536)
- **agents**: Name the changed default for existing installations (#526)
- Update CHANGELOG.md for v0.13.0 [skip ci]


### Features
- **github**: Sub-pages with a left navigation and a shared repository filter (#529)
- **github**: Readable density, status badges and grouped rows (#335) (#531)
- **github**: Readable density, status badges and grouped rows (#335)
- **github**: Pull request detail pane (#336) (#532)
- **github**: Issue detail pane (#337) (#533)
- **github**: Inbox sub-page with what needs me (#344) (#534)
- **settings**: Agent actions for issues and pull requests in the config (#345) (#535)
- **github**: Run agent actions at the repository root (#541)
- **github**: Run agent actions in a worktree (#347) (#542)
- **github**: Review agent action with diff-only mode and posting on GitHub (#545)
- **github**: Badge for running agent actions on issues and PRs (#348) (#547)
- **explorer**: Open a terminal in a folder (#489) (#552)
- **explorer**: Fetch or pull a repository and fetch all repositories below a folder (#490) (#557)
- **spaces**: Space overview as an editor tab from the space row (#492) (#558)
- **spaces**: Configuration section in the space overview (#493) (#559)
- **sidebar**: Split the space row into two lines (#521) (#561)
- **sidebar**: Split the space row into two lines (#521)
- **deliver**: Deliver panel reads progress events instead of status.json (#480) (#562)
- **agents**: Events tab in the bottom panel (#481) (#563)
- **templates**: Create the default space template and layout on the first start (#526) (#564)
- **templates**: Seed the default space layout and template on the first start (#526)


## [0.12.0] — 2026-11-08

### Bug Fixes
- **release**: One version for the app and the poimen-* crates, label the status bar versions (#477) (#479)
- **terminal**: Tell xterm.js a Windows emulated stream is ConPTY (#455) (#478)
- **server**: Create the Windows ConPTY in pass-through mode (#455) (#484)
- **agents**: Hooks read the payload without waiting for EOF and log failed reports (#485) (#494)
- **agents**: Retry the transcript watch of a new session after /clear (#486) (#495)
- **panel**: Name Claude Code and GitHub Copilot CLI in the Flow and Output empty states (#496)
- **build**: Raise the initial bundle warning budget to 1.6 MB (#497) (#498)
- **terminal**: Let xterm.js handle the wheel on an emulated stream (#503)
- **release**: Bundle no rpm for a pre-release on Linux (#505) (#506)
- **templates**: Add a default space template for directories no template matches (#524) (#525)
- **server**: Ignore a broken pipe in the hook test helper (#527) (#528)


### Documentation
- **adr**: Poimen console with herdr scope next to the app (#172) (#507)
- Update CHANGELOG.md for v0.12.0 [skip ci]


### Features
- **deliver**: Deliver panel with live progress of running deliver loops (#439) (#475)
- **deliver**: Watch deliver loop status files (#439)
- **deliver**: Deliver panel with live progress of deliver loops (#439)
- **release**: SHA256SUMS and build provenance for the installers (#522) (#523)
- **release**: SHA256SUMS and build provenance for the installers (#522)


## [0.11.0] — 2026-11-06

### Bug Fixes
- **spaces**: Focus the inline rename input (#370)
- Replace the updater signing key (#459) (#460)
- **agents**: Copilot token usage and a testable token row
- **terminal**: Reset the font size by removing the key (#456)
- **settings**: Accept the notification row token (#473) (#474)


### Documentation
- Manual acceptance checklist per OS and package (#461)
- **site**: Screenshot of the Flow tab (#264) (#470)
- Update CHANGELOG.md for v0.11.0 [skip ci]


### Features
- **sidebar**: Latest agent notification text per space (#251) (#440)
- **project**: Teardown before worktree cleanup and on demand (#257) (#442)
- **project**: Custom commands and presets in the command palette (#258) (#447)
- **status-bar**: Claude usage 5h, 7d, 1d and 1m from tamias (#359) (#449)
- **settings**: Terminal and UI font family and size on the Appearance page (#361) (#451)
- **spaces**: Rename a space (#370) (#452)
- **spaces**: Rename a space
- **server**: Tokens per terminal via terminal.report_tokens and a report CLI (#438) (#463)
- **server**: Tokens per terminal via terminal.report_tokens and a report CLI (#438)
- **spaces**: Space templates per directory with agent and layout (#464)
- **spaces**: Match space templates by GitHub owner or Azure DevOps project (#445) (#465)
- **settings**: Spaces page to edit space templates (#446) (#466)
- **explorer**: Filter the directories by name (#467)
- **agents**: Token usage per node in the flow view (#454) (#468)
- **agents**: Token usage per node in the flow view
- **terminal**: Larger default terminal font on Windows (#456) (#469)
- **terminal**: Larger default terminal font on Windows (#456)


### Refactoring
- **tasks**: Task modes start agents through the harness registry (#349) (#448)


## [0.10.0] — 2026-11-06

### Bug Fixes
- **server**: Private socket directory, ship the server as a sidecar (#151)
- **tasks**: Run the tab of a tab-mode task after the trust answer (#191)
- **tasks**: Check for uncommitted changes before a cleanup command (#192)
- **github**: Match a pull request's head branch only when it is not in a fork (#236)
- **changes**: Use the repository root for paths, report real base, bound file reads (#242)
- **changes**: Push explicitly to the same-named branch, literal pathspecs (#247)
- **github**: Balance braces in branch pull request query (#354) (#388)
- **spaces**: Give feedback when creating a space opens an existing one (#355) (#389)
- **spaces**: Show closing state and avoid disconnect flash (#356) (#390)
- **settings**: Stack notification checkboxes (#360) (#391)
- **notify**: Taskbar icon notification count keeps growing (#371) (#393)
- **notify**: Withdraw seen notifications from the tray (#371)
- **notify**: Derive dock badge and tray withdrawal from Attention (#371)
- **tabs**: Overflowing tabs cannot be reached with the mouse (#376) (#394)
- **tabs**: Scroll buttons and all-tabs menu for overflowing tabs (#376)
- **tab-bar**: Keep manual scroll on status updates, cover drag and drop while scrolled
- **tab-bar**: Reveal the tab on every user activation, not only on activeId change
- **build**: Pnpm tauri dev cannot determine which binary to run (#392) (#395)
- **build**: Set default-run so pnpm tauri dev starts the app (#392)
- **build**: Drop stray default-run from workspace.package (#392)
- **spaces**: A space in a parent directory swallows new spaces of its repositories (#397) (#398)
- **spaces**: Match a space only by a pane in the chosen directory (#397)
- **backend**: Report the agent and its session of a terminal (#401) (#402)
- **agents**: The poimen backend never reports done (#406) (#413)
- **agents**: Report done when a turn ends on the poimen backend (#406)
- **ui**: Name the active backend in user-facing text (#407) (#414)
- **client**: Allow socket timeouts after the peer shut down on macOS
- **backend**: Remove the unused ensure_poimen_server command (#409) (#418)
- **agents**: Offer only hook-reporting agent kinds on the poimen backend (#420)
- **spaces**: A task space without terminals is not found on the poimen backend (#411) (#421)
- **spaces**: Find a task space without terminals on the poimen backend
- **spaces**: Skip a labelled space in another directory in find_space
- **app**: Compile changes.rs on Windows
- **app**: Find the bundled poimen-server.exe beside the app
- **app**: Start poimen-server on Unix when the socket directory is loose
- **backend**: Stage poimen-server out of the AppImage mount (#424)
- **agents**: Agent arguments are quoted for the terminal's shell (#426) (#434)
- **agents**: Quote agent arguments for the terminal's shell (#426)
- **agents**: Quote PowerShell metacharacters in agent arguments (#426)
- **backend**: Offer the hooks again when they go missing (#427) (#435)
- **docs**: README describes both backends and all platforms (#428) (#436)
- **docs**: README describes both backends and all platforms (#428)
- **docs**: Make shared README passages backend-neutral (#428)
- **backend**: Hooks status does not stage the server (#429) (#437)


### Documentation
- **roadmap**: Add ROADMAP.md with the plan for M10 to M13 (#351)
- Describe pr_worktree in AGENTS.md (#236)
- **roadmap**: Update ROADMAP.md for M10 to M14 (#373) (#374)
- **roadmap**: Update ROADMAP.md for M10 to M13 (#373)
- **roadmap**: Split M10, new M11, shift M11 to M13 to M12 to M14 (#373)
- **roadmap**: Sizes of the M10 bugs, decision on #355 (#373)
- **spaces**: Update create_space comment to the exact-directory rule (#397)
- Correct stale statements about the poimen backend (#408) (#415)
- **protocol**: Unix socket and named pipe transport
- Windows support in README, download and installation guide
- Update CHANGELOG.md for v0.10.0 [skip ci]


### Features
- **server**: Poimen-server with PTY sessions and socket API (#151) (#363)
- **server**: Poimen-server with PTY sessions and socket API (#151)
- **server**: Headless terminal state and attach snapshot (#152) (#364)
- **backend**: Poimen backend in the app (#153) (#365)
- **backend**: Poimen backend in the app (#153)
- **agents**: Agent status from Claude Code hooks (#154) (#366)
- **agents**: Agent status from GitHub Copilot CLI hooks (#155) (#367)
- **agents**: Start an agent and send a prompt on the poimen backend (#156) (#368)
- **server**: Answer terminal queries while no app is attached (#369)
- **tasks**: Issue picker dialog (#189) (#372)
- **tasks**: Create the worktree for an issue (#375)
- **tasks**: Start a task in a new space (#191) (#377)
- **tasks**: Start a task in a new space
- **tasks**: Clean up merged task worktrees (#192) (#378)
- **tasks**: Clean up merged task worktrees
- **tasks**: Start a task from an issue link in a terminal (#193) (#379)
- **github**: Worktree for a pull request (#236) (#380)
- **github**: Worktree for a pull request (#236)
- **changes**: Diff of a worktree against its base (#242) (#381)
- **changes**: Diff of a worktree against its base
- **changes**: Changes tab with file list and CodeMirror merge view (#243) (#382)
- **changes**: Syntax highlighting in the diff (#244) (#383)
- **changes**: Line comments sent to the agent as one prompt (#245) (#384)
- **changes**: Show review findings as comments in the diff (#246) (#385)
- **changes**: Stage, commit and push from the Changes tab (#247) (#386)
- **changes**: Stage, commit and push from the Changes tab (#247)
- **settings**: Color scheme on the Appearance page (#350) (#399)
- **icon**: Terminal panel around the crook in the app icon (#403) (#404)
- **backend**: Poimen as the default backend (#173) (#405)
- **server**: Local sockets via interprocess (named pipes on Windows) (#412) (#419)
- **client**: Connect through a transport over interprocess local sockets
- **server**: Run the poimen backend on Windows (#416) (#422)
- **protocol,server**: Windows socket and state paths
- **server**: Windows default shell
- **server**: Terminal.detach for clients without socket shutdown
- **app**: Start poimen-server detached on Windows
- **server**: Claude Code and Copilot hooks on Windows


### Refactoring
- **backend**: Backend trait over the herdr client (#150) (#362)
- **backend**: Backend-neutral space and terminal types in the frontend (#153)
- **server**: Listen through the transport


## [0.9.0] — 2026-11-04

### Documentation
- Update CHANGELOG.md for v0.9.0 [skip ci]


### Features
- **status-bar**: Help and update link on the far right, items separated by | (#326)
- **project**: Run the setup command in a new space (#328)
- **project**: Task modes per repository (#329)


## [0.8.0] — 2026-11-04

### Bug Fixes
- **app**: Hide console windows of child processes on Windows (#319)


### Documentation
- Name milestones M<n> while open (#323)
- Name milestones M<n> while open
- Closed milestones are renumbered M1 to M7
- Update CHANGELOG.md for v0.8.0 [skip ci]


### Features
- **tasks**: Task modes in the settings (#304)
- **github**: Shared gh client with repository detection and cache (#306)
- **github**: Pull request view with checks and review state (#231) (#307)
- **github**: Issues tree over native sub-issues with linked PRs (#308)
- **github**: Current milestone with progress (#233) (#309)
- **agents**: Harness registry for Claude Code and Copilot CLI with model choice (#311)
- **sidebar**: Dirty state and ahead/behind next to the branch (#312)
- **project**: .poimen/config.json with layout, setup, teardown, commands and presets (#313)
- **sidebar**: Linked pull request with checks per space (#314)
- **project**: Trust a repository before running its commands (#315)
- **settings**: Split the settings view into categories with a tree and search (#316)
- **app**: Auto-update via tauri-plugin-updater (#320)
- **github**: Recognize M<n> milestone titles for the current milestone (#322)


### Refactoring
- **plugins**: Remove the herdr plugin integration and the plugin-based deliver board (#310)


## [0.7.1] — 2026-11-04

### Bug Fixes
- **docs**: Correct close-space and palette placement wording (#302) (#303)


### Documentation
- Describe the 0.6.1 changes in README and site features (#298) (#299)
- Update CHANGELOG.md for v0.7.1 [skip ci]


## [0.7.0] — 2026-11-04

### Bug Fixes
- **sidebar**: Reach the focused treeitem with ::ng-deep to show the close icon (#290)
- **command-palette**: Popup is centred on the window instead of the terminal (#292) (#297)
- **command-palette**: Centre the popup on the editor area (#292)
- **command-palette**: Fall back to window centre when no tab is open (#292)


### Documentation
- **site**: Stop publishing the ADRs on the site (#289) (#291)
- Update CHANGELOG.md for v0.7.0 [skip ci]


### Features
- **sidebar**: Close a space with an icon left of the chevron (#290) (#295)
- **sidebar**: Close a space with an icon left of the chevron (#290)
- **prompt-bar**: Show the prompt bar only in read-only agent tabs (#293) (#296)


## [0.6.0] — 2026-11-04

### Bug Fixes
- **app**: Build on Windows and talk to herdr over its named pipe (#218) (#220)
- **app**: Build on Windows and talk to herdr over its named pipe (#218)
- **app**: Compile pipe_name only on Windows and in tests (#218)
- **settings**: Add explorer folders on Windows (#221) (#222)
- **agents**: Hide the blue drag handle at the end of flow edges (#219) (#225)
- **app**: Do not mark already-done panes unread when the first real snapshot arrives
- **app**: Open the clicked notification's tab through the plugin's action events
- **app**: Report notification clicks on macOS and bring the window to the front
- **terminal**: Map mouse cells onto the herdr frame grid (#166) (#253)
- **app**: Find herdr on the Windows PATH (#267) (#269)
- Release review 0.6.0 findings (Windows paths, notification wording, docs) (#268) (#271)
- Windows paths in the Rust backend and a shared herdr directory (#268)
- Windows cwd names in the UI and one notification wording (#268)
- Site sidebar from the ADR files, ADR heading form, AGENTS.md ADR 0005 (#268)
- **app**: Keep the herdr config test inside its temp home and reword the ADR 0004 decision
- **app**: Group the pane menu by plugin name and harden its keyboard handling (#273)
- **notifications**: Send done once per pane after it stayed done (#286)
- **app**: Group palette plugin actions and panes by plugin id under the plugin name (#274)


### Documentation
- **site**: Landing page with downloads and installation guide on GitHub Pages (#216) (#261)
- **site**: Landing page, download links, installation guide and decisions (#216)
- **site**: Link the site in README and AGENTS (#216)
- **adr**: GitHub views and agent harnesses native in poimen (#229) (#263)
- **adr**: GitHub views and agent harnesses native in poimen (#229)
- **adr**: Accept ADR 0005 (#229)
- **site**: Real screenshots of sidebar, tabs and color schemes (#216) (#266)
- **site**: Real screenshots of sidebar, tabs and color schemes (#216)
- **site**: Fix the color scheme screenshot file names (#216)
- Update CHANGELOG.md for v0.6.0 [skip ci]


### Features
- **agents**: Flow tab and subagent views for GitHub Copilot CLI panes (#219) (#224)
- **agents**: Flow tab and subagent views for GitHub Copilot CLI panes (#219)
- **agents**: Small muted edge anchors in the flow view (#219)
- **notify**: Attention ring and unread badges (#28) (#227)
- **notify**: Attention ring and unread badges
- **notify**: System notification that jumps to the tab (#29) (#228)
- **notify**: System notification that jumps to the tab
- **notify**: Dock badge with the waiting count (#30) (#240)
- **notify**: Notification center view with recent agent status changes (#31) (#241)
- **agents**: Send a prompt without focusing the terminal (#34) (#262)
- **adr**: GitHub views and agent harnesses native in poimen (#229) (#263)
- **sidebar**: Close a space (#270) (#275)
- **status-bar**: Keyboard shortcuts overview (#272) (#276)
- **sidebar**: Group the context menu by plugin (#273) (#285)
- **sidebar**: Group the context menu by plugin (#273)
- **notifications**: Send done notifications once and add switches to the settings view (#286) (#287)
- **settings**: Notification switches in the settings view (#286)
- **command-palette**: Compact JetBrains-style popup (#274) (#288)
- **command-palette**: Compact JetBrains-style popup (#274)


## [0.5.0] — 2026-11-03

### Bug Fixes
- **plugins**: Persist and prune the plugin pane mapping, scope the close entry (#41)
- **deliver**: Bound the login shell PATH read, guard non-GitHub start, document deviations (#43)
- **agents**: Validate session ids, linear tailer, race-free subagent watch (#102)
- **flow**: Mark failed nested agent calls and document Flow tab limits (#104)
- **agents**: Close the flow tab with its pane (#196)
- **agents**: Draw the connections of the flow view (#199) (#200)
- **app**: Never start a second herdr for a bound but silent socket (#211)
- **app**: Bound herdr auto start by a clock deadline (#211)
- **app**: Clear a failed herdr start once herdr connects and resolve one herdr binary (#213) (#214)


### Documentation
- **adr**: Own terminal backend without herdr (#149) (#158)
- **adr**: Native views for herdr plugins (#42) (#165)
- **adr**: Leaving herdr for native plugin views (#187) (#194)
- **adr**: Leaving herdr for native plugin views (#187)
- **adr**: Label the leaving-herdr triggers as a reading of ADR 0003 (#187)
- Update CHANGELOG.md for v0.5.0 [skip ci]


### Features
- **plugins**: Command palette over plugin actions (#39) (#160)
- **plugins**: Plugin actions in menus and the pane context menu (#40) (#162)
- **plugins**: Plugin panes open as tabs (#41) (#164)
- **plugins**: Plugin panes open as tabs (#41)
- **deliver**: Native deliver board panel (#43) (#167)
- **deliver**: Native deliver board panel (#43)
- **agents**: Output panel with live subagent transcripts (#102) (#168)
- **agents**: Watch and parse Claude Code subagent transcripts (#102)
- **agents**: Output panel state, toggleOutput shortcut and menu item (#102)
- **agents**: Output panel with live subagent transcripts (#102)
- **agents**: Flow graph of runs, skill calls and subagents (#104) (#178)
- **agents**: Flow graph of runs, skill calls and subagents (#104)
- **agents**: Set the bottom panel apart from the editor panes (#195) (#197)
- **agents**: Set the bottom panel apart from the editor panes (#195)
- **agents**: Visible splitter between editor and panel (#195)
- **agents**: Open the flow of a pane as an editor tab (#196) (#198)
- **agents**: Open the flow of a pane as an editor tab (#196)
- **agents**: Status bar button to show the closed output panel (#201) (#202)
- **sidebar**: Frame the sidebar like the bottom panel (#203) (#204)
- **editor**: Frame the editor area like the sidebar and panel (#205) (#206)
- **sidebar**: Frame the activity bar like the other surfaces (#207) (#208)
- **agents**: Icon toolbar and minimap swap in the flow view (#209) (#210)
- **app**: Start the herdr server automatically when it is not running (#211) (#212)
- **app**: Start the herdr server when it is not running
- **app**: Show herdr start failures with Retry and the autoStart setting


### Refactoring
- **agents**: Key subagent watches by follower (#196)
- **app**: Share the GUI PATH resolution in gui_path


## [0.4.0] — 2026-11-02

### Bug Fixes
- **terminal**: Ctrl+click does not open links on Linux and Windows (#91) (#92)
- **terminal**: Highlight links on hover and open them with Cmd (macOS) or Ctrl (#91)
- **terminal**: Open links with Cmd or Ctrl in input mode (#91)
- **layout**: Tab header and terminal follow the light theme (#93) (#95)
- **terminal**: Keep the split shortcut from xterm so it is not sent to the pane as SIGQUIT (#23)
- **terminal**: Trim trailing punctuation of detected links like link-trim (#36)
- **explorer**: Restore expanded folders under a ~ root (#99)
- **activity-bar**: Draw the view icons as inline svgs (#129) (#130)
- **settings**: Selected color scheme is unreadable in dark schemes (#139) (#143)
- **settings**: Do not add an explorer root twice under another spelling (#157)


### Documentation
- **adr**: ADR 0002 color schemes (#124)
- Update CHANGELOG.md for v0.4.0 [skip ci]


### Features
- **tabs**: Split editor groups (#23) (#103)
- **tabs**: Split editor groups (#23)
- **layout**: Restore tabs and layout on restart (#105)
- **app**: Menu bar and editable keyboard shortcuts (#106)
- **panes**: New terminal, split and close pane from the UI (#32) (#107)
- **agents**: Start an agent in a pane and rename agents and panes (#108)
- **terminal**: Copy and paste in terminal tabs (#35) (#109)
- **terminal**: Clickable links open in the system browser (#36) (#119)
- **terminal**: Open OSC 8 hyperlinks through the opener (#36)
- **sidebar**: Configurable rows of the spaces tree (sidebar.rows) (#121)
- **spaces**: Create a space from a directory (#98) (#122)
- **explorer**: Directory tree view in the activity bar (#99) (#123)
- **explorer**: Directory tree view in the activity bar
- **terminal**: Full ANSI palette, selection and cursor colors in xterm (#125)
- **spaces**: Configurable pane layout for a new space (#100) (#126)
- **explorer**: Create a space from a directory in the explorer (#101) (#127)
- **theme**: Color scheme registry with the herdr schemes and chrome derived from the palette (#128)
- **theme**: Light/dark pair in colorSchema follows the theme setting (#114) (#131)
- **theme**: ColorSchema "herdr" follows the herdr theme (#132)
- **theme**: User color schemes from WezTerm TOML files (#135)
- **settings**: Full Ghostty palette over the color scheme (#136)
- **settings**: Settings view with color scheme picker and live preview (#137)
- **sidebar**: Split layout with an Agents view and a toggle in the Spaces header (#138)
- **sidebar**: Splitter between Spaces and Agents in the split layout (#140) (#145)
- **settings**: Manage the explorer roots in the settings view (#141) (#144)
- **settings**: Edit the pane layout of a new space in the settings view (#142) (#146)


## [0.3.0] — 2026-11-02

### Bug Fixes
- **sidebar**: Compact rows and herdr-style info in the spaces tree (#67) (#68)
- **sidebar**: Compact rows and herdr-style info in the spaces tree
- **sidebar**: Two-line pane rows, aggregate workspace state, open-row style
- **sidebar**: Show herdr label for terminal rows, widen tree indent (#72)
- **sidebar**: Uneven row heights, duplicated agent title (#73) (#74)
- **sidebar**: Uniform rows, single agent title, active pane marker (#73)
- **sidebar**: Collapse via right-edge chevron and readable open row names
- **app**: Titles Spaces and Poimen, tab header with workspace, clearer active row (#75) (#78)
- **app**: Titles Spaces and Poimen, tab header with workspace, clearer active row
- **sidebar**: Readable text on the active row tint, workspace in close label (#75)
- **status-bar**: Set released version in manifests, show herdr version without protocol
- **terminal**: Keep read-only tabs read-only, classify held controller, report input after first frame
- **terminal**: Keep the held state on resize, drop stale connects
- **terminal**: Keep padding off the element xterm fits into (#83)
- **terminal**: Mouse clicks do not reach the pane application in input mode (#81) (#84)
- **terminal**: Forward mouse events to the pane application in input mode
- **terminal**: Shift bypasses mouse forwarding, release drags, gate on opened mode
- **terminal**: Open http(s) links in the terminal output in the default browser (#88) (#90)


### Documentation
- Update CHANGELOG.md for v0.3.0 [skip ci]


### Features
- **layout**: Poimen and herdr version in the status bar (#76) (#79)
- **layout**: Show poimen and herdr version in the status bar
- **tabs**: Input mode by default when no other client controls the pane (#77) (#82)
- **tabs**: Input mode by default when no other client controls the pane
- **layout**: Flat mode button instead of a switch in the status bar (#86)
- **layout**: Theme button in the toolbar (light, dark, system) (#87) (#89)


## [0.2.0] — 2026-11-02

### Bug Fixes
- **app**: Feed row text from agent_status and deliver_step, drop guessed fields (#20)
- **sidebar**: Report a second click or Enter on the same row (#60) (#61)
- **terminal**: Propagate the size after a font change (#62) (#63)


### Documentation
- Update CHANGELOG.md for v0.2.0 [skip ci]


### Features
- **layout**: Activity bar, resizable sidebar and main area (#54)
- **sidebar**: Spaces tree with agents and terminals (#20) (#56)
- **sidebar**: Spaces tree with agents and terminals
- **tabs**: Editor tabs for open panes (#21) (#57)
- **layout**: Status bar with connection, waiting agents and active tab (#22) (#58)
- **settings**: Settings file with theme and terminal font (#27) (#59)


## [0.1.0] — 2026-11-02

### Documentation
- **adr**: Frontend stack Angular 22 and helix-ui (#17) (#48)
- Describe the Angular frontend and its checks (#18)
- Update CHANGELOG.md for v0.1.0 [skip ci]


### Features
- **app**: Add poimen app icon (#25) (#47)


### Refactoring
- **app**: Move the spike UI to Angular and helix-ui (#18) (#49)
- **app**: Move the spike UI to Angular 22 and helix-ui (#18)



