#!/usr/bin/env bash
# Writes SHA256SUMS over the release binaries in <dir> (the downloaded assets of a GitHub release) and checks it.
#
# Every regular file counts except the updater signatures (*.sig), the updater manifest (latest.json) and an earlier
# SHA256SUMS. The names are the bare file names, so `sha256sum -c SHA256SUMS` works in a folder with the downloads.
# release.yml attests the same list (actions/attest-build-provenance, subject-checksums). Fails when <dir> holds no
# binary. Tested by scripts/release-checksums.test.sh.
set -euo pipefail

dir=${1:?usage: release-checksums.sh <dir>}
cd "$dir"

files=()
for f in *; do
  [[ -f $f ]] || continue
  case $f in
    *.sig | latest.json | SHA256SUMS) continue ;;
  esac
  files+=("$f")
done

if ((${#files[@]} == 0)); then
  echo "::error::no release binaries in $dir" >&2
  exit 1
fi

sha256sum "${files[@]}" > SHA256SUMS
sha256sum -c --quiet SHA256SUMS
cat SHA256SUMS
