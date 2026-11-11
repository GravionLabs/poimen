# Updates of the desktop application

poimen updates itself from the latest GitHub release. It checks only when the user clicks
**Check for updates** (status bar, next to the running version); it makes no other network call.
If a newer version exists, the status bar shows **Update available**; clicking it shows the release
notes and **Install and restart**, which downloads, verifies and installs the update, then
restarts poimen.

Terminals are not touched: `poimen-server` (and herdr, on the herdr backend) keeps them running, so closing or restarting poimen never closes a terminal or pane. On Windows a running `.exe` cannot be replaced, so poimen never starts the installed `poimen-server.exe` itself: it starts a copy under `%LOCALAPPDATA%\poimen-server\bin\<version>\`, and the installer can replace the installed file while that copy runs. The new version starts its own copy only when no server answers; the old server keeps running (and keeps the terminals) until it is stopped. Copies of other versions are removed when they are not running.

## Terminals and updates (`poimen-pty-host`)

Each terminal is owned by its own small process, `poimen-pty-host` ([ADR 0009](adr/0009-terminal-ownership-in-a-stable-pty-host-server-updates-without-losing-terminals.md)), which ships next to `poimen-server` in every package (macOS `.dmg`, Windows installers, Linux `.deb`, `.rpm` and AppImage; `poimen-pty-host-rc` on the RC). It runs from the same place as the server:

- Windows and AppImage: from a per-version copy next to the server copy, `%LOCALAPPDATA%\poimen-server\bin\<version>\` or `$XDG_DATA_HOME/poimen-server/bin/<version>/`, so an update can replace the installed file while a host runs.
- macOS and the Linux packages: the installed binary.

Copies of other versions are removed only when neither a server nor a host runs from them, so an update never touches a running host; a directory that is still in use is left whole and checked again at the next start.

**The first update to the version that ships the host ends the existing terminals once**, because the older server owns its terminals itself and ends them when it is restarted (the confirmation names them). From then on an update keeps the terminals: the new server reconnects to the running hosts.

A host is not replaced by an update, so a terminal keeps the host version it started with; a fix to the host applies to terminals started after the update. The host version of a running terminal is the `version` of its `hello` message (host protocol in `docs/protocol.md`); the version of an installed host is `poimen-pty-host --version`, and a host running from a copy shows its version in the directory name of its path (`ps`, Task Manager).

## Errors

The header names the failed step and shows the error text (select it to copy):

- **Update check failed**: the check itself did not work. **Try again** repeats it.
- **Update failed**: the update was found but could not be installed. It stays available, so
  **Install and restart** can be clicked again without a new check.

Common causes: no network connection; the release has no package for this platform; the download
was removed from the release; the signature does not match the public key.

## Signing

Update packages are signed with a Tauri updater key. This is separate from operating system code
signing, which the installers do not have yet (#45).

- The public key is `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`; the endpoint is
  `https://github.com/GravionLabs/poimen/releases/latest/download/latest.json`.
- The private key and its password are the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `release.yml` passes them to `GravionLabs/ci/tauri/installers`,
  which hands them to `tauri-action`; it signs the update packages and uploads `latest.json` with
  the installers.
- `src-tauri/tauri.bundle.conf.json` turns on `createUpdaterArtifacts`. Only `release.yml` passes it
  (`--config`), so local builds and `tauri dev` need no key.

The key pair was replaced on 2026-11-06 because the earlier private key was lost. The new private
key and its password are backed up in the owner's password manager. Installs made with the earlier
key cannot verify updates signed with the new one and must be reinstalled by hand once.

To make a new key: `pnpm tauri signer generate -w <file>`, then replace the public key in
`tauri.conf.json` and both secrets. Apps with the old key cannot verify updates signed with the
new one.

## Pre-releases

`release.yml` dispatched on `release/next` (channel rc, from the branch) publishes a GitHub **pre-release**
`v<x.y.z>-rc<nnn>` with installers (no `.msi` and no `.rpm`) for the RC test beside the release PR ([manual acceptance](manual-acceptance.md)). A dispatch on any other branch builds `<x.y.z>-beta<nnn>` installers as workflow
artifacts: no tag and no release.

Both are built for the build-time channel `rc` (`POIMEN_CHANNEL=rc`, overlay `src-tauri/tauri.rc.conf.json`):
**Poimen RC**, which installs next to the stable app (own identifier `dev.gravionlabs.poimen.rc`, binaries,
server socket, settings and hooks) and does not update itself:

- No updater artifacts and no `latest.json` (`createUpdaterArtifacts: false`), no update endpoint, and the
  updater plugin is not registered (`updater::enabled`); the status bar has no **Check for updates**. A newer
  RC is installed by hand over the older one.
- Stable installs never see a pre-release: the endpoint `releases/latest/download/latest.json` resolves
  through GitHub's "latest release", which is the most recent published release that is neither a
  pre-release nor a draft, and a pre-release has no `latest.json` anyway.
- The update items of the RC test (M-8, W-S-8, A-12) are checked on the stable app, when the release is
  published.
- Shared update channels (an RC that updates to the next RC) are #499.

## Limits

- Linux `.deb` installs are updated through the package manager; the in-app updater replaces
  AppImage, Windows and macOS installs.
- Versions before the first release with an updater key cannot update themselves; they need one
  manual install.
