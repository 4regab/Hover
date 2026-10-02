#!/bin/bash
# Runs a command on this checkout inside Anthropic's sandbox-runtime (srt, Apache-2.0),
# at background priority, so agent work never gets in the way of the person using the
# Mac:
#   - writes only to this checkout (never its .git or .kiro), with every cache and
#     Hover data folder under .sandbox/ in it, and temp files in srt's /tmp/claude;
#   - can't read SSH or cloud keys, keychains, mail, other apps' data or the user's
#     documents (only this checkout);
#   - can't reach the window server or send Apple Events: no windows, no focus taken,
#     no apps launched or scripted (srt's macOS profile allows neither);
#   - the network only through srt's proxy, to package registries and GitHub, with
#     macOS's certificate service (trustd) in reach, without which .NET, Go and
#     Security-framework TLS can't verify a certificate
#     (HOVER_SANDBOX_DOMAINS="a.com,*.b.com" adds more; --offline allows none);
#   - background QoS (taskpolicy -b): efficiency cores and throttled disk I/O.
#
#   scripts/sandbox.sh [--offline] -- <command> [args...]
#   scripts/sandbox.sh check      prove the boundary holds
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
BOX="$ROOT/.sandbox"
SRT="${HOVER_SRT_EXE:-$(command -v srt || true)}"
usage() { echo 'usage: scripts/sandbox.sh [--offline] -- <command> [args...] | check' >&2; exit 64; }
[[ -x "$SRT" ]] || { echo 'srt is not installed: npm install -g @anthropic-ai/sandbox-runtime@0.0.78 (and brew install ripgrep)' >&2; exit 69; }
command -v rg >/dev/null || { echo 'srt needs ripgrep on macOS: brew install ripgrep' >&2; exit 69; }

OFFLINE=0
MODE=run
case "${1:-}" in
  check) MODE=check; shift ;;
  --offline) OFFLINE=1; shift; [[ "${1:-}" == -- ]] && shift ;;
  --) shift ;;
  *) usage ;;
esac
[[ "$MODE" == check || $# -gt 0 ]] || usage

mkdir -p "$BOX"/{cache,nuget,npm,dotnet-cli,hover-data}
# Temp files go in srt's own /tmp/claude, which it lets sandboxed commands write: a
# path short enough for Unix sockets (104 bytes), which MSBuild and the compiler
# server use between their processes. Sockets work there and nowhere else.
TMP="/private/tmp/claude/hover"
mkdir -p "$TMP"
# Apple's tools (sips, iconutil, xcrun) write to the per-user temp folder whatever
# TMPDIR says; that folder (temp files only) is writable too.
DARWIN_TMP="$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null || true)"
# Spotlight leaves it alone, so nothing built here (Hover.app) is registered as an app.
touch "$BOX/.metadata_never_index"

DOMAINS='"api.nuget.org","*.nuget.org","registry.npmjs.org","*.npmjs.org","github.com","*.github.com","*.githubusercontent.com","localhost"'
if [[ -n "${HOVER_SANDBOX_DOMAINS:-}" ]]; then
  IFS=',' read -ra EXTRA <<< "$HOVER_SANDBOX_DOMAINS"
  for d in "${EXTRA[@]}"; do [[ "$d" =~ ^[A-Za-z0-9*.:-]+$ ]] && DOMAINS+=",\"$d\""; done
fi
[[ "$OFFLINE" == 1 ]] && DOMAINS=''
q() { python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"; }
cat > "$BOX/srt-settings.json" <<JSON
{
  "network": { "allowedDomains": [$DOMAINS], "deniedDomains": [], "allowLocalBinding": true, "allowUnixSockets": [$(q "$TMP")] },
  "filesystem": {
    "denyRead": ["~/.ssh", "~/.gnupg", "~/.aws", "~/.azure", "~/.config/gh", "~/.netrc", "~/.npmrc",
      "~/.kiro", "~/.codex", "~/.cursor", "~/.local/share/opencode", "~/.cua-driver",
      "~/Library/Keychains", "~/Library/Containers", "~/Library/Group Containers", "~/Library/Mail",
      "~/Library/Messages", "~/Library/Safari", "~/Library/Cookies", "~/Library/Application Support",
      "~/Documents", "~/Desktop", "~/Downloads", "~/Pictures", "~/Movies", "~/Music", "~/.Trash"],
    "allowRead": [$(q "$ROOT")],
    "allowWrite": [$(q "$ROOT")${DARWIN_TMP:+, $(q "${DARWIN_TMP%/}")}],
    "denyWrite": [$(q "$ROOT/.git"), $(q "$ROOT/.kiro"), $(q "$ROOT/.sandbox/srt-settings.json")]
  },
  "allowAppleEvents": false,
  "enableWeakerNetworkIsolation": true
}
JSON

# Tools keep their state here, not in the user's home.
export TMPDIR="$TMP/" DOTNET_CLI_HOME="$BOX/dotnet-cli" NUGET_PACKAGES="$BOX/nuget" \
  npm_config_cache="$BOX/npm" XDG_CACHE_HOME="$BOX/cache" CLANG_MODULE_CACHE_PATH="$BOX/cache/clang" \
  HOVER_DATA_DIR="$BOX/hover-data" HOVER_SANDBOXED=1 DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 \
  DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 DOTNET_GENERATE_ASPNET_CERTIFICATE=false DOTNET_EnableDiagnostics=0 \
  HOMEBREW_NO_AUTO_UPDATE=1 GIT_OPTIONAL_LOCKS=0 \
  MSBUILDDISABLENODEREUSE=1 DOTNET_CLI_USE_MSBUILD_SERVER=0 UseSharedCompilation=false
cd "$ROOT"
# srt points TMPDIR at its own /tmp/claude; ours goes back on inside.
run() { exec /usr/sbin/taskpolicy -b /usr/bin/nice -n 10 "$SRT" --settings "$BOX/srt-settings.json" -- /usr/bin/env TMPDIR="$TMPDIR" "$@"; }

# MSBuild's worker nodes talk over sockets it puts at /tmp/MSBuild<pid>, outside
# anything srt can allow without opening every socket in /tmp (ssh-agent's among
# them), so a dotnet build here stays in one process: -m:1 goes on dotnet's build
# verbs, and scripts that run dotnet themselves get it from a Directory.Build.rsp in
# their own copy (scripts/test-macos.sh writes one).
if [[ "$MODE" == run && "$(basename "$1")" == dotnet && "${2:-}" =~ ^(build|test|publish|pack|msbuild|run|restore)$ ]]; then
  set -- "$1" "$2" -m:1 "${@:3}"
fi

if [[ "$MODE" == check ]]; then
  # Each probe must be refused; the script fails loudly if one gets through.
  PROBE="$HOME/.hover-sandbox-probe-$$"
  /usr/sbin/taskpolicy -b "$SRT" --settings "$BOX/srt-settings.json" -- /bin/bash -c '
    fail=0
    if echo x > "$1" 2>/dev/null; then echo "FAIL: wrote outside the checkout ($1)"; fail=1; else echo "ok: writes outside the checkout are refused"; fi
    if echo x > "$2/.git/hover-probe" 2>/dev/null; then echo "FAIL: wrote into .git"; fail=1; else echo "ok: .git is read-only"; fi
    if ls "$HOME/Documents" >/dev/null 2>&1; then echo "FAIL: read ~/Documents"; fail=1; else echo "ok: ~/Documents is unreadable"; fi
    if ls "$HOME/Library/Group Containers" >/dev/null 2>&1; then echo "FAIL: read other apps data"; fail=1; else echo "ok: other apps data (Office, Mail...) is unreadable"; fi
    if /usr/bin/osascript -e "tell application \"System Events\" to count processes" >/dev/null 2>&1; then echo "FAIL: sent an Apple Event"; fail=1; else echo "ok: Apple Events are refused (no apps scripted or launched)"; fi
    # With the window server in reach this returns the login session; without it,
    # nothing, and then no window can be drawn and no focus taken.
    if /usr/bin/python3 -c "import ctypes,sys; cg=ctypes.CDLL(\"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics\"); cg.CGSessionCopyCurrentDictionary.restype=ctypes.c_void_p; sys.exit(0 if cg.CGSessionCopyCurrentDictionary() else 1)" 2>/dev/null; then echo "FAIL: reached the window server"; fail=1; else echo "ok: the window server is out of reach (no windows, no focus)"; fi
    if /usr/bin/curl -s -m 8 -o /dev/null https://example.com; then echo "FAIL: reached example.com"; fail=1; else echo "ok: the network is limited to the allowlist"; fi
    if /usr/bin/curl -s -m 15 -o /dev/null https://api.nuget.org/v3/index.json; then echo "ok: allowed registries are reachable"; else echo "note: api.nuget.org unreachable (offline?)"; fi
    echo x > "$2/.sandbox/probe" && rm "$2/.sandbox/probe" && echo "ok: the checkout is writable"
    exit $fail' probe "$PROBE" "$ROOT"
  status=$?
  rm -f "$PROBE"
  exit $status
fi
run "$@"
