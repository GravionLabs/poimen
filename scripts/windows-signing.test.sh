#!/usr/bin/env bash
# Tests scripts/windows-signing.sh: run `bash scripts/windows-signing.test.sh` (needs jq).
# shellcheck disable=SC2016
set -uo pipefail

script="$(cd "$(dirname "$0")" && pwd)/windows-signing.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
check() { # <name> <condition...>
  local name=$1
  shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; failures=$((failures + 1)); fi
}
run() { env -i PATH="$PATH" "$@" 2> /dev/null; }

full=(AZURE_TENANT_ID=t AZURE_CLIENT_ID=c AZURE_CLIENT_SECRET=topsecret AZURE_TRUSTED_SIGNING_ENDPOINT=https://weu.codesigning.azure.net
  AZURE_TRUSTED_SIGNING_ACCOUNT=acc AZURE_TRUSTED_SIGNING_PROFILE=prof)

# A fake for Get-AuthenticodeSignature: files named *.bad are NotSigned, *.tampered HashMismatch, the rest Valid.
fake="$tmp/status.sh"
printf '#!/usr/bin/env bash\ncase "$1" in *.bad|*.bad.exe) echo NotSigned;; *.tampered) echo HashMismatch;; *) echo Valid;; esac\n' > "$fake"
chmod +x "$fake"
touch "$tmp/a.exe" "$tmp/b.exe" "$tmp/c.bad" "$tmp/d.tampered"
verify() { run POIMEN_SIGNATURE_STATUS_CMD="$fake" bash "$script" verify "$@"; }

check "plan: complete settings sign" test "$(run "${full[@]}" bash "$script" plan)" = "sign=true"
check "plan: nothing set, optional: unsigned" test "$(run bash "$script" plan)" = "sign=false"
check "plan: nothing set, required: fails" bash -c '! env -i PATH="$PATH" POIMEN_REQUIRE_SIGNING=true bash "$1" plan > /dev/null 2>&1' _ "$script"
check "plan: one setting missing, required: fails" bash -c '! env -i PATH="$PATH" POIMEN_REQUIRE_SIGNING=true AZURE_TENANT_ID=t bash "$1" plan > /dev/null 2>&1' _ "$script"
check "plan: complete, required: passes" test "$(run POIMEN_REQUIRE_SIGNING=true "${full[@]}" bash "$script" plan)" = "sign=true"
check "plan: empty value counts as missing" test "$(run "${full[@]}" AZURE_CLIENT_SECRET= bash "$script" plan)" = "sign=false"

run "${full[@]}" bash "$script" config "$tmp/sign.json"
check "config: command and arguments" test "$(jq -c .bundle.windows.signCommand "$tmp/sign.json")" = \
  '{"cmd":"trusted-signing-cli","args":["-e","https://weu.codesigning.azure.net","-a","acc","-c","prof","-d","poimen","%1"]}'
check "config: incomplete settings fail" bash -c '! env -i PATH="$PATH" bash "$1" config "$2" > /dev/null 2>&1' _ "$script" "$tmp/x.json"
check "config: no secret in the file" bash -c '! grep -q topsecret "$1"' _ "$tmp/sign.json"

check "verify: valid files pass" verify "$tmp/a.exe" "$tmp/b.exe"
check "verify: unsigned file fails" bash -c '! env -i PATH="$PATH" POIMEN_SIGNATURE_STATUS_CMD="$1" bash "$2" verify "$3" "$4" > /dev/null 2>&1' _ "$fake" "$script" "$tmp/a.exe" "$tmp/c.bad"
check "verify: invalid signature fails" bash -c '! env -i PATH="$PATH" POIMEN_SIGNATURE_STATUS_CMD="$1" bash "$2" verify "$3" > /dev/null 2>&1' _ "$fake" "$script" "$tmp/d.tampered"
check "verify: missing file fails" bash -c '! env -i PATH="$PATH" POIMEN_SIGNATURE_STATUS_CMD="$1" bash "$2" verify /nonexistent.exe > /dev/null 2>&1' _ "$fake" "$script"
check "verify: no files is a usage error" bash -c 'env -i PATH="$PATH" bash "$1" verify > /dev/null 2>&1; test $? = 2' _ "$script"
# A fake 7z: extracts "$layout" (a directory named by the installer's first line) into the -o folder.
fake7z="$tmp/7z.sh"
printf '#!/usr/bin/env bash\nout=; for a in "$@"; do case "$a" in -o*) out=${a#-o};; esac; done\nfile=${@: -1}\ncp -r "$(cat "$file")/." "$out/"\n' > "$fake7z"
chmod +x "$fake7z"
mkdir -p "$tmp/good/\$PLUGINSDIR" "$tmp/rcgood" "$tmp/badin" "$tmp/empty"
touch "$tmp/good/poimen-app.exe" "$tmp/good/poimen-server.exe" "$tmp/good/\$PLUGINSDIR/x.exe" "$tmp/good/uninstall.bad.exe"
touch "$tmp/rcgood/poimen-rc.exe" "$tmp/badin/poimen-rc.exe" "$tmp/badin/poimen-server.bad"
mv "$tmp/badin/poimen-server.bad" "$tmp/badin/poimen-server.bad.exe"
for n in good rcgood badin empty; do echo "$tmp/$n" > "$tmp/$n-setup.exe"; done
vi() { env -i PATH="$PATH" POIMEN_SEVENZIP="$fake7z" POIMEN_SIGNATURE_STATUS_CMD="$fake" bash "$script" verify-installer "$1" > /dev/null 2>&1; }
check "verify-installer: signed contents pass" vi "$tmp/good-setup.exe"
check "verify-installer: rc main binary name passes" vi "$tmp/rcgood-setup.exe"
check "verify-installer: unsigned exe inside fails" bash -c '! env -i PATH="$PATH" POIMEN_SEVENZIP="$1" POIMEN_SIGNATURE_STATUS_CMD="$2" bash "$3" verify-installer "$4" > /dev/null 2>&1' _ "$fake7z" "$fake" "$script" "$tmp/badin-setup.exe"
check "verify-installer: nothing inside fails" bash -c '! env -i PATH="$PATH" POIMEN_SEVENZIP="$1" POIMEN_SIGNATURE_STATUS_CMD="$2" bash "$3" verify-installer "$4" > /dev/null 2>&1' _ "$fake7z" "$fake" "$script" "$tmp/empty-setup.exe"
check "unknown command: exit 2" bash -c 'bash "$1" nope > /dev/null 2>&1; test $? = 2' _ "$script"

((failures == 0)) || { echo "$failures failed"; exit 1; }
echo "all passed"
