#!/bin/bash
# Take the prototype's matching screenshots on a headless X server WITH a compositing manager (transparency needs one; see README).
# Usage: tools/capture.sh <out-dir> [scale]    Needs: Xvfb, ImageMagick (import), xcompmgr, Mesa llvmpipe (no GPU in the sandbox).
set -u
OUT=${1:?out dir}; SCALE=${2:-1}; HERE=$(cd "$(dirname "$0")" && pwd); DLL=$HERE/../avalonia/HoverAvalonia/bin/Debug/net9.0/HoverAvalonia.dll
XCM=${XCOMPMGR:-xcompmgr}
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/tmp/xdg} HOME=${HOME:-/tmp/hhome} DISPLAY=:${DISP:-77}; mkdir -p "$OUT" "$XDG_RUNTIME_DIR"
W=$((1920*SCALE)); H=$((1080*SCALE))
Xvfb $DISPLAY -screen 0 ${W}x${H}x24 +extension RANDR +extension XTEST +extension COMPOSITE >/dev/null 2>&1 & XP=$!; sleep 2
$XCM -a >/dev/null 2>&1 & CP=$!; sleep 1
[ "$SCALE" != 1 ] && export AVALONIA_GLOBAL_SCALE_FACTOR=$SCALE
run() { # name seconds args...
  local name=$1 secs=$2; shift 2
  dotnet "$DLL" --software-gl --backdrop --no-hotkey "$@" >/dev/null 2>&1 & local P=$!
  sleep "$secs"
  local ww=$((1200*SCALE)); local hh=$((480*SCALE)); local x=$(( (W-ww)/2 ))
  import -window root -crop ${ww}x${hh}+${x}+0 +repage "$OUT/$name.png"
  kill $P 2>/dev/null; wait $P 2>/dev/null
}
run rest 6 --preset shot --bots 4
run office 12 --preset shot --bots 4 --open --view-cal 1.115
run office-chat 14 --preset shot --bots 4 --open --chat --view-cal 1.115
kill $CP $XP 2>/dev/null
ls -la "$OUT"
