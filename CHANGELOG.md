# Changelog

All notable changes to GravionLabs/poimen are documented here.
Versioning follows [Semantic Versioning](https://semver.org/).

---

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



