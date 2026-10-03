#!/bin/bash
# Build in a disposable copy. Every executable test runs under a fail-closed
# macOS sandbox: writes stay here, personal files and external network are denied.
# Run through scripts/sandbox.sh (HOVER_SANDBOXED=1), srt is that sandbox: the copy
# goes in srt's temp folder, nothing nests a second sandbox-exec, and the app smokes,
# which put Hover's windows on the screen, are skipped.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
BUILD_PATH="$PATH"
IN_SRT="${HOVER_SANDBOXED:-0}"
if [[ "$IN_SRT" == 1 ]]; then SANDBOX="$(mktemp -d "${TMPDIR%/}/hover-sandbox.XXXXXX")"; SHORT_TMP="$SANDBOX/t/"
else SANDBOX="$(mktemp -d /private/tmp/hover-sandbox.XXXXXX)"; SHORT_TMP="$SANDBOX/tmp/"; fi
printf 'Sandbox: %s\n' "$SANDBOX"
mkdir -p "$SANDBOX/repo" "$SANDBOX/tmp" "$SHORT_TMP" "$SANDBOX/data" "$SANDBOX/cli" "$SANDBOX/fake-bin"
# Private, as a real $TMPDIR is: Hover puts its single-instance socket only in one of the user's own.
chmod 700 "$SHORT_TMP"
# srt refuses writes to any .git/config, so under it the copy has no .git of its own
# and git reads the checkout's, read-only.
if [[ "$IN_SRT" == 1 ]]; then COPY_GIT=(--exclude .git); REPO_GIT="$ROOT/.git"; else COPY_GIT=(); REPO_GIT="$SANDBOX/repo/.git"; fi
# Anchored: crates/hover-core/src/bin is source. The build output (target/) is made in the copy.
rsync -a --exclude node_modules --exclude /target --exclude /dist --exclude /publish --exclude /.sandbox ${COPY_GIT[@]+"${COPY_GIT[@]}"} "$ROOT/" "$SANDBOX/repo/"
# cargo builds in the copy (so test binaries find their fixtures there), with the user's own
# toolchain and registry cache; only the test binaries run under the sandbox below.
export HOVER_SANDBOX_ROOT="$SANDBOX" HOVER_DATA_DIR="$SANDBOX/data" TMPDIR="$SHORT_TMP" CARGO_TARGET_DIR="$SANDBOX/target"
export npm_config_cache="$SANDBOX/npm-cache" CFFIXED_USER_HOME="$SANDBOX/cli"
# Hover itself must not wrap its agents in a second sandbox inside this one.
export HOVER_SANDBOXED=1
# Restore/build may download packages, but do not execute tests or launch agents. The
# backend and the crates it stands on: their test binaries are built here and listed as
# "<crate folder> <executable>" lines, then run below under the sandbox.
TEST_CRATES=(-p hover-backend -p hover-core -p hover-agents -p hover-quota -p hover-md -p hover-diagram)
cargo test --manifest-path "$SANDBOX/repo/Cargo.toml" "${TEST_CRATES[@]}" --no-run --message-format=json \
  | /usr/bin/python3 -c '
import json, os, sys
for line in sys.stdin:
    try: m = json.loads(line)
    except ValueError: continue
    if m.get("reason") == "compiler-artifact" and m.get("profile", {}).get("test") and m.get("executable"):
        print(os.path.dirname(m["manifest_path"]), m["executable"])
' > "$SANDBOX/test-binaries.txt"
[[ -s "$SANDBOX/test-binaries.txt" ]] || { echo 'cargo listed no test binaries' >&2; exit 1; }
export HOVER_SETTINGS_TESTS=1
export HOVER_APP_OUTPUT="$SANDBOX/build/Hover.app"
# Build helper takes its source root from git; use the copied checkout's git context.
GIT_DIR="$REPO_GIT" GIT_WORK_TREE="$SANDBOX/repo" "$SANDBOX/repo/scripts/build-macos.sh"
cat > "$SANDBOX/test.sb" <<PROFILE
(version 1)
(allow default)
(deny file-write*)
(allow file-write* (subpath "$SANDBOX") (literal "/dev/null") (literal "/dev/tty"))
(deny file-read* (subpath "/Users") (subpath "/Library/Keychains"))
(deny network*)
(allow network* (local ip "localhost:*") (remote ip "localhost:*"))
; Unix sockets only in the test's own folder (single instance, the agents' relay).
(allow network* (local unix-socket (subpath "$SANDBOX")) (remote unix-socket (subpath "$SANDBOX")))
PROFILE
sandbox_run() {
  if [[ "$IN_SRT" == 1 ]]; then (cd "$SANDBOX/repo" && "$@"); return; fi
  /usr/bin/python3 - "$SANDBOX/repo" "$SANDBOX/test.sb" "$@" <<'PYRUN'
import os,sys
os.chdir(sys.argv[1])
os.execv('/usr/bin/sandbox-exec', ['/usr/bin/sandbox-exec','-f',sys.argv[2],*sys.argv[3:]])
PYRUN
}
sandbox_smoke() {
  sandbox_run /usr/bin/python3 -c 'import subprocess,sys; subprocess.run([sys.argv[1], "--smoke-test"], check=True, timeout=60)' "$HOVER_APP_OUTPUT/Contents/MacOS/Hover"
}
# Prove the boundary before running tests. Failure does not fall back to isolation
# by environment variables alone. Under srt the checkout is writable, so the probe
# is outside it.
if [[ "$IN_SRT" == 1 ]]; then CANARY="$HOME/.hover-sandbox-denial-probe"; else CANARY="$ROOT/.hover-sandbox-denial-probe"; fi
if sandbox_run /bin/sh -c 'echo forbidden > "$1"' sh "$CANARY" 2>"$SANDBOX/denial.log"; then
  echo 'Sandbox write boundary failed; tests were not run.' >&2; exit 1
fi
[[ ! -e "$CANARY" ]] || { echo 'Unexpected sandbox probe file' >&2; exit 1; }
# The stand-in gh the agents' tests run, built here: the sandbox below can't reach the
# toolchain (it is under /Users), and the tests find it where they would build it.
mkdir -p "$SHORT_TMP/hover-fakegh-build" "$SHORT_TMP/hover-backend-fakegh-build"
rustc --edition 2021 -C debuginfo=0 "$SANDBOX/repo/crates/hover-agents/tests/fixtures/fakegh.rs" -o "$SHORT_TMP/hover-fakegh-build/fakegh"
cp "$SHORT_TMP/hover-fakegh-build/fakegh" "$SHORT_TMP/hover-backend-fakegh-build/fakegh"
# The backend's protocol tests build their stand-in agent and OpenCode with cargo: done
# here, and the cargo they call (CARGO) only finds them made.
cargo build --manifest-path "$SANDBOX/repo/Cargo.toml" -p hover-measure --bin fake-agent --bin fake-opencode
# fd 3: a test binary must not read the list from stdin.
while read -r CRATE_DIR TEST_BIN <&3; do
  # A home of their own: the user's is unreadable here (and tests have no business in it).
  mkdir -p "$SANDBOX/home"
  HOME="$SANDBOX/home" CARGO=/usr/bin/true sandbox_run /bin/sh -c 'cd "$1" && exec "$2" --test-threads=1' sh "$CRATE_DIR" "$TEST_BIN"
done 3< "$SANDBOX/test-binaries.txt"
export PATH="$SANDBOX/fake-bin:/usr/bin:/bin"
sandbox_run /usr/bin/python3 "$SANDBOX/repo/tests/macos/backend-smoke.py" "$HOVER_APP_OUTPUT" "$SANDBOX"
if [[ "$IN_SRT" == 1 ]]; then
  # The app smokes draw Hover's notch and office on the user's screen; srt has no
  # window server to give them, and they'd be in the user's way if it had.
  echo 'App smokes skipped in srt (they need the screen). Build, the backend tests and the packaged backend passed.'
  printf 'All sandboxed checks passed. Reports: %s\n' "$SANDBOX"
  exit 0
fi
# WKWebView uses system XPC rendering services; smoke mode uses an ephemeral store,
# has no Keychain/login/hotkey/agent actions, and all host writes remain sandboxed.
HOVER_DATA_DIR="$SANDBOX/ui-data" sandbox_smoke >"$SANDBOX/ui.stdout" 2>"$SANDBOX/ui.stderr"
/usr/bin/python3 - "$SANDBOX/smoke-result.json" <<'PY'
import json,sys
result=json.load(open(sys.argv[1]))
assert result['success'], result
print('Native office smoke passed:',result['detail'])
PY
# Ship the production compilation without the interaction harness, then smoke it
# under the same boundary before making that build available.
cp "$SANDBOX/smoke-result.json" "$SANDBOX/settings-smoke-result.json"
export HOVER_SETTINGS_TESTS=0
PATH="$BUILD_PATH" GIT_DIR="$REPO_GIT" GIT_WORK_TREE="$SANDBOX/repo" "$SANDBOX/repo/scripts/build-macos.sh"
HOVER_DATA_DIR="$SANDBOX/production-ui-data" sandbox_smoke >"$SANDBOX/production-ui.stdout" 2>"$SANDBOX/production-ui.stderr"
/usr/bin/python3 - "$SANDBOX/smoke-result.json" <<'PYRESULT'
import json,sys
assert json.load(open(sys.argv[1]))['success']
PYRESULT
printf 'All checks passed. Reports and screenshot: %s\n' "$SANDBOX"
