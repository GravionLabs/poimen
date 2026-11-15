#!/usr/bin/env bash
# macOS code signing and notarization helpers for release.yml (#45).
#
#   macos-signing.sh plan           reads the APPLE_* environment, prints `sign=<true|false>` and
#                                   `notarize=<apple-id|api-key|false>` (one per line, GITHUB_OUTPUT format) and a log
#                                   line on stderr. Without a complete signing set the build is unsigned; with
#                                   POIMEN_REQUIRE_SIGNING=true (stable releases) that is an error instead.
#   macos-signing.sh verify <app> [notarized]
#                                   codesign --verify (app and every executable in Contents/MacOS, hardened runtime),
#                                   spctl --assess (Gatekeeper) and, when notarized, xcrun stapler validate.
#
# Signing needs APPLE_CERTIFICATE (base64 .p12), APPLE_CERTIFICATE_PASSWORD and APPLE_SIGNING_IDENTITY.
# Notarization needs APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID, or APPLE_API_ISSUER + APPLE_API_KEY + a key file
# (APPLE_API_KEY_PATH). See docs/releasing.md.
set -euo pipefail

plan() {
  local sign=false notarize=false reason=""
  if [[ -n "${APPLE_CERTIFICATE:-}" && -n "${APPLE_CERTIFICATE_PASSWORD:-}" && -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    sign=true
    if [[ -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
      notarize=api-key
    elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
      notarize=apple-id
    else
      reason="signing secrets are set but no notarization credentials (APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID or APPLE_API_ISSUER, APPLE_API_KEY, APPLE_API_KEY_PATH)"
    fi
  else
    reason="no signing secrets (APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY)"
  fi
  if [[ -n "$reason" && "${POIMEN_REQUIRE_SIGNING:-}" == true ]]; then
    echo "::error::macOS signing is required for this release but $reason" >&2
    return 1
  fi
  if [[ "$sign" == false ]]; then
    echo "::warning::macOS bundle will be UNSIGNED: $reason" >&2
  elif [[ "$notarize" == false ]]; then
    echo "::warning::macOS bundle will be signed but NOT notarized: $reason" >&2
  else
    echo "macOS bundle will be signed and notarized ($notarize)" >&2
  fi
  echo "sign=$sign"
  echo "notarize=$notarize"
}

verify() {
  local app=${1:?usage: verify <app> [notarized]} notarized=${2:-false} bin
  [[ -d "$app" ]] || { echo "not an app bundle: $app" >&2; return 1; }
  codesign --verify --deep --strict --verbose=2 "$app"
  # Tauri signs the sidecars (poimen-server, poimen-pty-host, poimen) with the app; each needs the hardened runtime
  # for notarization.
  for bin in "$app"/Contents/MacOS/*; do
    [[ -f "$bin" ]] || continue
    codesign --verify --strict "$bin"
    codesign -dv "$bin" 2>&1 | grep -q 'flags=.*runtime' || { echo "no hardened runtime: $bin" >&2; return 1; }
  done
  spctl --assess --type execute --verbose=2 "$app"
  if [[ "$notarized" == true || "$notarized" == apple-id || "$notarized" == api-key ]]; then
    xcrun stapler validate "$app"
  else
    echo "not notarized: skipping stapler validate"
  fi
}

case "${1:-}" in
  plan) plan ;;
  verify) shift; verify "$@" ;;
  *) echo "usage: macos-signing.sh plan | verify <app> [notarized]" >&2; exit 2 ;;
esac
