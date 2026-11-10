#!/usr/bin/env bash
# Tests scripts/package-cli.sh: run `bash scripts/package-cli.test.sh`.
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/package-cli.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failures=0

check() { # <name> <condition...>
  local name=$1
  shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; failures=$((failures + 1)); fi
}

printf '#!/bin/sh\necho poimen\n' > "$tmp/poimen"
chmod +x "$tmp/poimen"

archive=$(bash "$script" 1.2.3 x86_64-unknown-linux-musl "$tmp/poimen" "$tmp/out")
check "archive name" test "$archive" = "$(cd "$tmp/out" && pwd)/poimen-1.2.3-x86_64-unknown-linux-musl.tar.gz"
check "archive lists only poimen" test "$(tar -tzf "$archive")" = "poimen"
mkdir "$tmp/x"
tar -C "$tmp/x" -xzf "$archive"
check "binary stays executable" test -x "$tmp/x/poimen"

rc_version=$(bash "$script" 0.15.0-rc003 aarch64-apple-darwin "$tmp/poimen" "$tmp/out")
check "rc version in the name" test "$(basename "$rc_version")" = "poimen-0.15.0-rc003-aarch64-apple-darwin.tar.gz"

check "missing binary fails" bash -c '! bash "$0" 1 t "$1/none" "$1/o" 2>/dev/null' "$script" "$tmp"
check "wrong arguments exit 2" bash -c 'bash "$0" 1 2>/dev/null; test $? -eq 2' "$script"

if [ "$failures" -ne 0 ]; then echo "$failures failed"; exit 1; fi
echo "all passed"
