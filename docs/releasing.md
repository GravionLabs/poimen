# Releasing: code signing

## macOS: signing and notarization

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

Only `release.yml` reads these secrets, and it runs on a push to `main` and on a manual dispatch, never for a
pull request.

### One certificate and one API key for the team

The Developer ID Application certificate belongs to the team, not to an app: the same `.p12` and the same App Store
Connect API key sign and notarize every GravionLabs app. The `.p12`, its password and the `.p8` are kept in the
owner's password manager; the private key of the certificate and the `.p8` cannot be downloaded again.

### Rotation

- Certificate (valid for five years, or after a leak): generate a new private key and CSR (`openssl`, RSA 2048),
  create a new Developer ID Application certificate at developer.apple.com (Account Holder only), combine the `.cer`
  and the key into a password-protected `.p12`, then set `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and, when
  the name changed, `APPLE_SIGNING_IDENTITY` in every repository that signs. Revoke the old certificate only after a
  leak: Gatekeeper can then reject the apps already signed with it.
- API key: create a new team key at App Store Connect (Users and Access, Integrations, Team Keys, role Developer),
  set `APPLE_API_KEY`, `APPLE_API_KEY_CONTENT` and `APPLE_API_ISSUER` (unchanged for the same team), then revoke the
  old key.
- Set a secret from a file without showing it: `base64 -i cert.p12 | gh secret set APPLE_CERTIFICATE` and
  `gh secret set APPLE_API_KEY_CONTENT < AuthKey_<id>.p8`.
- Check with an rc (`gh workflow run release.yml --ref release/next`): the job log says
  `macOS bundle will be signed and notarized (api-key)` and the verify step passes.

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

## Windows

The Windows installer and its executables are not signed (no Authenticode certificate), so SmartScreen warns on the
first start. `SHA256SUMS` and the build provenance attestation cover the Windows files.

## Draft until complete

The release or pre-release is created and set to draft at once, the installers are attached to the draft, and the final
`publish` job makes it public only after every installer, server archive, CLI package and the checksums succeeded. A
macOS build whose signature check failed is therefore never downloadable.

After a failed run use "Re-run failed jobs" on the same run. The release id output is kept, the secrets are read again,
and `publish` runs after the repaired jobs. Do not delete the draft and dispatch again: for a stable release the tag
exists and there is no dispatch on `main`. A left-over draft blocks nothing, it is only visible to maintainers.
