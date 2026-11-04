# Changelog

All notable changes to GravionLabs/poimen are documented here.
Versioning follows [Semantic Versioning](https://semver.org/).

---

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



