# Installation

## Prerequisites

None. The installers bundle `poimen-server`, the terminal server poimen uses by default. poimen starts it detached when nothing answers on its socket, and never stops it, so your terminals and agents keep running when you close the app. You do not need herdr.

Optional: Claude Code or GitHub Copilot CLI. On the first start poimen offers to install their hooks (`poimen-server integration install`), which report whether an agent works, waits or is done; "Not now" is remembered. The same Claude Code and Copilot CLI are used for the subagent and Flow views.

### Alternative: the herdr backend

Set `"backend": "herdr"` in `~/.config/poimen/config.json` to use [herdr](https://herdr.dev) instead (a restart is needed). This backend is removed in a later release.

1. Install herdr 0.9.5 or newer, following the instructions on [herdr.dev](https://herdr.dev).
2. Make sure `herdr` is on the `PATH` of your **login shell** (check with `herdr --version` in a new terminal). A macOS app does not see your shell's `PATH`, so poimen reads it from your login shell and also looks in Homebrew, `~/.local/bin` and `~/.cargo/bin`. `HERDR_BIN_PATH` overrides the lookup.
3. You do not have to start the server. poimen starts a detached `herdr server` when nothing listens on the socket, and never restarts or stops one that answers. The setting `herdr.autoStart` turns the automatic start off (for either backend).

## Download

Get the installer for your system from the [download page](/download) or from the [latest release](https://github.com/GravionLabs/poimen/releases/latest) on GitHub.

## macOS (Apple Silicon)

1. Open the `.dmg` and drag poimen to `Applications`.
2. The app is not signed or notarized, so Gatekeeper refuses the first start ("poimen is damaged" or "cannot be opened because the developer cannot be verified"). Remove the quarantine flag once:

   ```sh
   xattr -dr com.apple.quarantine /Applications/poimen.app
   ```

   Alternatively, try to open the app once, then use System Settings > Privacy & Security > "Open Anyway".
3. Start poimen. It connects to `poimen-server` (started if needed) and lists your spaces.

There is no Intel build; on an Intel Mac, build from source (see the README).

## Windows (x64)

1. Run the `.exe` setup (or the `.msi`).
2. Microsoft Defender SmartScreen shows "Windows protected your PC" because the installer is not signed. Click **More info**, then **Run anyway**.
3. Start poimen from the Start menu, **not as administrator**. poimen starts `poimen-server.exe` in the background (no console window), and it keeps running after you close poimen.

How it runs on Windows:

- The terminal server listens on a named pipe that only your user can open. Its state (`server.json`, the pipe's marker `server.sock`, `server.lock`) is in `%LOCALAPPDATA%\poimen-server\`. poimen starts a copy of the server from `%LOCALAPPDATA%\poimen-server\bin\<version>\`, so an update can replace the installed one while your terminals keep running.
- A new terminal starts PowerShell 7 (`pwsh.exe`) when it is on the `PATH`, else Windows PowerShell, else `%COMSPEC%` (`cmd.exe`).
- The agent hooks (`poimen-server integration install claude|copilot`, or the offer on the first start) run through PowerShell: Claude Code entries carry `"shell": "powershell"`, the Copilot file a `powershell` command. Claude Code versions without that field run hooks in Git Bash, where the hook fails harmlessly and no status is reported.
- If poimen says access to `poimen-server` was denied, the server was started elevated (for example by a poimen run as administrator). Quit that server in the Task Manager and start poimen normally.

Known limitations (Windows support is new; a hands-on check of a released build follows in [#417](https://github.com/GravionLabs/poimen/issues/417)):

- Terminals run through ConPTY, which reports intermediate cursor positions while it repaints, so the cursor can flicker or jump during output. Input method (IME) composition, for example for CJK text, is only partly anchored. Older Windows 10 builds drop some keyboard sequences (modified keys such as Shift+Enter).
- `poimen-server.exe` is not code-signed either, so Microsoft Defender may warn about it.
- Only x64 is built; Windows on ARM may run it through emulation, untested.

## Linux (x64)

Pick one:

- **AppImage:** `chmod +x poimen_*_amd64.AppImage && ./poimen_*_amd64.AppImage`
- **Debian, Ubuntu:** `sudo apt install ./poimen_*_amd64.deb`
- **Fedora, openSUSE:** `sudo rpm -i poimen-*.x86_64.rpm` (or `sudo dnf install ./poimen-*.x86_64.rpm`)

## Build from source

Needs Rust 1.99.1, Node.js 26 and pnpm. See "Development" in the [README](https://github.com/GravionLabs/poimen#development).

## First start

- The status bar shows "poimen connected" and the sidebar your spaces. If it shows "unreachable", press **Retry**; the message names the reason (for the herdr backend: herdr not found on the `PATH`, no answer in 10 s, or a protocol older than the supported one).
- Settings are optional: `~/.config/poimen/config.json`, applied without a restart. See [Features](/guide/features#settings).
