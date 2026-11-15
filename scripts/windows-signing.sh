#!/usr/bin/env bash
# Authenticode signing of the Windows build (#672), run by release.yml on the windows-latest runner (Git Bash).
#   plan                 prints sign=true|false from the environment (Azure Trusted Signing); exits 1 when signing is
#                        required (POIMEN_REQUIRE_SIGNING=true) and the settings are incomplete
#   config <file>        writes the Tauri config overlay with bundle.windows.signCommand (signs the app and the installer)
#   sign <file>...       signs the files with trusted-signing-cli (the sidecars, before Tauri bundles them)
#   verify <file>...     fails unless every file has a Valid Authenticode signature (Get-AuthenticodeSignature)
#   verify-installer <setup.exe>
#                        verifies the installer and every executable inside it (extracted with 7z), whatever the main
#                        binary is called (poimen-app.exe, poimen-rc.exe); fails when none is found
# POIMEN_SEVENZIP replaces 7z in tests. POIMEN_SIGNATURE_STATUS_CMD replaces the PowerShell check in tests: it is called with the path and prints the status.
# Settings (docs/releasing.md): AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET (read by trusted-signing-cli),
# AZURE_TRUSTED_SIGNING_ENDPOINT, AZURE_TRUSTED_SIGNING_ACCOUNT, AZURE_TRUSTED_SIGNING_PROFILE.
set -euo pipefail

complete() {
  local v
  for v in AZURE_TENANT_ID AZURE_CLIENT_ID AZURE_CLIENT_SECRET AZURE_TRUSTED_SIGNING_ENDPOINT \
    AZURE_TRUSTED_SIGNING_ACCOUNT AZURE_TRUSTED_SIGNING_PROFILE; do
    [ -n "${!v:-}" ] || return 1
  done
}

signer() {
  printf '%s\n' -e "$AZURE_TRUSTED_SIGNING_ENDPOINT" -a "$AZURE_TRUSTED_SIGNING_ACCOUNT" \
    -c "$AZURE_TRUSTED_SIGNING_PROFILE" -d poimen
}

status_of() {
  if [ -n "${POIMEN_SIGNATURE_STATUS_CMD:-}" ]; then
    "$POIMEN_SIGNATURE_STATUS_CMD" "$1"
  else
    # The path goes through the environment: in -Command mode trailing arguments are not $args.
    # POIMEN_PS_PREAMBLE lets a test define a stand-in for Get-AuthenticodeSignature.
    # shellcheck disable=SC2016
    P="$1" pwsh -NoProfile -Command "${POIMEN_PS_PREAMBLE:-}"'(Get-AuthenticodeSignature -LiteralPath $env:P).Status' | tr -d '\r\n'
    echo
  fi
}

# shellcheck disable=SC2016
cmd="${1:-}"
[ $# -gt 0 ] && shift
case "$cmd" in
  plan)
    if complete; then
      echo "sign=true"
    elif [ "${POIMEN_REQUIRE_SIGNING:-false}" = true ]; then
      echo "::error::Windows signing settings are incomplete; an unsigned installer is not published (docs/releasing.md)" >&2
      exit 1
    else
      echo "::warning::Windows signing settings are incomplete; this build is unsigned" >&2
      echo "sign=false"
    fi
    ;;
  config)
    [ $# -eq 1 ] || { echo "usage: windows-signing.sh config <file>" >&2; exit 2; }
    complete || { echo "signing settings are incomplete" >&2; exit 1; }
    jq -n --arg e "$AZURE_TRUSTED_SIGNING_ENDPOINT" --arg a "$AZURE_TRUSTED_SIGNING_ACCOUNT" \
      --arg c "$AZURE_TRUSTED_SIGNING_PROFILE" \
      '{bundle: {windows: {signCommand: {cmd: "trusted-signing-cli", args: ["-e", $e, "-a", $a, "-c", $c, "-d", "poimen", "%1"]}}}}' > "$1"
    ;;
  sign)
    complete || { echo "signing settings are incomplete" >&2; exit 1; }
    mapfile -t args < <(signer)
    for f in "$@"; do trusted-signing-cli "${args[@]}" "$f"; done
    ;;
  verify)
    [ $# -gt 0 ] || { echo "usage: windows-signing.sh verify <file>..." >&2; exit 2; }
    bad=0
    for f in "$@"; do
      if [ ! -f "$f" ]; then echo "missing: $f" >&2; bad=1; continue; fi
      s="$(status_of "$f" | tr -d '\r')"
      if [ "$s" = Valid ]; then echo "signed: $f"; else echo "NOT SIGNED ($s): $f" >&2; bad=1; fi
    done
    exit "$bad"
    ;;
  verify-installer)
    [ $# -eq 1 ] || { echo "usage: windows-signing.sh verify-installer <setup.exe>" >&2; exit 2; }
    [ -f "$1" ] || { echo "missing: $1" >&2; exit 1; }
    out="$(mktemp -d)"
    trap 'rm -rf "$out"' EXIT
    "${POIMEN_SEVENZIP:-7z}" x -y "-o$out" "$1" > /dev/null || { echo "cannot extract $1" >&2; exit 1; }
    mapfile -t inside < <(find "$out" -type f -iname '*.exe' ! -ipath '*PLUGINSDIR*' ! -iname 'uninstall*.exe' | sort)
    [ ${#inside[@]} -gt 0 ] || { echo "no executable inside $1" >&2; exit 1; }
    bash "$0" verify "$1" "${inside[@]}"
    ;;
  *)
    echo "usage: windows-signing.sh plan|config|sign|verify|verify-installer" >&2
    exit 2
    ;;
esac
