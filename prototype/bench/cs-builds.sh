#!/bin/bash
# C# build timings. NuGet packages are already restored/downloaded. Usage: cs-builds.sh <Debug|Release> <runs>
# clean   : rm -rf bin obj, build servers shut down first (cold compiler server), then `dotnet build -c <cfg>`
# nochange: build again immediately
# ui      : one-value edit in Ui/NotchWindow.axaml (mini-pill text margin 22 <-> 23)
# logic   : one-value edit in Office/Bot.cs (Pip's colour 0x9b6bff <-> 0x9b6bfe)
set -u
CFG=${1:-Debug}; RUNS=${2:-3}
cd "$(dirname "$0")/../avalonia/HoverAvalonia"
OUT=${OUT:-/projects/sandbox/bench}/cs-builds-$CFG.csv
echo "config,kind,run,seconds" > "$OUT"
t() { local s=$(date +%s.%N); dotnet build -c "$CFG" --nologo -v q > /tmp/csb.log 2>&1; local rc=$?; local e=$(date +%s.%N); [ $rc -ne 0 ] && { echo "BUILD FAILED"; tail -20 /tmp/csb.log; exit 1; }; echo "$e - $s" | bc; }
for r in $(seq 1 "$RUNS"); do
  rm -rf bin obj; dotnet build-server shutdown >/dev/null 2>&1
  echo "$CFG,clean,$r,$(t)" >> "$OUT"
  echo "$CFG,nochange,$r,$(t)" >> "$OUT"
  if grep -q 'Margin="22,0,5,0"' Ui/NotchWindow.axaml; then sed -i 's/Margin="22,0,5,0"/Margin="23,0,5,0"/' Ui/NotchWindow.axaml; else sed -i 's/Margin="23,0,5,0"/Margin="22,0,5,0"/' Ui/NotchWindow.axaml; fi
  echo "$CFG,ui,$r,$(t)" >> "$OUT"
  if grep -q '0x9b6bff' Office/Bot.cs; then sed -i '0,/0x9b6bff/s//0x9b6bfe/' Office/Bot.cs; else sed -i '0,/0x9b6bfe/s//0x9b6bff/' Office/Bot.cs; fi
  echo "$CFG,logic,$r,$(t)" >> "$OUT"
done
# put both edits back exactly
git checkout -- Ui/NotchWindow.axaml Office/Bot.cs 2>/dev/null || true
cat "$OUT"
