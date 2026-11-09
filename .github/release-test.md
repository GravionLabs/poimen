# Release test: <version>

Release candidate `v<version>-rc<nnn>` of milestone M<n>. The RC test runs beside the release PR
(#<release PR>) and does not gate it. Checklist: [docs/manual-acceptance.md](../docs/manual-acceptance.md).

Install the RC next to the stable app: it installs as **Poimen RC** with its own server, settings
and hooks ("The RC next to the stable app" in the checklist), so the stable poimen keeps working.
The RC does not update itself: M-8, W-S-8 and A-12 (update) are checked on the stable app only,
when the release is published; in the RC test they are **not run** (reason: RC).

Record each run as the checklist describes ("How to record a run"): versions, OS and package with
its SHA-256, then one comment per platform with **pass**, **fail** or **not run** (with a reason) per
item ID. Every gap becomes a `[Bug]` issue on the milestone; link it from the comment.

## All platforms

- [ ] RC-1 Side by side with the stable app

## macOS (`.dmg`)

- [ ] M-1 Install from the disk image
- [ ] M-2 Server starts detached
- [ ] M-3 Server survives closing the app
- [ ] M-4 Hooks offer and install
- [ ] M-5 Agent status idle, working, blocked, done
- [ ] M-6 Start an agent with a prompt
- [ ] M-7 Task start and cleanup
- [ ] M-8 Update to the next version (stable app only)

## Windows (`-setup.exe` and `.msi`)

- [ ] W-S-1 Install (`-setup.exe`), W-M-1 Install (`.msi`)
- [ ] W-S-2 Server starts without a console window
- [ ] W-S-3 Server survives closing the app
- [ ] W-S-4 Hooks offer and install
- [ ] W-S-5 Agent status idle, working, blocked, done
- [ ] W-S-6 Start an agent with a prompt
- [ ] W-S-7 Task start and cleanup
- [ ] W-S-8 Update to the next version (stable app only)

## Linux (`.deb`, `.AppImage`)

- [ ] L-D-1 Install (`.deb`)
- [ ] L-D-2 Server starts detached
- [ ] L-D-3 Server survives closing the app
- [ ] L-D-4 Hooks offer and install
- [ ] L-D-5 Agent status idle, working, blocked, done
- [ ] L-D-6 Start an agent with a prompt
- [ ] L-D-7 Task start and cleanup
- [ ] A-1 Install (`.AppImage`), A-3 Server survives closing the app, A-4 Hooks offer and install
- [ ] A-6 Start an agent with a prompt, A-11 Task start and cleanup, A-12 Update to the next version (stable app only)

The Windows and AppImage open checks (W-S-9 to W-S-14, A-7 to A-10) are run once on the matching
package, as the checklist says.
