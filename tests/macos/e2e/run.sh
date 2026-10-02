#!/bin/bash
# Hover's E2E run, in the background: the real office, bridge, browser and packaged
# backend, with a stand-in agent and gh, against a local test site. Nothing shows on
# the screen (no window, the harness never activates), nothing outside a temp folder
# is written, and no network is reached but this machine's own. Real apps are never
# touched: computer use is reported by the stand-in agent, and Control's clicks are
# checked as Hover maps them, not sent.
#   tests/macos/e2e/run.sh <Hover.app>    (build it first with scripts/build-macos.sh)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd -P)"; ROOT="$(cd "$HERE/../../.." && pwd -P)"
APP="${1:?usage: run.sh <Hover.app>}"
BOX="$(mktemp -d /private/tmp/hover-e2e.XXXXXX)"
echo "E2E folder: $BOX"
mkdir -p "$BOX/bin" "$BOX/data" "$BOX/tmp"
# The harness: Hover's own Swift (OfficeHost, AgentBrowser, Screen) with E2E.swift as main.
cp "$HERE/E2E.swift" "$BOX/main.swift"
xcrun swiftc -swift-version 5 -target "$(uname -m)-apple-macos14.0" -framework AppKit -framework WebKit -framework ScreenCaptureKit -framework Security \
  "$BOX/main.swift" "$ROOT/macos/Sources/OfficeHost.swift" "$ROOT/macos/Sources/AgentBrowser.swift" "$ROOT/macos/Sources/Screen.swift" "$ROOT/macos/Sources/Spaces.swift" -o "$BOX/e2e"
# A project the agent works in: a git repository with a remote of its own.
P="$BOX/project"; mkdir -p "$P"; printf '<form>\n</form>\n' > "$P/login.html"
export GIT_CONFIG_GLOBAL="$BOX/gitconfig" GIT_CONFIG_NOSYSTEM=1
git config --global user.name e2e; git config --global user.email e2e@example.invalid; git config --global init.defaultBranch main; git config --global commit.gpgsign false
git -C "$BOX" init -q --bare remote.git
git -C "$P" init -q; git -C "$P" add -A; git -C "$P" commit -qm init; git -C "$P" remote add origin "$BOX/remote.git"; git -C "$P" push -q -u origin main
# The stand-ins: Codex's ACP adapter (the agent), codex (signed in) and gh (signed in;
# no pull request until one is made).
cp "$HERE/fake-agent.py" "$BOX/bin/codex-acp"; chmod +x "$BOX/bin/codex-acp"
cp "$HERE/fake-cua.py" "$BOX/bin/cua"; chmod +x "$BOX/bin/cua"
printf '#!/bin/sh\necho "Logged in using ChatGPT (e2e)"\n' > "$BOX/bin/codex"; chmod +x "$BOX/bin/codex"
cat > "$BOX/bin/gh" <<GH
#!/bin/sh
case "\$1 \$2" in
  "--version "*) echo "gh version 2.80.0 (e2e)";;
  "auth status") echo "github.com"; echo "  ✓ Logged in to github.com account e2e-user (keyring)";;
  "auth setup-git") ;;
  "pr view") if [ -f "$BOX/pr-made" ]; then echo '{"number":7,"title":"Check sign-in","state":"OPEN","isDraft":false,"url":"https://github.com/acme/demo/pull/7","headRefName":"hover/x","baseRefName":"main","additions":1,"deletions":0,"changedFiles":1,"body":"","author":{"login":"e2e-user"},"reviewDecision":"","statusCheckRollup":[],"updatedAt":"","comments":[]}'; else echo "no pull requests found for branch" >&2; exit 1; fi;;
  "pr create") printf '%s\n' "\$@" > "$BOX/gh-create.txt"; touch "$BOX/pr-made"; echo "https://github.com/acme/demo/pull/7";;
  *) echo "gh e2e: \$*" >&2; exit 2;;
esac
GH
chmod +x "$BOX/bin/gh"
# The test site the agent opens in Hover's browser.
PORT=$((47000 + RANDOM % 1000))
/usr/bin/python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$HERE/site" >/dev/null 2>&1 &
SITE_PID=$!; trap 'kill $SITE_PID 2>/dev/null || true' EXIT
sleep 1
cat > "$BOX/e2e.sb" <<SB
(version 1)
(allow default)
(deny file-write*)
(allow file-write* (subpath "$BOX") (subpath "/private/var/folders") (literal "/dev/null") (literal "/dev/tty") (subpath "$HOME/Library/Caches") (subpath "$HOME/Library/WebKit") (subpath "$HOME/Library/HTTPStorages"))
(deny file-read* (subpath "$HOME/.ssh") (subpath "$HOME/.config/gh") (subpath "$HOME/Library/Mail") (subpath "$HOME/Library/Messages") (subpath "$HOME/Library/Group Containers"))
(deny network*)
(allow network* (local ip "localhost:*") (remote ip "localhost:*") (remote unix-socket) (local unix-socket))
SB
export PATH="$BOX/bin:/usr/bin:/bin:/usr/sbin" HOVER_SANDBOXED=1 HOVER_SANDBOX_ROOT="$BOX" HOVER_E2E_ROOT="$BOX" HOVER_E2E_SITE="http://127.0.0.1:$PORT" \
  HOVER_BROWSER_SOCKET="$BOX/b.sock" HOVER_SANDBOX_TMP="$BOX/tmp" HOVER_E2E_HOLD=12 TMPDIR="$BOX/tmp/" DOTNET_CLI_TELEMETRY_OPTOUT=1
cd "$BOX"
/usr/sbin/taskpolicy -b /usr/bin/sandbox-exec -f "$BOX/e2e.sb" "$BOX/e2e" "$APP" "$BOX" "$P"
