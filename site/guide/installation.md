# Installation

## Prerequisite: herdr

poimen is a window onto [herdr](https://herdr.dev); it does not work without it.

1. Install herdr 0.9.5 or newer, following the instructions on [herdr.dev](https://herdr.dev).
2. Make sure `herdr` is on the `PATH` of your **login shell** (check with `herdr --version` in a new terminal). A macOS app does not see your shell's `PATH`, so poimen reads it from your login shell and also looks in Homebrew, `~/.local/bin` and `~/.cargo/bin`. `HERDR_BIN_PATH` overrides the lookup.
3. You do not have to start the server. poimen starts a detached `herdr server` when nothing listens on the socket, and never restarts or stops one that answers. The setting `herdr.autoStart` turns this off.

Optional: Claude Code or GitHub Copilot CLI for the subagent and Flow views, and the herdr plugin `deliver-board` for the Deliver board.

## Download

Get the installer for your system from the [download page](/download) or from the [latest release](https://github.com/GravionLabs/poimen/releases/latest) on GitHub.

## macOS (Apple Silicon)

1. Open the `.dmg` and drag poimen to `Applications`.
2. The app is not signed or notarized, so Gatekeeper refuses the first start ("poimen is damaged" or "cannot be opened because the developer cannot be verified"). Remove the quarantine flag once:

   ```sh
   xattr -dr com.apple.quarantine /Applications/poimen.app
   ```

   Alternatively, try to open the app once, then use System Settings > Privacy & Security > "Open Anyway".
3. Start poimen. It connects to herdr and lists your spaces.

There is no Intel build; on an Intel Mac, build from source (see the README).

## Windows (x64)

1. Run the `.exe` setup (or the `.msi`).
2. Microsoft Defender SmartScreen shows "Windows protected your PC" because the installer is not signed. Click **More info**, then **Run anyway**.
3. Start poimen from the Start menu.

## Linux (x64)

Pick one:

- **AppImage:** `chmod +x poimen_*_amd64.AppImage && ./poimen_*_amd64.AppImage`
- **Debian, Ubuntu:** `sudo apt install ./poimen_*_amd64.deb`
- **Fedora, openSUSE:** `sudo rpm -i poimen-*.x86_64.rpm` (or `sudo dnf install ./poimen-*.x86_64.rpm`)

## Build from source

Needs Rust 1.99.1, Node.js 26 and pnpm. See "Development" in the [README](https://github.com/GravionLabs/poimen#development).

## First start

- With herdr running, the sidebar shows "herdr connected" and your spaces. If it shows "not reachable", press **Retry**; the message names the reason (herdr not found on the `PATH`, no answer in 10 s, or a protocol older than the supported one).
- Settings are optional: `~/.config/poimen/config.json`, applied without a restart. See [Features](/guide/features#settings).
