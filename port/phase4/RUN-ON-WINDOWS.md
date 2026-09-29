# Phases 2 and 4 on a real Windows PC

Do phase 3's `RUN-ON-WINDOWS.md` first (build, round trips). Then:

```powershell
cd native
cargo build --release --workspace
cargo test --release --workspace --no-fail-fast
```

## Phase 2: the office

| Check | How | Pass when |
|---|---|---|
| The adapter | Open the office; read `hover.log` in the data folder | an `office:` error is absent and the frames come from a DX12 hardware adapter, not WARP |
| Scene ΔE on Windows | `target\release\examples\shot.exe night.png night` (and `day`, `night --empty`); `node port\phase2\capture-scene.mjs out`; `python port\phase2\compare.py out\page-scene-night.png night.png diff.png` | mean ΔE ≤ 2.0; the over-10 share as on Linux or better (REPORT question 7) |
| The UI views | Open each panel, the new-task circle (rest, pick, box), the drawer, the toast, the delete confirmation; compare with WebView2 captures from `capture-office.mjs` | SCREENS.md §4 tolerances, or noted as a difference |
| Glass | Drawer, menu and toast over a busy part of the room | the blur follows the room behind, with no seam at the edges |
| Drop and remake | Open, fold, wait 35 s (the log says `office dropped`), open | the camera and the open chat come back; Task Manager's GPU memory falls after the drop |
| Pacing | Task Manager's GPU engine with the office open and idle, then with a bot working | about 10 fps idle and 30 fps busy (PresentMon if installed) |

## Phase 4

| Check | How | Pass when |
|---|---|---|
| Benchmark, C# | `pwsh port/bench/Measure-Hover.ps1 -Exe .\publish\Hover.exe -Impl csharp -Out .\bench-cs` (5 runs) | it completes; keep the medians |
| Benchmark, native | `pwsh port/bench/Measure-Hover.ps1 -Exe .\native\target\release\hover.exe -Impl native -Out .\bench-rs` (5 runs, same PC, same power plan) | G1–G4 in BENCHMARK.md §5 against the C# medians |
| UIA | Accessibility Insights on the notch, the office, Settings | the ids in SCREENS.md and the office's HUD ids (`brand`, `timeAuto`, `timeNight`, `timeDay`, `beats`, `histBtn`, `setBtn`, `closeBtn`, `fabMain`) with names and roles |
| Installer | `iscc /DMyAppVersion=0.9.0 /DExeDir=target\release /O..\dist installer\Hover.iss` (from `native`) | `dist\Hover-Setup-0.9.0.exe` is made |
| Install over C# | With the C# Hover installed and data in `%APPDATA%\Hover`, run the new setup | one entry in Apps; the Rust exe starts; the history, settings and key open; "Start when Windows starts" still works |
| Uninstall | Apps → Hover → Uninstall | the folder and the Run value go; `%APPDATA%\Hover` stays |
