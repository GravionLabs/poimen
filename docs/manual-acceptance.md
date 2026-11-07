# Manual acceptance

Some behavior of the poimen backend can only be checked by hand, on the real operating system and
the installed package: job objects and breakaway, elevated servers, PowerShell hooks, ConPTY, the
AppImage mount, App Translocation of a moved app. CI has no desktop session, no real package mount
and no user settings of Claude Code or Copilot CLI, so these checks stay manual.

Run the checklist for every release, on the published build (the packages `release.yml` attaches to
the GitHub release), not on a local build. Each section below is one OS and package. Items are
numbered per section (for example `W-S-4`) so that a result and a bug can name the exact item.

Each item lists **Steps**, **Expected** and **Source**, the issue the check comes from. When a step
names a path, use the path the app shows, not a guess.

## How to record a run

1. Note the versions: poimen (status bar), backend server version (status bar), OS and build,
   package file name with its SHA-256 (`shasum -a 256 <file>` on macOS and Linux,
   `Get-FileHash <file>` on Windows), Claude Code (`claude --version`), Copilot CLI
   (`copilot --version`).
2. For every item write **pass**, **fail** or **not run**, with the date. A not-run item needs a
   reason.
3. Post one comment per package on the release's manual-check issue. For M10 that is #417. Use the
   item IDs in the comment.
4. Every gap becomes a `[Bug]` issue (`github-issue` conventions, for example
   `[Bug] fix(backend): ...`) that names the item ID and the package. Link the bug from the comment.
   An item with a gap is not **pass**.
5. The release counts as manually checked when every item is **pass** or has a linked bug.

Keep the exact error text. For an elevated-server denial or a hook failure, paste the message as it
was shown.

## macOS: `.dmg`

### M-1 Install from the disk image

- **Steps:** open `poimen_<version>_<arch>.dmg`, drag poimen to Applications, eject the image, start
  poimen from Applications.
- **Expected:** the app starts. Gatekeeper shows at most its first-open prompt. The status bar shows
  the poimen backend and its server version.
- **Source:** #46 (packaging and updater).

### M-2 Server starts detached

- **Steps:** start poimen. Run `pgrep -fl poimen-server`.
- **Expected:** one `poimen-server` runs. Its parent is not poimen (`ps -o ppid= -p <pid>` shows
  launchd, PID 1).
- **Source:** #153, #416.

### M-3 Server survives closing the app

- **Steps:** open a terminal in a space and type a marker line. Quit poimen with Cmd+Q. Run
  `pgrep -fl poimen-server`. Start poimen again.
- **Expected:** the server is still running with the same PID. The terminal is still there with the
  marker line.
- **Source:** #153.

### M-4 Hooks offer and install

- **Steps:** with no poimen hooks in `~/.claude/settings.json` or `~/.copilot/hooks`, start poimen.
  Accept the offer. Run `grep -c poimen-agent-hook ~/.claude/settings.json`. Choose **Not now**,
  restart poimen.
- **Expected:** the offer appears. After install both files hold poimen hook entries. After **Not
  now** the offer returns on the next start while the hooks are missing.
- **Source:** #173, #427.

### M-5 Agent status idle, working, blocked, done

- **Steps:** in a space start Claude Code. Send a prompt. Ask for a step that needs permission. Answer
  it. Let the agent finish. Look at the tab and the sidebar row each time, and view the tab only at
  the end.
- **Expected:** working while it runs, blocked when it waits for permission (tab and row ring),
  working again after the answer, done when it finishes. The row counts the unread done state until
  the tab is viewed.
- **Source:** #154, #286.

### M-6 Start an agent with a prompt

- **Steps:** from poimen start Claude Code in a space with a prompt text.
- **Expected:** the agent starts in a new pane and receives the prompt. Its status changes as in M-5.
- **Source:** #156.

### M-7 Task start and cleanup

- **Steps:** start a task for an issue of a repository with a task mode. When the branch is merged,
  clean up the task.
- **Expected:** the worktree and a new space exist after the start. After cleanup the worktree is
  gone and the space is closed.
- **Source:** #191, #192.

### M-8 Update to the next version

- **Steps:** run the previous version with a running agent. Click **Check for updates**, then
  **Install and restart**.
- **Expected:** poimen restarts into the new version. The terminals and the agent still run. The
  hooks still report status. Copies of other server versions are removed once they stop.
- **Source:** #46, `docs/updater.md`.

### M-9 App Translocation of a moved app

- **Steps:** run poimen from `~/Downloads` (or from the mounted image) without moving it to
  Applications. Run `ps -o args= -C poimen-server` and `grep poimen-server ~/.claude/settings.json`.
  Quit and start again from the same place.
- **Expected:** the server and the hook commands do not point into a translocated path
  (`/private/var/folders/.../AppTranslocation/`), or poimen asks to be moved to Applications. Record
  which one happens. A translocated path that breaks after a restart is a bug.
- **Source:** #432 (no issue yet; file a bug on failure).

## Windows: `-setup.exe`

The open M10 checks (W-S-9 to W-S-14) run once, here. The `.msi` section repeats the core flows.

### W-S-1 Install

- **Steps:** run `poimen_<version>_x64-setup.exe`, accept the defaults, start poimen from the Start
  menu.
- **Expected:** the installer finishes without error. poimen starts. The status bar shows the backend
  and its server version.
- **Source:** #417, #416.

### W-S-2 Server starts without a console window

- **Steps:** start poimen. Open Task Manager and look for `poimen-server.exe`.
- **Expected:** the process runs from `%LOCALAPPDATA%\poimen-server\bin\<version>\` (the staged
  copy) and no console window appears.
- **Source:** #416, #417.

### W-S-3 Server survives closing the app

- **Steps:** open a PowerShell terminal in a space and type a marker line. Close poimen. Check Task
  Manager. Start poimen again.
- **Expected:** `poimen-server.exe` still runs with the same PID. The terminal is intact.
- **Source:** #416, #417.

### W-S-4 Hooks offer and install (Claude Code and Copilot CLI)

- **Steps:** with no poimen hooks, start poimen and accept the offer. Check
  `%USERPROFILE%\.claude\settings.json` and `%USERPROFILE%\.copilot\hooks`. Choose **Not now**, restart
  poimen.
- **Expected:** both files hold poimen hook entries after install. The offer returns after **Not now**
  while the hooks are missing.
- **Source:** #173, #416, #427.

### W-S-5 Agent status idle, working, blocked, done

- **Steps:** in one space start Claude Code and in another GitHub Copilot CLI, both in PowerShell.
  Run M-5 in each.
- **Expected:** both report status as in M-5, in the sidebar and the Output and Flow panels. Agent
  icons match the agent.
- **Source:** #154, #155, #417.

### W-S-6 Start an agent with a prompt

- **Steps:** from poimen start Claude Code with a prompt, in a `cmd.exe` terminal and in PowerShell.
- **Expected:** the agent starts in both shells and receives the prompt.
- **Source:** #156, #417.

### W-S-7 Task start and cleanup

- **Steps:** start a task for an issue in a repository with a task mode, then clean it up after the
  merge.
- **Expected:** the worktree and space exist after the start. After cleanup the worktree is removed.
  If an agent in the task still runs, cleanup says so and does not remove the worktree.
- **Source:** #191, #192.

### W-S-8 Update to the next version

- **Steps:** run the previous version with a running agent and a PowerShell terminal. Check for
  updates, then install and restart.
- **Expected:** the update installs although `poimen-server.exe` of the old version runs. The
  terminals survive. The new version starts its own copy only when no server answers.
- **Source:** #46, `docs/updater.md`.

### W-S-9 Breakaway denied inside a job

- **Steps:** start poimen from a parent process that puts it in a job object which forbids breakaway
  (record the launcher used). Start the server through poimen.
- **Expected:** poimen starts the server or shows a clear error. It never hangs, and it does not leave
  the backend unset without a message. Record the exact text.
- **Source:** #416 (open check, PR #422).

### W-S-10 Elevated-server denial text

- **Steps:** start `poimen-server.exe` as administrator. Start poimen without elevation and try to
  connect.
- **Expected:** the connection is refused with the clear message from the plan of #412 and #416
  (a text that says the server runs elevated and poimen must be started the same way). Record the
  exact text.
- **Source:** #416 (open check, PR #422).

### W-S-11 Rename over a running staged copy

- **Steps:** keep the staged copy of the current version running. Run the setup of the next version.
- **Expected:** the installer replaces the installed `poimen-server.exe` without an error about a
  locked file. The running copy keeps serving its terminals. The new version starts its own copy only
  after the old server stops.
- **Source:** #416 (open check, PR #422), `docs/updater.md`.

### W-S-12 Claude Code honors `"shell": "powershell"`

- **Steps:** open `%USERPROFILE%\.claude\settings.json` and find the poimen hook entries. Check the
  `shell` key of each. Run a prompt in Claude Code and watch the status.
- **Expected:** each entry sets `"shell": "powershell"`. Claude Code runs the hook and the status
  changes in poimen. If the status does not change, the hook did not run; record the Claude Code
  version.
- **Source:** #416 (open check, PR #422).

### W-S-13 Copilot CLI loads the hooks on Windows

- **Steps:** check that `%USERPROFILE%\.copilot\hooks` holds the poimen hook. Open three spaces from
  a layout (panes `agent`, `tests`, `shell`), start Copilot CLI in the `agent` pane of each, run a
  prompt in each and watch the status. Repeat with Claude Code.
- **Expected:** Copilot CLI loads the hook and the status changes in poimen, in every space.
  `%LOCALAPPDATA%\poimen-server\hooks.log` has no new line; if it has one, its reason goes into
  the issue.
- **Source:** #416 (open check, PR #422), #155, #485.

### W-S-14 ConPTY cursor and IME

- **Steps:** in PowerShell run `Write-Host "abc"` and move the cursor with the arrow keys. Resize the
  window and run a full-screen program (for example `less` on a long file). Compose Japanese text with
  the Microsoft IME in the terminal.
- **Expected:** the cursor sits where the text ends after moves and resizes. The IME composition shows
  at the cursor and commits once, without a duplicate. Known ConPTY limits are listed in herdr's
  Windows notes (`docs/next/website/src/content/docs/windows-beta.mdx` in `herdrdev/herdr`). Record
  each difference from them.
- **Source:** #416 (open check, PR #422).

## Windows: `.msi`

The core flows run again on the `.msi` package. The open M10 checks are covered in the `-setup.exe`
section.

### W-M-1 Install

- **Steps:** run `poimen_<version>_x64_en-US.msi` (or `msiexec /i <file>.msi /l*v install.log`).
  Start poimen.
- **Expected:** the install finishes without error. poimen starts with the backend and server
  version in the status bar. The log has no error entries.
- **Source:** #417, #416.

### W-M-2 Server starts without a console window

- **Steps:** as W-S-2.
- **Expected:** as W-S-2.
- **Source:** #416, #417.

### W-M-3 Server survives closing the app

- **Steps:** as W-S-3.
- **Expected:** as W-S-3.
- **Source:** #416, #417.

### W-M-4 Hooks offer and install

- **Steps:** as W-S-4.
- **Expected:** as W-S-4.
- **Source:** #173, #416, #427.

### W-M-5 Agent status and start with a prompt

- **Steps:** as W-S-5 and W-S-6, one agent each.
- **Expected:** as W-S-5 and W-S-6.
- **Source:** #154, #156, #417.

### W-M-6 Task start and cleanup

- **Steps:** as W-S-7.
- **Expected:** as W-S-7.
- **Source:** #191, #192.

### W-M-7 Update to the next version

- **Steps:** as W-S-8, with the next `.msi` (or with the in-app update if it offers the `.msi`).
  Record which package the update installed.
- **Expected:** as W-S-8. The terminals survive.
- **Source:** #46, `docs/updater.md`.

## Linux: `.deb`

### L-D-1 Install

- **Steps:** run `sudo apt install ./poimen_<version>_amd64.deb`. Run `dpkg -L poimen` and check that
  `poimen-server` is listed. Start poimen from the menu.
- **Expected:** the install finishes. `poimen-server` is installed next to poimen. The status bar shows
  the backend and server version.
- **Source:** #46 (packaging and updater).

### L-D-2 Server starts detached

- **Steps:** start poimen. Run `pgrep -fl poimen-server`.
- **Expected:** one `poimen-server` runs, not a child of poimen (`ps -o ppid= -p <pid>` is not the
  poimen PID).
- **Source:** #153.

### L-D-3 Server survives closing the app

- **Steps:** open a terminal in a space and type a marker line. Quit poimen. Run `pgrep -fl
  poimen-server`. Start poimen again.
- **Expected:** the same server PID runs. The terminal is intact.
- **Source:** #153.

### L-D-4 Hooks offer and install

- **Steps:** as M-4. Check `~/.claude/settings.json` and `~/.copilot/hooks`.
- **Expected:** as M-4.
- **Source:** #173, #427.

### L-D-5 Agent status idle, working, blocked, done

- **Steps:** as M-5.
- **Expected:** as M-5.
- **Source:** #154, #155, #286.

### L-D-6 Start an agent with a prompt

- **Steps:** as M-6.
- **Expected:** as M-6.
- **Source:** #156.

### L-D-7 Task start and cleanup

- **Steps:** as M-7.
- **Expected:** as M-7.
- **Source:** #191, #192.

### L-D-8 Update to the next version

- **Steps:** install the next `.deb` (or use the in-app update, if it offers the package), restart
  poimen.
- **Expected:** poimen runs the new version. The server that still runs keeps the terminals. The hooks
  report status again after the server restarts.
- **Source:** #46, `docs/updater.md`.

## Linux: `.rpm`

### L-R-1 Install

- **Steps:** run `sudo dnf install ./poimen-<version>.x86_64.rpm` (or `sudo rpm -i`). Run
  `rpm -ql poimen` and check that `poimen-server` is listed. Start poimen.
- **Expected:** the install finishes. `poimen-server` is installed next to poimen. The status bar shows
  the backend and server version.
- **Source:** #46 (packaging and updater).

### L-R-2 Server starts detached

- **Steps:** as L-D-2.
- **Expected:** as L-D-2.
- **Source:** #153.

### L-R-3 Server survives closing the app

- **Steps:** as L-D-3.
- **Expected:** as L-D-3.
- **Source:** #153.

### L-R-4 Hooks offer and install

- **Steps:** as M-4.
- **Expected:** as M-4.
- **Source:** #173, #427.

### L-R-5 Agent status idle, working, blocked, done

- **Steps:** as M-5.
- **Expected:** as M-5.
- **Source:** #154, #155, #286.

### L-R-6 Start an agent with a prompt

- **Steps:** as M-6.
- **Expected:** as M-6.
- **Source:** #156.

### L-R-7 Task start and cleanup

- **Steps:** as M-7.
- **Expected:** as M-7.
- **Source:** #191, #192.

### L-R-8 Update to the next version

- **Steps:** as L-D-8, with the next `.rpm`.
- **Expected:** as L-D-8.
- **Source:** #46, `docs/updater.md`.

## Linux: `.AppImage`

The AppImage runs its program from a temporary FUSE mount (`/tmp/.mount_poimen…`) that is gone after
the app exits. The server is therefore staged into a stable per-version copy (#423, PR #424). The
checks A-7 to A-9 verify that staging.

### A-1 Install (run the file)

- **Steps:** make `poimen_<version>_amd64.AppImage` executable and run it. Quit it.
- **Expected:** poimen starts from the file without FUSE errors (or the error text names the missing
  FUSE package, which is recorded as a gap for the install instructions). The status bar shows the
  backend and server version.
- **Source:** #423.

### A-2 Server starts detached

- **Steps:** start poimen. Run `ps -o args= -C poimen-server`.
- **Expected:** one `poimen-server` runs from `$XDG_DATA_HOME/poimen-server/bin/<version>/poimen-server`
  (default `~/.local/share/...`), not from `/tmp/.mount_…`.
- **Source:** #423, #153.

### A-3 Server survives closing the app

- **Steps:** open a terminal in a space and type a marker line. Quit poimen. Run
  `ls -d /tmp/.mount_poimen*` and `pgrep -fl poimen-server`. Start the AppImage again.
- **Expected:** the mount is gone after quit, the server with the same PID still runs, and the terminal
  is intact.
- **Source:** #153, #423.

### A-4 Hooks offer and install

- **Steps:** with no poimen hooks, start the AppImage and accept the offer. Check
  `~/.claude/settings.json` and `~/.copilot/hooks`.
- **Expected:** the hook commands point at the stable path (the staged copy or its stable link), not at
  `/tmp/.mount_…`. The offer does not return after install.
- **Source:** #173, #423, #429.

### A-5 Agent status idle, working, blocked, done

- **Steps:** as M-5.
- **Expected:** as M-5.
- **Source:** #154, #155, #286.

### A-6 Start an agent with a prompt

- **Steps:** as M-6.
- **Expected:** as M-6.
- **Source:** #156.

### A-7 AppImage passes `APPIMAGE` and `APPDIR`

- **Steps:** start the AppImage. Find the PID of the poimen process (`pgrep -f poimen` for the
  process started from the AppImage, not the server) and run
  `tr '\0' '\n' < /proc/<pid>/environ | grep -E '^(APPIMAGE|APPDIR)='`.
- **Expected:** both variables are set. `APPDIR` is the mount path, `APPIMAGE` is the file path. The
  staging code reads them to decide to copy the server.
- **Source:** #423 (open check, PR #424).

### A-8 Staged server outlives the mount

- **Steps:** as A-3, and then check the staged file with `ls -l` on the path from A-2.
- **Expected:** the staged file exists after the mount is gone, and the server that runs from it keeps
  serving the terminals. Record the path.
- **Source:** #423 (open check, PR #424).

### A-9 Hooks after a restart

- **Steps:** quit the AppImage, start it again (the mount path changes). Run a prompt in Claude Code and
  in Copilot CLI and watch the status.
- **Expected:** the hook commands still point at the stable path. The status updates without a new
  hook install.
- **Source:** #423 (open check, PR #424), #429.

### A-10 Hooks and server after an update

- **Steps:** keep the previous AppImage with a running agent. Run the next AppImage version (a second
  file) and quit the first. Run a prompt in Claude Code.
- **Expected:** the new version stages its own copy under its version directory, and the hooks follow
  the stable link so the status works. The old copy is removed only after its server stops. Record any
  ENOENT error, which is a known gap of PR #424 when two versions run at once.
- **Source:** #423 (open check, PR #424), #46, `docs/updater.md`.

### A-11 Task start and cleanup

- **Steps:** as M-7.
- **Expected:** as M-7.
- **Source:** #191, #192.

### A-12 Update to the next version

- **Steps:** as M-8 with the next AppImage.
- **Expected:** as M-8. The hooks still work (A-9).
- **Source:** #46, #423.
