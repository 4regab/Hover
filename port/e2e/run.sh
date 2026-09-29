#!/bin/sh
# Hover end to end on Linux: each scenario starts Xvfb, an AT-SPI bus and the real
# hover (native/target/release/hover), with fake-agent.py standing in for kiro-cli,
# codex-acp and cursor-agent (and fake-opencode.py for opencode), and drives it with X input (XTEST), finding what to click
# through AT-SPI. Needs: Xvfb, dbus-run-session, at-spi2-core, python3 with
# python-xlib, Pillow and PyGObject. Usage: port/e2e/run.sh [sA sB ...]
set -u
E2E=$(cd "$(dirname "$0")" && pwd)
WORK=${E2E_WORK:-$E2E/work}
export E2E_WORK="$WORK"
mkdir -p "$WORK/bin" "$WORK/out"
for n in kiro-cli codex-acp codex cursor-agent; do ln -sf "$E2E/fake-agent.py" "$WORK/bin/$n"; done
ln -sf "$E2E/fake-opencode.py" "$WORK/bin/opencode"
# The folder picker, and the browser a link opens in.
printf '#!/bin/sh\ncase "$*" in *--directory*) echo "$E2E_PROJ";; esac\n' > "$WORK/bin/zenity"
printf '#!/bin/sh\necho "$@" >> "$E2E_WORK/browser.log"\n' > "$WORK/bin/browser"
cp "$WORK/bin/browser" "$WORK/bin/xdg-open"
chmod +x "$E2E/fake-agent.py" "$E2E/fake-opencode.py" "$WORK/bin/zenity" "$WORK/bin/browser" "$WORK/bin/xdg-open"
[ $# -gt 0 ] || set -- sA sB sC sD sE sF sH sI sJ sK sL sM
fail=0
for s in "$@"; do
  rm -f "$WORK/agent.log" "$WORK/browser.log"
  echo "## $s"
  out=$(cd "$E2E" && timeout -k 5 300 dbus-run-session -- python3 -u "$s.py" 2>/dev/null)
  echo "$out" | grep -E '^(PASS|FAIL|==)'
  echo "$out" | grep -q '^== ' || { echo "FAIL $s did not finish"; fail=1; }
  echo "$out" | grep -q '^FAIL' && fail=1
done
exit $fail
