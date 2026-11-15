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
`poimen`, and the `-rc` variants) are signed with Azure Trusted Signing by `release.yml`. The sidecars are signed first
(`scripts/windows-signing.sh sign`), then Tauri signs the app and the installer through
`bundle.windows.signCommand` (a config overlay written at build time, so no account name is committed). The job
finishes with `scripts/windows-signing.sh verify`, which runs `Get-AuthenticodeSignature` on each file and fails unless
the status is `Valid`. The MSI is not built for pre-releases and stays unsigned for the stable one until a follow-up.

### What the owner does by hand

1. Create an Azure Trusted Signing account and a certificate profile (identity validation of the publisher is part of
   it and takes days), in a region with its endpoint (e.g. `https://weu.codesigning.azure.net`).
2. Create an app registration (service principal) and give it the role `Trusted Signing Certificate Profile Signer`
   on the account.
3. Create the GitHub environment `release` (Settings, Environments) and, if wanted, restrict it to `main` and
   `release/**` and add required reviewers.
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
