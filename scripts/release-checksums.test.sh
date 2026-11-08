#!/usr/bin/env bash
# Tests scripts/release-checksums.sh: run `bash scripts/release-checksums.test.sh` (release.yml runs it before it uses the script).
# The checks pass their values as positional arguments to bash -c, so the single quotes are intended.
# shellcheck disable=SC2016
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/release-checksums.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failures=0

check() { # <name> <condition...>
  local name=$1
  shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; failures=$((failures + 1)); fi
}

# A release folder like poimen's: installers, update archives, their signatures and latest.json.
rel="$tmp/release"
mkdir -p "$rel/subdir"
for f in poimen_1.0.0_x64-setup.exe poimen_1.0.0_x64_en-US.msi poimen_1.0.0_aarch64.dmg \
  poimen_1.0.0_amd64.deb "poimen with space.AppImage" poimen_aarch64.app.tar.gz; do
  echo "content of $f" > "$rel/$f"
done
echo sig > "$rel/poimen_1.0.0_x64-setup.exe.sig"
echo '{}' > "$rel/latest.json"
echo stale > "$rel/SHA256SUMS"
echo nested > "$rel/subdir/ignored.exe"

out=$(bash "$script" "$rel")
sums="$rel/SHA256SUMS"
names=$(sed -E 's/^[0-9a-f]{64}  //' "$sums" | sort)

check "lists the six binaries" test "$(wc -l < "$sums" | tr -d ' ')" = 6
check "includes the setup.exe" grep -qx 'poimen_1.0.0_x64-setup.exe' <<< "$names"
check "includes a name with a space" grep -qx 'poimen with space.AppImage' <<< "$names"
check "skips the signatures" bash -c '! grep -q "\.sig$" <<< "$1"' _ "$names"
check "skips latest.json and itself" bash -c '! grep -qE "^(latest\.json|SHA256SUMS)$" <<< "$1"' _ "$names"
check "skips subdirectories" bash -c '! grep -q ignored <<< "$1"' _ "$names"
check "verifies with sha256sum -c" bash -c 'cd "$1" && sha256sum -c --quiet SHA256SUMS' _ "$rel"
check "prints the sums" test "$out" = "$(cat "$sums")"

empty="$tmp/empty"
mkdir -p "$empty"
echo '{}' > "$empty/latest.json"
check "fails without binaries" bash -c '! bash "$1" "$2" 2> /dev/null' _ "$script" "$empty"

((failures == 0)) || { echo "$failures failed"; exit 1; }
echo "all passed"
