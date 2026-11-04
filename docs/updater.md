# Updates of the desktop application

poimen updates itself from the latest GitHub release. It checks only when the user clicks
**Check for updates** (status bar, next to the running version); it makes no other network call.
If a newer version exists, the status bar shows **Update available**; clicking it shows the release
notes and **Install and restart**, which downloads, verifies and installs the update, then
restarts poimen.

herdr panes are not touched: closing or restarting poimen never closes a pane.

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

**Keep a backup of the private key and its password.** If they are lost, installed apps can never
update again and users must install a new version by hand.

To make a new key: `pnpm tauri signer generate -w <file>`, then replace the public key in
`tauri.conf.json` and both secrets. Apps with the old key cannot verify updates signed with the
new one.

## Limits

- Linux `.deb` installs are updated through the package manager; the in-app updater replaces
  AppImage, Windows and macOS installs.
- Versions before the first release with an updater key cannot update themselves; they need one
  manual install.
