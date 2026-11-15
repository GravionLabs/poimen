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

## Windows: Authenticode (#672)

The NSIS installer (`-setup.exe`) and every shipped executable (`poimen-app.exe`, `poimen-server`, `poimen-pty-host`,
`poimen`, and the `-rc` variants; `poimen.exe` of the standalone CLI zip is signed and verified in the `cli` job's Windows leg) are signed with Azure Trusted Signing by `release.yml`. The sidecars are signed first
(`scripts/windows-signing.sh sign`), then Tauri signs the app and the installer through
`bundle.windows.signCommand` (a config overlay written at build time, so no account name is committed). After the build
`scripts/windows-signing.sh verify` (sidecars) and `verify-installer` (the installer is opened with 7z and every
executable inside is checked, so `poimen-app.exe` and the rc's `poimen-rc.exe` are covered alike) run
`Get-AuthenticodeSignature` and fail the job unless every status is `Valid`. Order: the release or pre-release is
created and set to draft at once, the installers are attached to the draft, and the final `publish` job makes it public
only after every installer, server archive, CLI package and the checksums succeeded. An unsigned build therefore fails
the job and the release stays a draft; nothing unsigned is ever downloadable. The MSI (stable only) is neither signed nor verified: `windows-signing.sh` cannot verify `bundle/msi/*.msi` yet, so this is a documented gap until a follow-up.

### Recovery after a failed signing run

Use "Re-run failed jobs" on the same run. The release id output is kept, the secrets are read again, and `publish` runs
after the repaired jobs. Do not delete the draft and dispatch again: for a stable release the tag exists and there is no
dispatch on `main`. A left-over draft blocks nothing, it is only visible to maintainers.

### What the owner does by hand

1. Create an Azure Trusted Signing account and a certificate profile (identity validation of the publisher is part of
   it and takes days), in a region with its endpoint (e.g. `https://weu.codesigning.azure.net`).
2. Create an app registration (service principal) and give it the role `Trusted Signing Certificate Profile Signer`
   on the account.
3. Create the GitHub environment `release` (Settings, Environments) leave it without branch restriction and
   without required reviewers. Restricting it to `main` and `release/**` makes beta dispatches from other branches fail
   at job start, and required reviewers would demand an approval for each leg that uses it (macOS, Linux and Windows of
   the installers job, plus the Windows leg of the `cli` job).
4. Add these as secrets of that environment, not as repository secrets:

| Secret | Content |
| --- | --- |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` | the service principal |
| `AZURE_TRUSTED_SIGNING_ENDPOINT` | the regional endpoint URL |
| `AZURE_TRUSTED_SIGNING_ACCOUNT` | the account name |
| `AZURE_TRUSTED_SIGNING_PROFILE` | the certificate profile name |

Pull requests never use `release.yml`, and the environment secrets are not visible to other workflows.

### Behavior

- All settings present: everything is signed and verified.
- A stable release or an rc with a missing setting: the job fails in the first Windows step, before the installer is
  built or uploaded.
- A beta (workflow artifacts only) with a missing setting: unsigned, with a `::warning::`.
- The signing tool is `trusted-signing-cli` (installed with `cargo install`, version pinned in the workflow).

### Manual check

On Windows: `Get-AuthenticodeSignature .\poimen-<version>-setup.exe` and the installed `poimen-app.exe`,
`poimen-server.exe`, `poimen-pty-host.exe` show `Valid`; SmartScreen names the verified publisher.
