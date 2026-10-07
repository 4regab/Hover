# Avalonia feasibility prototype (experiment — not shipped)

A C# / Avalonia 12 slice of Hover: the notch, a 3D office with the real bots, and a chat panel, built to answer
"can Hover move from Rust + Slint/wgpu to C# + Avalonia?". Read **[REPORT.md](REPORT.md)** for the measurements and the decision.

* Baseline: `main` @ `b6c5e21` (Rust + Slint + wgpu, v4.0.0). Nothing under `app/`, `crates/`, `macos/`, `.github/` is touched.
* Everything lives under `prototype/`. No release workflow knows about it.

## Stack

| Piece | Choice |
|---|---|
| UI | Avalonia **12.1.2** (stable, Sep 2026), `net9.0`, Fluent theme, compiled bindings |
| 3D | **Own renderer in C#**: GLSL port of `office.wgsl` (three.js 0.170 shading, ACES, PCF-soft shadows) on **Silk.NET.OpenGL 2.22**, drawing into the framebuffer of Avalonia's `OpenGlControlBase`. No Rust, no WebView, no Node, no Electron |
| GPU sharing | `OpenGlControlBase` renders in a GL context shared with Avalonia's compositor, which draws the result on the GPU: **no pixel readback / CPU copy** (Hover on Linux reads every office frame back and composites it on the CPU). Only verified on Mesa llvmpipe here — see REPORT |
| Scene | Direct ports of `scene.rs` (the room, same RNG sequence), `bot.rs` (all poses/animations), `m.rs`, `js.rs`, the camera/lights/picking of `office.rs`, `hover-notch` (geometry, easing, hover state machine) |
| Platform | `IPlat` per OS: **X11** (XShape input/bounding region, XQueryPointer poll, XGrabKey Alt+N) · **Win32** (compiles only) · **macOS** (compiles only, partial) |
| Assets | Hover's own `Inter-*.ttf` and `PixelifySans.ttf`, linked from `app/assets` (not copied) |

## Build and run

Needs the .NET 9 SDK. From the repo root of this branch:

```sh
cd prototype/avalonia/HoverAvalonia
dotnet run -c Release -- --open --preset demo --bots 4          # office open, scripted agents idle -> working -> completed
dotnet run -c Release -- --preset shot --bots 4 --day --view-cal 1.115   # the four-bot state of the baseline screenshots
```

Useful flags: `--open`, `--chat` (open a bot's chat), `--bots N` (1–6), `--preset demo|shot|busy`, `--day`, `--fps N`,
`--drop-ms N` (hidden time before the office is dropped; Hover: 30000), `--no-hotkey`, `--backdrop` (opaque window behind, for screenshots),
`--software-gl` (**only for machines with no GPU** — Avalonia blacklists Mesa llvmpipe by default; this sets
`AVALONIA_GLX_IGNORE_RENDERER_BLACKLIST=1`).
Open: hover the top-centre, click the pill, or **Alt+N**. Close: Esc, Alt+N, or leave. Drag pans, wheel zooms, click a bot selects it (camera closes in, chat opens),
click the window toggles day/night.

Publish: `dotnet publish -c Release -r linux-x64 --self-contained false` · `-p:PublishAot=true` for NativeAOT (needs clang) ·
`-r win-x64` / `-r osx-arm64` / `-r osx-x64` cross-publish (verified to compile and publish from Linux; never run).

## Reproducing the evidence (headless Linux)

```sh
tools/capture.sh <dir> [scale]        # Xvfb + xcompmgr -n (needed for per-pixel alpha) + ImageMagick: rest / office / office-chat screenshots
tools/capture_anim.sh out.gif          # opening animation + working bots
dotnet bin/Debug/net9.0/HoverAvalonia.dll --software-gl --selftest <dir> --backdrop --bots 4   # drives the real pointer/keyboard via XWarpPointer + XTest, writes report.json
bench/run_compare.sh rust|cs-fd|cs-aot bench/compare.hms <out-prefix> 3    # tools/hover-measure + cpu_sampler.py; memory = USS, same tool for all three
bench/cs-builds.sh Debug|Release 3     # build timings
python3 bench/summarize.py <runs-prefix>...
```
`bench/compare.hms` and `bench/cycles.hms` are played by the *existing* `tools/hover-measure` against either app: the prototype implements Hover's
`HOVER_BENCH` stdin/stdout protocol (`Bench.cs`), so the scripts are identical. (`xcompmgr` was built from source; it is not in the distro repos.)

## Layout

```
prototype/avalonia/HoverAvalonia/
  Core/       Rng, M (V3/M4/Rgb), NotchGeometry          — ports of js.rs, m.rs, hover-notch
  Office/     Scene, Bot, OfficeScene, Sim, Textures, Shaders (GLSL), GlRenderer, OfficeControl
  Platform/   IPlat, X11Plat, Win32Plat, MacPlat
  Ui/         NotchWindow(.axaml), ChatPanel(.axaml), Controls (Ring, TagLayer)
  SelfTest.cs, Bench.cs, Program.cs
prototype/bench/   scripts and scenarios        prototype/tools/   capture scripts        prototype/results/   raw numbers and images
```
