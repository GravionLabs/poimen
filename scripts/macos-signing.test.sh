#!/usr/bin/env bash
# Tests the plan logic of scripts/macos-signing.sh: run `bash scripts/macos-signing.test.sh`.
# shellcheck disable=SC2016
set -uo pipefail

script="$(cd "$(dirname "$0")" && pwd)/macos-signing.sh"
failures=0
check() { # <name> <condition...>
  local name=$1
  shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; failures=$((failures + 1)); fi
}
plan() { env -i PATH="$PATH" "$@" bash "$script" plan 2> /dev/null; }

cert=(APPLE_CERTIFICATE=x APPLE_CERTIFICATE_PASSWORD=p APPLE_SIGNING_IDENTITY="Developer ID Application: X (T)")
appleid=(APPLE_ID=a@b.c APPLE_PASSWORD=pw APPLE_TEAM_ID=T)
apikey=(APPLE_API_ISSUER=i APPLE_API_KEY=k APPLE_API_KEY_PATH=/tmp/k.p8)

check "no secrets: unsigned" test "$(plan)" = $'sign=false\nnotarize=false'
check "no secrets, required: fails" bash -c '! env -i PATH="$PATH" POIMEN_REQUIRE_SIGNING=true bash "$1" plan > /dev/null 2>&1' _ "$script"
check "certificate and Apple ID: signed and notarized" test "$(plan "${cert[@]}" "${appleid[@]}")" = $'sign=true\nnotarize=apple-id'
check "certificate and API key: signed and notarized" test "$(plan "${cert[@]}" "${apikey[@]}")" = $'sign=true\nnotarize=api-key'
check "API key wins over Apple ID" test "$(plan "${cert[@]}" "${apikey[@]}" "${appleid[@]}")" = $'sign=true\nnotarize=api-key'
check "certificate only: signed, not notarized" test "$(plan "${cert[@]}")" = $'sign=true\nnotarize=false'
check "certificate only, required: fails" bash -c '! env -i PATH="$PATH" POIMEN_REQUIRE_SIGNING=true APPLE_CERTIFICATE=x APPLE_CERTIFICATE_PASSWORD=p APPLE_SIGNING_IDENTITY=i bash "$1" plan > /dev/null 2>&1' _ "$script"
check "complete, required: passes" test "$(plan POIMEN_REQUIRE_SIGNING=true "${cert[@]}" "${appleid[@]}")" = $'sign=true\nnotarize=apple-id'
check "missing password: unsigned" test "$(plan APPLE_CERTIFICATE=x APPLE_SIGNING_IDENTITY=i "${appleid[@]}")" = $'sign=false\nnotarize=false'
check "partial Apple ID: not notarized" test "$(plan "${cert[@]}" APPLE_ID=a APPLE_PASSWORD=p)" = $'sign=true\nnotarize=false'
check "unknown command: exit 2" bash -c 'bash "$1" nope 2> /dev/null; test $? = 2' _ "$script"
check "verify rejects a missing app" bash -c '! bash "$1" verify /nonexistent.app 2> /dev/null' _ "$script"

((failures == 0)) || { echo "$failures failed"; exit 1; }
echo "all passed"
