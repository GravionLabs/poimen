#!/usr/bin/env bash
# Builds poimen-server and the poimen CLI (poimen-console, #519) and copies them to
# src-tauri/binaries/poimen-server-<target-triple>[.exe] and poimen-<target-triple>[.exe], where Tauri's
# bundle.externalBin (src-tauri/tauri.bundle.conf.json) expects it. Release builds only: the file is not needed by
# `cargo test`, `clippy` or `pnpm tauri dev`, because externalBin lives in the release-only bundle config.
# With POIMEN_CHANNEL=rc (pre-release builds of release.yml) the server is built for the rc channel and copied to
# poimen-server-rc-<target-triple>[.exe] and poimen-cli-rc-<target-triple>[.exe], the externalBin of src-tauri/tauri.rc.conf.json.
# Usage: scripts/prepare-sidecar.sh [target-triple]   (default: the host triple)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/src-tauri"

target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
ext=""
case "$target" in *windows*) ext=".exe" ;; esac

case "${POIMEN_CHANNEL:-stable}" in
  "" | stable) name="poimen-server"; cli="poimen" ;;
  rc) name="poimen-server-rc"; cli="poimen-cli-rc" ;;
  *)
    echo "POIMEN_CHANNEL must be stable or rc, not '$POIMEN_CHANNEL'" >&2
    exit 2
    ;;
esac

cargo build --release --locked -p poimen-server -p poimen-console --target "$target"
mkdir -p binaries
cp "target/$target/release/poimen-server$ext" "binaries/$name-$target$ext"
cp "target/$target/release/poimen$ext" "binaries/$cli-$target$ext"
echo "sidecar: src-tauri/binaries/$name-$target$ext"
echo "sidecar: src-tauri/binaries/$cli-$target$ext"
