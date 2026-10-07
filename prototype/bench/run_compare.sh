#!/bin/bash
# usage: run_compare.sh <rust|cs-fd|cs-aot> <script.hms> <out-base> <runs>
# One Xvfb per run (1920x1080, scale 1, no compositor — same for both apps), hover-measure drives the app through its HOVER_BENCH protocol,
# cpu_sampler.py records the root process's CPU. Memory = hover-measure's samples (USS on Linux).
set -u
APP=$1; SCRIPT=$2; BASE=$3; RUNS=${4:-3}
H=/projects/sandbox/Hover; B=/projects/sandbox/bench; P=/projects/sandbox/hover-avalonia/prototype
HM=$H/target/release/hover-measure
export XDG_RUNTIME_DIR=/tmp/xdg HOME=/tmp/hhome DOTNET_ROOT=/root/.local/share/mise/dotnet-root DOTNET_CLI_TELEMETRY_OPTOUT=1; mkdir -p /tmp/xdg $HOME; chmod 700 /tmp/xdg
for r in $(seq 1 "$RUNS"); do
  D=$BASE-$r; rm -rf "$D" $B/data-run; mkdir -p "$D"; cp -r $B/data-rust $B/data-run
  export DISPLAY=:$((60+r)); Xvfb $DISPLAY -screen 0 1920x1080x24 +extension RANDR +extension XTEST >/dev/null 2>&1 & XP=$!; sleep 2
  case $APP in
    rust)   EXE=$H/target/release/hoverai; COMM=hoverai; ENVS=();;
    cs-fd)  EXE=$B/publish/fd/HoverAvalonia; COMM=HoverAvalonia; ENVS=(--env AVALONIA_GLX_IGNORE_RENDERER_BLACKLIST=1 --env HOVER_PROTO_ARGS="--preset busy --bots 3");;
    cs-aot) EXE=$B/publish/aot/HoverAvalonia; COMM=HoverAvalonia; ENVS=(--env AVALONIA_GLX_IGNORE_RENDERER_BLACKLIST=1 --env HOVER_PROTO_ARGS="--preset busy --bots 3");;
  esac
  python3 $P/bench/cpu_sampler.py $COMM $D/cpu.csv & CS=$!
  timeout 400 $HM run --exe $EXE --script $SCRIPT --out $D --data $B/data-run --path-first $B/fakebin --interval-ms 250 "${ENVS[@]}" > $D/run.log 2>&1
  kill $CS $XP 2>/dev/null; wait $CS 2>/dev/null
  echo "run $r done: $(grep -a 'startup\|first-office' $D/markers.csv | tr '\n' ' ')"; python3 $P/bench/cpu_report.py $D
done
$HM summarize $BASE-* --md $BASE-summary.md > /dev/null 2>&1; cat $BASE-summary.md | sed -n '1,30p'
