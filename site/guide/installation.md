# Installation

## Prerequisites

None. The installers bundle `poimen-server`, the terminal server poimen uses by default. poimen starts it detached when nothing answers on its socket, and never stops it, so your terminals and agents keep running when you close the app.

Optional: Claude Code, GitHub Copilot CLI or Gemini CLI. On the first start poimen offers to install their hooks (`poimen integration install`), which report whether an agent works, waits or is done, and an MCP entry so agents can look around in poimen; "Not now" is remembered. The GitHub views need the GitHub CLI (`gh auth login`).

### Coming from herdr

The herdr backend was removed. Your spaces and terminals of herdr are not moved. If you leave herdr, remove its agent hooks so they do not report next to poimen's: run `herdr integration uninstall` for each agent you installed them for. An old `backend`, `herdr.autoStart` or `colorSchema: "herdr"` in `config.json` is ignored.

## Download

Get the installer for your system from the [download page](/download) or from the [latest release](https://github.com/GravionLabs/poimen/releases/latest) on GitHub.

### Verify the download

The installers are not code-signed. Each release carries a `SHA256SUMS` file and a build provenance attestation instead, so you can check that a file is unchanged and was built by poimen's release workflow on GitHub.

Checksum: download `SHA256SUMS` from the same release and compare the line of your file.

- **Windows (PowerShell):** `(Get-FileHash .\poimen_<version>_x64-setup.exe -Algorithm SHA256).Hash`, then compare it with the line in `SHA256SUMS` (the case does not matter).
- **macOS:** `shasum -a 256 -c SHA256SUMS --ignore-missing`
- **Linux:** `sha256sum -c SHA256SUMS --ignore-missing`

Provenance, with the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify poimen_<version>_x64-setup.exe --repo GravionLabs/poimen
```

It succeeds only for a file that the release workflow of `GravionLabs/poimen` built. A verified file can still make Windows SmartScreen or Microsoft Defender warn on the first runs of a new version, because they rate unsigned files by how often they have been seen.

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
- The agent hooks (`poimen integration install claude|copilot`, or the offer on the first start) run through PowerShell: Claude Code entries carry `"shell": "powershell"`, the Copilot file a `powershell` command. Claude Code versions without that field run hooks in Git Bash, where the hook fails harmlessly and no status is reported.
- If poimen says access to `poimen-server` was denied, the server was started elevated (for example by a poimen run as administrator). Quit that server in the Task Manager and start poimen normally.

Known limitations (Windows support is new; a hands-on check of a released build follows in [#417](https://github.com/GravionLabs/poimen/issues/417)):

- Terminals run through ConPTY, which reports intermediate cursor positions while it repaints, so the cursor can flicker or jump during output. Input method (IME) composition, for example for CJK text, is only partly anchored. Older Windows 10 builds drop some keyboard sequences (modified keys such as Shift+Enter).
- `poimen-server.exe` is not code-signed either, so Microsoft Defender may warn about it.
- Only x64 is built; Windows on ARM may run it through emulation, untested.

## Linux (x64)

Pick one:

- **AppImage:** `chmod +x poimen_*_amd64.AppImage && ./poimen_*_amd64.AppImage`. The AppImage's mount moves on every run, so poimen starts a copy of the terminal server from `~/.local/share/poimen-server/bin/<version>/` (`$XDG_DATA_HOME` when set), and the agent hooks call the link `~/.local/share/poimen-server/bin/poimen-server`, which stays valid across restarts and updates.
- **Debian, Ubuntu:** `sudo apt install ./poimen_*_amd64.deb`
- **Fedora, openSUSE:** `sudo rpm -i poimen-*.x86_64.rpm` (or `sudo dnf install ./poimen-*.x86_64.rpm`)

## Console

The `poimen` binary (TUI and CLI, see [Console](/guide/console)) ships with every installer and as its own download:

- **macOS:** inside the app at `/Applications/poimen.app/Contents/MacOS/poimen`. Link it onto the PATH with `sudo ln -sf /Applications/poimen.app/Contents/MacOS/poimen /usr/local/bin/poimen`.
- **Windows:** the `-setup.exe` and the `.msi` install `poimen.exe` next to the app and add that directory to your user PATH (open a new terminal afterwards).
- **Linux:** the `.deb` and `.rpm` of the app install `/usr/bin/poimen`. For a host without the app, install the `poimen-cli` package (`.deb` or `.rpm` of the release; it conflicts with the app package). It holds the console and the server: `/usr/bin/poimen`, `/usr/bin/poimen-server` and `/usr/bin/poimen-pty-host`. The package starts no server, see [Console setup](/guide/console#the-server-must-run).
- **Without the app:** the release assets `poimen-<version>-<target>.tar.gz` (Linux, macOS) and `.zip` (Windows) hold only the binary.

Run `poimen doctor` to check the setup.

## Notifications on your phone

poimen-server can run a command or post to a URL when an agent is **blocked** or **done**. It runs in the server, so it works with the app closed. Put it in `~/.config/poimen/config.json` (applied without a restart):

```json
"notify": {
  "timeoutSeconds": 10,
  "blocked": { "command": ["curl", "-sS", "-H", "Title: {agent} is blocked in {space}", "-d", "{text}", "https://ntfy.sh/my-secret-topic"] },
  "done":    { "command": ["curl", "-sS", "-H", "Title: {agent} is done in {space}", "-d", "{text}", "https://ntfy.sh/my-secret-topic"] }
}
```

- `command` is an argv (no shell). `{agent}`, `{space}`, `{status}`, `{text}` and `{machine}` are replaced inside each element. `url` instead gets a JSON POST of the same five fields (`agent`, `space`, `status`, `text`, `machine`); both can be set. The call is made once per agent and transition, ends after `timeoutSeconds` (default 10), and a failure is only a line in `server.log`. `url` needs `curl` on the `PATH` of the server.
- **ntfy:** the example above; subscribe to the topic in the ntfy app. A self-hosted server works the same with its own URL.
- **Pushover:** `["curl", "-sS", "--form-string", "token=<app token>", "--form-string", "user=<user key>", "--form-string", "title={agent} {status} in {space}", "--form-string", "message={text}", "https://api.pushover.net/1/messages.json"]`
- **Telegram:** `["curl", "-sS", "--data-urlencode", "chat_id=<chat id>", "--data-urlencode", "text={agent} {status} in {space}: {text}", "https://api.telegram.org/bot<bot token>/sendMessage"]`

The tokens are in `config.json` in plain text (the file is yours alone; do not export it with them). **Settings > Backend > Notifications** shows what is set and has a **Test** button per status that runs it once with a sample message.

## Build from source

Needs Rust 1.99.1, Node.js 26 and pnpm. See "Development" in the [README](https://github.com/GravionLabs/poimen#development).

## First start

- The status bar shows "poimen connected" and the sidebar your spaces. If it shows "unreachable", press **Retry**; the message names the reason (for example the server did not answer in 10 s, or it speaks another protocol).
- Settings are optional: `~/.config/poimen/config.json`, applied without a restart. See [Settings](/guide/settings).
