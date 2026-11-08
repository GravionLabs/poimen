# Updates of the desktop application

poimen updates itself from the latest GitHub release. It checks only when the user clicks
**Check for updates** (status bar, next to the running version); it makes no other network call.
If a newer version exists, the status bar shows **Update available**; clicking it shows the release
notes and **Install and restart**, which downloads, verifies and installs the update, then
restarts poimen.

Terminals are not touched: `poimen-server` (and herdr, on the herdr backend) keeps them running, so closing or restarting poimen never closes a terminal or pane. On Windows a running `.exe` cannot be replaced, so poimen never starts the installed `poimen-server.exe` itself: it starts a copy under `%LOCALAPPDATA%\poimen-server\bin\<version>\`, and the installer can replace the installed file while that copy runs. The new version starts its own copy only when no server answers; the old server keeps running (and keeps the terminals) until it is stopped. Copies of other versions are removed when they are not running.

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

`release.yml` dispatched on `release/next` with `prerelease: true` publishes a GitHub **pre-release**
`v<x.y.z>-rc.<n>` with installers (no `.msi` and no `.rpm`) for the RC test beside the release PR ([manual acceptance](manual-acceptance.md)). Its `latest.json` is attached to
that pre-release only.

- Stable installs never see it: the endpoint `releases/latest/download/latest.json` resolves through
  GitHub's "latest release", which is the most recent published release that is neither a pre-release
  nor a draft. A pre-release is never "latest", so the endpoint keeps serving the last stable `latest.json`.
- An installed pre-release updates to the final release: the stable `v<x.y.z>` is a higher SemVer than
  `<x.y.z>-rc.<n>` (a pre-release part sorts below the release), and the endpoint serves the stable
  `latest.json` once the release is published. A pre-release install is not offered a newer pre-release;
  install it by hand.
- This follows from GitHub's documented semantics and is not yet observed live. Outstanding: check once on
  the first pre-release that a stable install reports no update, and that the pre-release install updates to
  the final release.

## Limits

- Linux `.deb` installs are updated through the package manager; the in-app updater replaces
  AppImage, Windows and macOS installs.
- Versions before the first release with an updater key cannot update themselves; they need one
  manual install.
