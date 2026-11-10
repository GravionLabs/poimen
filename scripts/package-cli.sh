#!/usr/bin/env bash
# Packages the poimen CLI for use without the app (#519): <out>/poimen-<version>-<target>.tar.gz (.zip for Windows
# targets) holding poimen[.exe]. Usage: scripts/package-cli.sh <version> <target> <binary> <out-dir>
set -euo pipefail

if [ $# -ne 4 ]; then
  echo "usage: package-cli.sh <version> <target> <binary> <out-dir>" >&2
  exit 2
fi
version=$1 target=$2 binary=$3 out=$4
[ -f "$binary" ] || { echo "no such binary: $binary" >&2; exit 1; }

name="poimen-$version-$target"
mkdir -p "$out"
out=$(cd "$out" && pwd)
bin_dir=$(cd "$(dirname "$binary")" && pwd)
bin_name=$(basename "$binary")
case "$target" in
  *windows*) (cd "$bin_dir" && 7z a -tzip "$out/$name.zip" "$bin_name" > /dev/null) ; echo "$out/$name.zip" ;;
  *) tar -C "$bin_dir" -czf "$out/$name.tar.gz" "$bin_name"; echo "$out/$name.tar.gz" ;;
esac
