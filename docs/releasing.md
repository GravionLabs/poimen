# Releasing: macOS signing and notarization

The macOS installer (`.dmg` and the `.app` inside it, Apple Silicon only) is signed with a Developer ID
Application certificate and notarized by `release.yml` (#45). Tauri signs the app and its sidecars
(`poimen-server`, `poimen-pty-host`, `poimen`, and the `-rc` variants) with the hardened runtime, which notarization
requires; no entitlements file is needed because the binaries use no JIT, unsigned memory or library injection.
This is separate from the updater key (see [updater.md](updater.md)).

## Secrets

Repository secrets (Settings, Secrets and variables, Actions):

| Secret | Content |
| --- | --- |
| `APPLE_CERTIFICATE` | the Developer ID Application certificate with its private key, exported as `.p12`, base64 (`base64 -i cert.p12`) |
| `APPLE_CERTIFICATE_PASSWORD` | the password of that `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |

Notarization, one of:

- Apple ID: `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`.
- App Store Connect API key (preferred for CI): `APPLE_API_ISSUER`, `APPLE_API_KEY` (key id) and
  `APPLE_API_KEY_CONTENT` (the text of the `.p8` file; the workflow writes it to a temp file and sets
  `APPLE_API_KEY_PATH`). The API key wins when both are set.

## Behavior

`scripts/macos-signing.sh plan` (tested by `scripts/macos-signing.test.sh`) decides per run:

- all secrets present: signed and notarized (the ticket is stapled to the app);
- a stable release (push to `main`) without a complete set: the job fails, so no unsigned stable release ships;
- an rc, a beta or a fork build without secrets: unsigned, with a `::warning::` log line; the app then needs the
  right-click Open workaround or `xattr -dr com.apple.quarantine`;
- signing secrets without notarization credentials: signed, not notarized, with a warning (an rc/beta only).

After the build `scripts/macos-signing.sh verify` runs `codesign --verify --deep --strict` on the app and on each
executable in `Contents/MacOS` (hardened runtime flag required), `spctl --assess --type execute` (Gatekeeper) and
`xcrun stapler validate`. The `.dmg` is attached to the GitHub release by the installers step like the other
installers; there is no x64 or universal build.

## Manual check

On a Mac that never ran the build: download the `.dmg` from the release, open it, copy the app, start it. No
Gatekeeper warning about an unidentified developer or an unverified app may appear
(`spctl --assess --type execute -vv /Applications/poimen.app` says `accepted`, `source=Notarized Developer ID`).
