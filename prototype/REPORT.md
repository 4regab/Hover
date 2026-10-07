# Hover: Rust + Slint/wgpu → C# + Avalonia — feasibility report

**Baseline:** `main` @ `b6c5e21` (Rust 1.94.1, Slint 1.18.1, wgpu 30; v4.0.0). **Prototype:** branch `experiment/avalonia-poc` (first prototype commit `8f55c3a`; the branch tip adds the fixes, scripts and results listed in `git log`).
The branch point is the baseline; no production file was changed. Raw numbers and images are in `results/`.

## 1. Decision

**Do not start the full port on this evidence. If the migration is pursued, do it as a hybrid — Avalonia UI over the existing Rust backend — and only after two spikes on real Windows and macOS hardware, which this environment could not run.**

| Question | Answer from the evidence |
|---|---|
| Can Avalonia + C# reproduce the notch, the 3D office, the real bots and animation, camera, picking and a chat panel without Electron/WebView/Rust? | **Yes, on Linux/X11.** Working slice, ~2.6k lines of C# + 140 of XAML, own GL renderer, no readback. |
| Does it look like Hover? | **Close, not identical** — §5 lists every visible difference I found. The lighting model matches (sampled pixels where geometry coincides are byte-identical). |
| Did development builds improve enough to justify migration? | **For the edit–build loop, yes by a large factor** (2.2 s vs 19–39 s dev / 145–154 s release). Two caveats: the prototype is ~4 % of the app's code, and the Rust release profile (thin LTO) is what makes edits slow — a cheaper Rust profile would close part of the gap without migrating (not measured). |
| Did memory stay acceptable? | **Not in this prototype.** Notch-only +73 MiB (JIT) / +58 MiB (NativeAOT) over Rust, office open +42 / +26 MiB, all USS, all with *software* GL. Idle CPU is 12× Rust's under JIT (5.8 % vs 0.5 %), 3× under AOT. Whether this holds on a real GPU is **unmeasured**. A prototype's memory is not the complete app's memory. |
| Is cross-platform parity shown? | **No.** Only Linux/X11 was run. Windows and macOS were compiled and published, never executed. |

## 2. What was built (and how the 3D path was chosen)

Avalonia is the UI framework, not a 3D engine. Official documentation shows three GPU-embedding routes: `OpenGlControlBase` (GL context shared with the compositor), `CompositionDrawingSurface` + `ICompositionGpuInterop` (import D3D11 shared handles on Windows, IOSurface on macOS, per-API), and Skia custom draw operations.
I chose **`OpenGlControlBase` + Silk.NET.OpenGL**, a C# port of Hover's renderer (GLSL ES 3.00 / GL 3.30 from `office.wgsl`):

* one code path for the three desktops (ANGLE→D3D11 on Windows, GLX/EGL on Linux, CGL on macOS), pure C#, no Rust build;
* no frame readback in the design (Hover on Linux reads every office frame back to the CPU; its Windows build shares a wgpu device and avoids it);
* **limits:** macOS OpenGL is deprecated and Avalonia's macOS compositor defaults to Metal — whether `OpenGlControlBase` works there without forcing the GL compositor is **unverified**; the future-proof route (`ICompositionGpuInterop` with IOSurface/D3D11, i.e. one renderer per graphics API) is more work. Avalonia 12 refuses Mesa llvmpipe by default (renderer blacklist); real GPUs are unaffected, this sandbox needed `--software-gl`.
* Not a hybrid: no Rust is involved in the prototype. (Keeping the wgpu renderer behind interop was not needed.)

Ported from the Rust source, not re-invented: the room (`scene.rs`, same RNG sequence, same boxes), all six bot stages and acts (`bot.rs`), camera/lights/day-night/picking (`office.rs`), shading (`office.wgsl`), notch geometry/animation/hover rules (`hover-notch`), pacing (30 fps working / 10 idle), platform behaviours (`notch.rs`, `x11.rs`, `win.rs` approach). Hover's own fonts are linked from `app/assets`.

## 3. Measurements

**Environment.** Amazon Linux 2023, kernel 6.1.186, Intel Xeon Platinum 8488C, 8 vCPU, 31 GiB, **no GPU**. Rust 1.94.1 (`rust-toolchain.toml`), .NET SDK 9.0.318 / runtime 9.0.20, Avalonia 12.1.2, Silk.NET.OpenGL 2.22.0, Xvfb 21.1.13 (1920×1080, scale 1), Mesa 24.2.6 llvmpipe (LLVM 15), xcompmgr 1.1.9. Dependencies were already downloaded. Commands are in `README.md`, `bench/*.sh`, `results/build/*.csv`.
n = runs; "median [min–max]". Memory is **USS (private resident) from `tools/hover-measure`, the same tool and metric for all three apps, root process only** (agent child processes excluded). All Linux; not comparable to Windows/macOS metrics.

### 3.1 Build times (seconds)

| | Rust dev (`cargo build`) | Rust release (`cargo build --release`) | C# Debug | C# Release |
|---|---|---|---|---|
| Clean build, deps downloaded | 191.7 (n=1) | 263.5 (n=1) | 4.56 [4.51–6.44] (3) | 4.62 [4.56–4.63] (3) |
| No-change rebuild | 0.39 [0.29–0.40] (3) | 0.30 [0.29–0.31] (3) | 1.03 [0.99–1.06] (3) | 1.03 [0.99–1.05] (3) |
| Small UI edit (`.slint` / `.axaml` value) | 39.1 [37.5–51.6] (3) | 153.8 [153.7–154.4] (3) | 2.20 [2.13–2.32] (3) | 2.26 [2.24–2.27] (3) |
| Small logic edit (one constant) | 19.2 [19.2–19.6] (3) | 145.1 [144.9–156.5] (3) | 2.13 [2.07–2.21] (3) | 2.14 [2.10–2.16] (3) |

* Clean Rust numbers are n=1 (I aborted the repeat runs because they take 3–4.5 min each; a later 150 s "rebuild" had a partly warm cache and is not a clean number).
* **The clean comparison is not like-for-like:** Rust compiles ~500 crates (Slint, wgpu, …) from source; C# uses prebuilt NuGet binaries. The edit loops are the fair comparison. The prototype is one 2.7k-line project; the real app's C# would be bigger, so its edit times would grow (the Rust edit time is dominated by the one big `hover` crate + thin LTO in release).
* Publish (release artefact), separately: framework-dependent 4.2 s first publish (1.1 s when up to date); self-contained 2.6 s; self-contained + partial trim 13.1 s (n=1); **NativeAOT 39.8 s (n=1)** — never the dev loop. Cross-publish for `win-x64`, `osx-arm64`, `osx-x64`, `linux-arm64` succeeded (3.9–6.5 s each).

### 3.2 Published size (Linux x64)

| Build | Size | Notes |
|---|---|---|
| Rust `hoverai` release | **57.1 MB** | single executable, `strip = symbols` |
| C# framework-dependent | 28.6 MB (38 files) | **plus** a .NET 9 runtime that must be installed |
| C# self-contained | 108.1 MB (222 files) | |
| C# self-contained, partial trim | 50.3 MB (95 files) | trim warnings in `SelfTest.cs` only |
| C# NativeAOT | **43.1 MB** | exe 29.1 + libSkiaSharp 11.2 + libHarfBuzzSharp 2.8 (a 59 MB `.dbg` is not shipped) |

Windows self-contained (untrimmed, with pdbs) is 216 MB and macOS 112–119 MB; measured on disk only, not run.

### 3.3 Runtime (3 runs each; scenario `bench/compare.hms`, window 1120×440 DIP, scale 1, 3 agents "working")

Same script, same tool, same Xvfb. Rust sessions are real `fake-agent` ACP processes; the prototype's three agents are simulated in-scene (`--preset busy`, acts cycling). **Software GL on both:** Hover detects the CPU adapter and renders the office at half resolution; the prototype renders full resolution on llvmpipe. CPU in the office rows is therefore dominated by llvmpipe (~6.5 cores) and says nothing about a GPU.

| | Rust (release) | C# JIT (Release) | C# NativeAOT |
|---|---|---|---|
| Startup until `bench visible` (ms) | 91.9 [90.3–179.3] | 591.5 [569.0–601.9] | 96.0 [94.4–180.9] |
| First office frame after unfold (ms) | 210.8 [205.9–817.6] | 200.3 [199.8–576.0] | 144.5 [144.4–526.2] |
| Notch only — USS (MiB) | 82.0 [80.9–102.0] | 154.9 [151.9–163.1] | 140.1 [137.7–149.2] |
| Notch only — CPU (% of 1 core) | 0.5 [0.4–1.4] | 5.8 [5.5–6.3] | 1.6 [1.6–1.6] |
| Office + 3 bots working — USS (MiB) | 205.4 [182.5–208.1] | 247.0 [244.3–260.5] | 231.2 [229.2–244.6] |
| Office + 3 bots working — PSS (MiB) | 214.4 [191.8–216.2] | 275.8 [272.9–289.2] | 246.4 [244.5–260.1] |
| Office + 3 bots working — CPU (%) | 643 [641–644] | 668 [667–668] | 671 [671–671] |
| Office frames in 25 s (count) / p50 / p95 / p99 (ms) | 1441–1449 / 16.7 / 26.8–30.1 / 31.9–32.7 | 625–631 / 39.5 / 42.7–43.2 / 44.3–45.0 | 634–636 / 39.2 / 42.1–42.3 / 43.6–43.9 |

Reading the table:
* Rust's "frames" are presented frames of the Slint window (they exceed its 30 fps office cap), the prototype's are office frames; software-GL frame times only say that llvmpipe at full resolution holds ~25 fps. **No claim about frame times on a GPU can be made from this.** Both start from a cold first run (first number in each range).
* The "notch only" rows are not identical content: Rust at rest with no sessions draws nothing; the prototype shows a static demo pill (clock ticks 1 Hz). Notch-with-agents CPU is not compared: Rust's ~92 % there is it ingesting three live 20-updates/s agent streams, which the simulation does not do.
* USS of the *whole process* includes the GL driver. The prototype initialises Avalonia's GL/Skia stack even while the notch rests, which probably explains part of the idle difference (I did not isolate it); with a real driver both numbers change.

**Memory after opening and closing the office repeatedly** (`bench/cycles.hms`, USS MiB, n=1; office dropped 30 s after hiding in both):

| | before | open | dropped | open | dropped | open | dropped |
|---|---|---|---|---|---|---|---|
| Rust | 123 | 248 | 185 | 244 | 158 | 253 | 155 |
| C# JIT | 186 | 285 | 269 | 295 | 295 | 299 | 296 |
| C# NativeAOT | 140 | 231 | 209 | 247 | 246 | 247 | 246 |

Rust gives most of the office back; the prototype keeps ~+100 MiB after the first open (GL/Skia/Mesa caches, JIT code) and creeps ~+5 MiB per cycle under JIT. Three cycles cannot show a leak; they do show the "drop the office" behaviour does not reclaim memory in the prototype.

## 4. Platform verification

| Behaviour | Linux X11 | Windows | macOS |
|---|---|---|---|
| Builds / publishes | **runtime tested** (JIT + NativeAOT) | compiles + publishes (win-x64), **not run** | compiles + publishes (osx-arm64, osx-x64), **not run** |
| Transparent window, rounded notch shape | **runtime tested** — *needs a compositing manager*; with none (bare Xvfb) transparent pixels render black | expected to work (DirectComposition path), untested | expected to work, untested |
| Always on top, no taskbar | dock-type + above hint; tested without a window manager only | `WS_EX_TOOLWINDOW`+topmost coded, untested | `NSStatusWindowLevel` coded, untested |
| Click-through outside the shape | **runtime tested**: XShape input region, verified with the real pointer (`XQueryPointer` child): over the pill → our window; inside the window rect but outside the shape, and left of the shape → not our window | `WS_EX_TRANSPARENT` toggled from the polled pointer — coded, untested | `ignoresMouseEvents` toggled — coded, untested |
| Never takes focus while resting | tested: focus not ours at rest (no window manager present) | `WS_EX_NOACTIVATE` coded, untested | **Known gap:** Hover uses a non-activating `NSPanel`; Avalonia owns the `NSWindow` class, so this needs a small native shim |
| Hover dwell opens (185 ms), leaving collapses (~390 ms), stays open over the panel | **runtime tested** | untested | untested |
| Global Alt+N | **runtime tested** (XGrabKey, real XTest key events, toggles both ways) | `RegisterHotKey` coded, untested | **not implemented** (needs Carbon hot key) |
| Esc closes | coded; **not exercised** with a real key event | untested | untested |
| Display scaling | 1× and 2× (`AVALONIA_GLOBAL_SCALE_FACTOR=2`) screenshots only; per-monitor DPI untested | untested | untested |
| Multi-monitor | re-places on display-signature change; **untested** (Xvfb exposes one screen; Avalonia reported it `IsPrimary=false`, code falls back to the first screen) | untested | untested; hardware-notch geometry (safe-area insets) not implemented |
| Wayland | **Unsupported by Avalonia 12 for general use** (its release notes call Wayland groundwork/private preview); Hover already runs through XWayland — **XWayland untested** here (no Wayland compositor) | – | – |
| GPU path | llvmpipe software GL **only**; real GPU, DRI3 sharing, GPU memory: **unmeasured** | ANGLE→D3D11: **unmeasured** | GL vs Metal compositor: **unverified** |

Self-test (`results/screenshots/selftest/report.json`): 13/13 checks passed on X11 — pill hit, two click-through checks, rest focus, hover-peek, stays open over the panel, bot selection zooms the camera, drag and wheel move/zoom the camera, leaving collapses, Alt+N opens and closes.

## 5. Visible differences from Hover (screenshots in `results/screenshots/`)

Baseline images are Hover's `--shots` output (Slint software renderer); prototype images are live X11 captures under llvmpipe. No automated pixel diff was run. Differences I can see:

1. **Wall canvases are simplified.** Hover draws the sky, TV (text/charts), session board (list) and LED clock through a canvas with text; the prototype draws flat shapes (sky gradient/clouds/stars, TV bars, empty board, clock blocks).
2. **Tool marks are letters** in the name tags, rings and chat, not Hover's logos (SVG/PNG assets not imported). Chat header buttons use Unicode glyphs; some render as empty boxes (missing in Inter).
3. **Resting pill:** 520 DIP wide vs Hover's 468; ring/mark proportions approximate; the working agent's ring lacks Hover's second small mark.
4. **Panel background:** Hover's warm radial brown gradient is approximated by a vertical dark gradient; a faint shadow smear shows under the open panel.
5. **Camera:** a calibration factor (`--view-cal 1.115`) was needed to match the baseline's scale; its cause in Hover's harness was not found. Even then positions differ by a few pixels.
6. **Text:** Skia's LCD sub-pixel antialiasing leaves coloured fringes on the transparent window; Slint's is greyscale.
7. **Chat:** static, state-driven text (no Markdown engine, no scrolling code block, no streaming); typography (Inter weights/sizes, spacing) is matched by eye from the screenshots and sampled colours.
8. **Bubbles:** "Done! ✓" expires after 6 s as in Hover; my screenshots were taken later, the baseline freezes it.

What matches: room geometry and colours, bot models/materials/animations (all six stages), lighting and tone mapping (pixels sampled where geometry coincides — e.g. `#543020`, `#5A5D38` — are identical), the notch outline/ears/easing, tag layout.

## 6. Not built (full parity gap)

Desk card and its eight panels, Settings (nine sections), the agent runtime (ACP/OpenCode/Claude hosts, sandbox, checkpoints), voice, tray, quotas (live), history/sealed storage, Markdown/diagram rendering, subagent helper bots, bot walk-out/retire, prop tooltips and clicks (TV/board/shelf/door), music, single-instance, installers, the macOS menu-bar item and hardware-notch layout, real tool logos.
Size of what exists in Rust: ≈69k lines of Rust (≈35k in `hover-core`/`hover-agents`/`hover-quota`, UI-independent), 6.4k Slint, 4.8k Swift, 3.2k JS for the Mac web office. The Mac app already runs on `hover-backend`, a JSON-lines process boundary around the Rust backend — the natural seam for a hybrid.

**Effort (an estimate, not a measurement):** Avalonia UI parity on three OSes with the Rust backend kept: roughly 4–8 engineer-months (Slint/office_ui/pages/desk ≈ 10k lines to re-express, `hover-chat` painter, the remaining `hover-office` canvases/helpers, three platform adapters including a macOS native shim, packaging). Re-writing the backend in C# as well would be a multiple of that and is not supported by anything measured here.

## 7. Recommendation

1. **Keep Rust as the product for now.** The prototype is faster to iterate on but is heavier at idle (memory, CPU under JIT), starts slower under JIT, and does not reclaim memory like Hover does.
2. **If iteration speed is the pain, first try a cheaper Rust dev-release profile** (the repo already has a no-LTO `ci` profile). I did not measure it.
3. **If the migration is still wanted, go hybrid:** Avalonia UI + the existing `hover-backend` over JSON lines, NativeAOT for shipping (it fixed startup and most idle CPU: 96 ms, 1.6 %), and run these gating spikes on real hardware first:
   * Windows: ANGLE/D3D11 `OpenGlControlBase` with a real GPU — RSS, GPU memory, frame times, `WS_EX_*` focus/click-through, multi-monitor/DPI.
   * macOS: does `OpenGlControlBase` work with the default Metal compositor (else `ICompositionGpuInterop` + IOSurface), a non-activating panel via native shim, the notch/menu-bar layout.
   * Linux: a real window manager and a Wayland session (XWayland) — focus, stacking, dock hint vs Hover's override-redirect window.
4. Stop the migration if any of: GPU-path memory is not clearly below the numbers above; the macOS panel needs more than a thin shim; or idle CPU stays above ~1 %.

## 8. Reproducibility notes

* `xcompmgr` was built from source (`-n` mode gives real alpha; `-a` does not); without it the notch's transparent pixels are black.
* Every number above is from this one machine; `results/runtime/*/markers.csv` and `results/build/*.csv` are the raw data (memory samples were not committed, they are large).
* A tool-call abort cost me the repeat clean Rust builds; that is why those two cells are n=1.
