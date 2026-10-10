# Profiling and memory

Measure the published build with its normal renderer and default settings.

## Counters

Report **private** memory (Windows: private commit, `PrivateMemorySize64`; Linux: USS or PSS) and
name the counter. A working set alone isn't the total, and counters that overlap shouldn't be
added (the working set already contains the private working set). Children such as the agent
tools, quota reads and Phonon's Python are separate processes: add them up on their own.

```powershell
Get-Process hoverai | Select-Object Name, PrivateMemorySize64, WorkingSet64
```

`tools/app-smoke.ps1` reads the same two numbers for you: `rest_private_mb` and
`rest_working_set_mb` with the notch at rest, and `private_mb` and `working_set_mb` after the
app window has been opened. CI keeps its `report.json` in the `windows-reports` artifact.

Compare builds on the same machine and settings, as the median of at least three runs.

## What is known

On CI's Windows runner, which has no GPU (the office is drawn by WARP, a software adapter):

- At rest: about 70 MB private, about 45 MB working set. The Rust app this one replaced held
  about 35 MB private on the same runner (69.8 against 34.7 MB, measured before it was removed).
  The extra is memory the Go runtime and Direct3D hold; the working sets were alike. Accepted.
- With the app window open: about 600 MB private, about 290 MB working set. Label numbers from a
  machine like this "WARP"; a real GPU differs.

## Voice

Measure Cloud and Local separately. Local's helper (Phonon's Python) is a child of Hover, so
include it in the total. Its large plane cache is memory-mapped, so it shows in the working set
and not in private memory. Cloud runs can use a fake Groq on this computer through
`HOVER_GROQ_BASE` (only `http://127.0.0.1:PORT` is taken).

## Where memory goes

Nothing here is wired for Go yet. Use Go's own tools: build with a `net/http/pprof`-style
listener or `runtime/pprof` in a scratch change, and `GODEBUG=gctrace=1` for the collector.
