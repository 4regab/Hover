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
mkdir -p "$SANDBOX/repo" "$SANDBOX/tmp" "$SHORT_TMP" "$SANDBOX/data" "$SANDBOX/cli" "$SANDBOX/nuget" "$SANDBOX/fake-bin"
# srt refuses writes to any .git/config, so under it the copy has no .git of its own
# and git reads the checkout's, read-only.
if [[ "$IN_SRT" == 1 ]]; then COPY_GIT=(--exclude .git); REPO_GIT="$ROOT/.git"; else COPY_GIT=(); REPO_GIT="$SANDBOX/repo/.git"; fi
rsync -a --exclude node_modules --exclude bin --exclude obj --exclude dist --exclude publish --exclude .sandbox ${COPY_GIT[@]+"${COPY_GIT[@]}"} "$ROOT/" "$SANDBOX/repo/"
# Under srt MSBuild can't reach the sockets its worker nodes use (/tmp/MSBuild<pid>),
# so it builds in one process (build-macos.sh reads HOVER_DOTNET_ARGS too).
if [[ "$IN_SRT" == 1 ]]; then export HOVER_DOTNET_ARGS="-m:1"; else export HOVER_DOTNET_ARGS=""; fi
DOTNET="${HOVER_DOTNET:-${DOTNET_ROOT:-$HOME/.dotnet}/dotnet}"
[[ -x "$DOTNET" ]] || DOTNET="$(command -v dotnet)"
SDK_DIR="$(/usr/bin/python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve().parent)' "$DOTNET")"
# Copy the SDK too, so sandboxed test hosts need no access to personal directories.
cp -cR "$SDK_DIR" "$SANDBOX/dotnet"
DOTNET="$SANDBOX/dotnet/dotnet"
SDK_DIR="$SANDBOX/dotnet"
export DOTNET_ROOT="$SDK_DIR"
export HOVER_SANDBOX_ROOT="$SANDBOX" HOVER_DATA_DIR="$SANDBOX/data" TMPDIR="$SHORT_TMP"
export DOTNET_CLI_HOME="$SANDBOX/cli" NUGET_PACKAGES="$SANDBOX/nuget" DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 DOTNET_GENERATE_ASPNET_CERTIFICATE=false
export DOTNET_EnableDiagnostics=0 DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE=true
export npm_config_cache="$SANDBOX/npm-cache" CFFIXED_USER_HOME="$SANDBOX/cli"
# Hover itself must not wrap its agents in a second sandbox inside this one.
export HOVER_SANDBOXED=1
# Restore/build may download packages, but do not execute tests or launch agents.
"$DOTNET" build "$SANDBOX/repo/tests/Hover.Portable.Tests/Hover.Portable.Tests.csproj" -c Release --nologo $HOVER_DOTNET_ARGS
export HOVER_SETTINGS_TESTS=1
export HOVER_APP_OUTPUT="$SANDBOX/build/Hover.app" HOVER_DOTNET="$DOTNET"
# Build helper takes its source root from git; use the copied checkout's git context.
GIT_DIR="$REPO_GIT" GIT_WORK_TREE="$SANDBOX/repo" "$SANDBOX/repo/scripts/build-macos.sh"
cat > "$SANDBOX/test.sb" <<PROFILE
(version 1)
(allow default)
(deny file-write*)
(allow file-write* (subpath "$SANDBOX") (literal "/dev/null") (literal "/dev/tty"))
(deny file-read* (subpath "/Users") (subpath "/Library/Keychains"))
(allow file-read* (subpath "$SDK_DIR"))
(deny network*)
(allow network* (local ip "localhost:*") (remote ip "localhost:*"))
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
sandbox_run "$DOTNET" test "$SANDBOX/repo/tests/Hover.Portable.Tests/Hover.Portable.Tests.csproj" -c Release --no-build --no-restore --logger "trx;LogFileName=portable.trx" --results-directory "$SANDBOX/results" --nologo
export PATH="$SANDBOX/fake-bin:/usr/bin:/bin"
sandbox_run /usr/bin/python3 "$SANDBOX/repo/tests/macos/backend-smoke.py" "$HOVER_APP_OUTPUT" "$SANDBOX"
if [[ "$IN_SRT" == 1 ]]; then
  # The app smokes draw Hover's notch and office on the user's screen; srt has no
  # window server to give them, and they'd be in the user's way if it had.
  echo 'App smokes skipped in srt (they need the screen). Build, portable tests and the packaged backend passed.'
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
