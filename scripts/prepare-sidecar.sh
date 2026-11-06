#!/usr/bin/env bash
# Builds poimen-server and copies it to src-tauri/binaries/poimen-server-<target-triple>[.exe], where Tauri's
# bundle.externalBin (src-tauri/tauri.bundle.conf.json) expects it. Release builds only: the file is not needed by
# `cargo test`, `clippy` or `pnpm tauri dev`, because externalBin lives in the release-only bundle config.
# Usage: scripts/prepare-sidecar.sh [target-triple]   (default: the host triple)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/src-tauri"

target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
ext=""
case "$target" in *windows*) ext=".exe" ;; esac

cargo build --release --locked -p poimen-server --target "$target"
mkdir -p binaries
cp "target/$target/release/poimen-server$ext" "binaries/poimen-server-$target$ext"
echo "sidecar: src-tauri/binaries/poimen-server-$target$ext"
