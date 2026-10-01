# Profiling and memory

Measure the release build with its normal renderer and default settings. Use a profiling
build only to find where memory goes, never for the numbers you report.

## Counters

`hover-measure` samples Hover's process tree from outside the process (every 250 ms by
default) into `samples.csv`. Scenario markers go in `markers.csv`.

| Column | Windows | Linux |
|---|---|---|
| `private` | private commit (PrivateUsage) | private resident pages (USS) |
| `private_resident` | private working set | USS again |
| `resident` | working set (includes shared DLL pages) | RSS |
| `pss`, `swap` | – | PSS, swap |
| GPU dedicated / shared | the process's GPU memory (PDH GPU Process Memory) | – |
| handles, threads | kernel handles, threads | open files, threads |

Report private commit, or USS/PSS on Linux, and name the counter. A working set alone isn't
the total. Don't add counters that overlap (the working set already contains the private
working set). Children such as tools and quota reads are summed separately (`tools private`)
and together with Hover (`tree private`).

## Runs

```powershell
# Windows, a logged-on session
.\tools\hover-measure\run-memory.ps1 -Exe target\release\hoverai.exe -Out out\after -Runs 3
# The same, when the shell is in session 0 (SSM/SSH): the app runs in the console session through PsExec
.\tools\hover-measure\run-console.ps1 -PsExec C:\tools\PsExec64.exe -Exe ... -Out ... -Runs 3 -Env SLINT_WGPU_CPU=1
.\target\release\hover-measure.exe summarize out\before\run1 out\before\run2 out\before\run3 --md before.md
```

`SLINT_WGPU_CPU=1` is needed only where the only adapter is WARP (a VM with no GPU). Label
numbers from such a machine "WARP". Compare builds on the same machine and settings, as the
median of at least three runs (five for startup). `summarize` gives the median, p95 and peak
of each scenario, and the median of the runs' medians.

Scenarios: `memory.hms` (everything), `office.hms` (resting, open, dropped: a short A/B),
`quota-probe.hms` (one quota switched on while at rest), `gpu.hms`, `peak.hms`, `heap.hms`.

### Voice

Measure Cloud and Local separately. Local's helper (Phonon's Python) is a child of
Hover, so `tools private` and `tree private` include it. Report its working set too:
its 304 MB plane cache is memory-mapped (`evidence/voice-chat/phonon-proto.md`), not
private commit. Measure idle with voice on, each stage, and after start, cancel, an
error and unload (the helper gone, no orphans). The recipe is in
`evidence/voice-chat/memory/HOWTO.md`, with what a voice run still needs (bench
commands and a fixed WAV in place of the microphone). Cloud runs use a fake Groq on
this computer through `HOVER_GROQ_BASE` (only `http://127.0.0.1:PORT` is taken). A
measurement is in progress; its results go beside the HOWTO, and none are here yet.

## Where memory goes

A profiling build counts every allocation, and keeps line tables for stacks:

```powershell
$env:CARGO_PROFILE_RELEASE_STRIP="none"; $env:CARGO_PROFILE_RELEASE_DEBUG="line-tables-only"
cargo build --release -p hover --features profiling --target-dir target-prof
```

Bench commands (profiling build unless noted):

- `heap`: live bytes, peak, count and total allocated. `heap-reset`: the peak starts again.
- `heap-trace N`: every allocation of N bytes or more goes to stderr with its stack.
  `tools/hover-measure/heap-trace.ps1 hover-stderr.log` sums them by stack.
- `gpu [full]` (any build, Windows): the shared GPU device's allocator, what it has handed
  out, what it holds from the driver, and with `full` each label's total.
- `regions FILE` (a script line): the process's address space by kind, and its largest
  reservations.

## What is known (Windows, WARP)

- Resting with nothing shown: about 40 MiB private commit, 12.5 MiB private working set.
- The first time the resting island draws something (a quota, an agent at work), about 45
  MiB more is committed: Slint's femtovg renderer and D3D12 on WARP. That memory isn't on
  Rust's heap (1.3 MB live) and isn't in the GPU allocator (4.7 MB). It stays after the
  island is empty again.
- Once the office is dropped, gpu-allocator keeps the last block of each memory type
  (d3d12: `is_dedicated_or_not_last_general_block`), about 46 MiB reserved, so the device
  doesn't start empty again.
- Each chat opened read every system font file once (hundreds of MB allocated, a peak of
  about 50 MB). The fonts are now listed once per process, from mapped files, and only when
  a flowchart is drawn.
