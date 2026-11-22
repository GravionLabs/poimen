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

# The poimen-cli package (#1177): the nfpm file places the console, the server and the PTY host in /usr/bin, named
# for the channel. Without nfpm only the filled-in file is checked; with it the built .deb is listed.
nfpm_file="$(cd "$(dirname "$0")/.." && pwd)/packaging/nfpm-poimen-cli.yaml"
for channel in stable rc; do
  suffix=""
  if [ "$channel" = rc ]; then suffix="-rc"; fi
  for b in server host; do printf '#!/bin/sh\necho %s\n' "$b" > "$tmp/$b"; chmod +x "$tmp/$b"; done
  filled="$tmp/nfpm-$channel.yaml"
  ARCH=amd64 VERSION=1.2.3 BINARY="$tmp/poimen" SERVER="$tmp/server" PTY_HOST="$tmp/host" \
    SERVER_NAME="poimen-server$suffix" PTY_HOST_NAME="poimen-pty-host$suffix" \
    envsubst '${ARCH} ${VERSION} ${BINARY} ${SERVER} ${PTY_HOST} ${SERVER_NAME} ${PTY_HOST_NAME}' < "$nfpm_file" > "$filled"
  check "$channel package: poimen" grep -q 'dst: /usr/bin/poimen$' "$filled"
  check "$channel package: server" grep -q "dst: /usr/bin/poimen-server$suffix\$" "$filled"
  check "$channel package: pty host" grep -q "dst: /usr/bin/poimen-pty-host$suffix\$" "$filled"
  check "$channel package: no variable left" bash -c '! grep -q "\${" "$0"' "$filled"
  check "$channel package: conflicts with the app" grep -q '^  - poimen$' "$filled"
  if command -v nfpm >/dev/null 2>&1; then
    mkdir -p "$tmp/pkg-$channel"
    nfpm package -f "$filled" -p deb -t "$tmp/pkg-$channel" >/dev/null
    if command -v dpkg-deb >/dev/null 2>&1; then
      listing=$(dpkg-deb -c "$tmp/pkg-$channel"/*.deb)
      check "$channel deb holds the server" grep -q "usr/bin/poimen-server$suffix\$" <<< "$listing"
      check "$channel deb holds the pty host" grep -q "usr/bin/poimen-pty-host$suffix\$" <<< "$listing"
    fi
  fi
done

if [ "$failures" -ne 0 ]; then echo "$failures failed"; exit 1; fi
echo "all passed"
