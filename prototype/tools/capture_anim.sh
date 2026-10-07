#!/bin/bash
# Records the opening animation + working bots as an animated GIF by polling screenshots (~8-10 fps: `import` is the limit, not the app).
# usage: capture_anim.sh <out.gif>   (needs Xvfb, xcompmgr, ImageMagick; software GL)
OUT=${1:?out.gif}; HERE=$(cd "$(dirname "$0")" && pwd); DLL=$HERE/../avalonia/HoverAvalonia/bin/Debug/net9.0/HoverAvalonia.dll; XCM=${XCOMPMGR:-xcompmgr}
export XDG_RUNTIME_DIR=/tmp/xdg HOME=/tmp/hhome DISPLAY=:75; mkdir -p /tmp/xdg /tmp/frames; rm -f /tmp/frames/*
Xvfb $DISPLAY -screen 0 1920x1080x24 +extension RANDR +extension COMPOSITE >/dev/null 2>&1 & XP=$!; sleep 2; $XCM -n >/dev/null 2>&1 & CP=$!; sleep 1
dotnet "$DLL" --software-gl --backdrop --no-hotkey --preset demo --bots 4 --day --view-cal 1.115 --open >/dev/null 2>&1 & AP=$!
i=0; t0=$(date +%s.%N)
while [ $i -lt 70 ]; do import -window root -crop 1200x480+360+0 +repage -resize 50% /tmp/frames/f$(printf %03d $i).png; i=$((i+1)); done
t1=$(date +%s.%N); fps=$(echo "$i / ($t1 - $t0)" | bc -l); echo "captured $i frames at $(printf %.1f $fps) fps"
kill $AP $CP $XP 2>/dev/null
# skip the empty-desktop frames before the app shows (frames identical to the first), keep the rest
d=$(printf %.0f "$(echo "100 / $fps" | bc -l)")
convert -delay $d -loop 0 /tmp/frames/f0[1-6]*.png -layers Optimize "$OUT"; ls -la "$OUT"
