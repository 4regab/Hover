# Benchmark procedure and decision thresholds (frozen)

**Frozen on 2026-09-28, before any candidate measurement.** Any change to this file
after a candidate has been measured must be listed in §6 with the reason, and it may not
make a gate easier to pass. A gate that fails is reported as failed.

The script is `port/bench/Measure-Hover.ps1`. Fixtures come from `port/tools/HoverFixture`
(sealed history written with the machine's own DPAPI key) and `port/tools/FakeAcp`
(a stand-in ACP agent that replays recorded provider events).

## 1. Conditions

- **Machine**: a real Windows 10/11 desktop with a GPU. Record OS build, CPU, RAM, GPU,
  driver version, display count, resolution and scale (`env.json` does this).
  CI runners (no GPU, WARP) give *indicative* numbers only.
- **Build**: release builds. C#: `build.ps1 publish` (single-file, R2R). Native:
  `cargo build --release` with the profile in `native/Cargo.toml`.
- **State**: a fresh `HOVER_DATA_DIR` per run, filled from the fixture. The same
  `settings.json`: KiroNoticeSeen = true, the KiroFolder fixture, and no quota items on
  the notch, so no quota readers run. Kiro, Codex and Cursor on PATH are replaced by the
  fake agent. Office size Default. Night time fixed (`office.time = night`), camera at
  its default, music off, and the reduced-motion OS setting off.
- No other Hover running; the machine idle (no Windows Update, and the rest noted in `env.json`).
- 5 runs of each scenario per implementation, in interleaved order (C, N, C, N…).

## 2. Scenarios

| Id | Scenario | Steps | Sample |
|---|---|---|---|
| S1 | Cold startup | launch the exe | time until the `Hover notch` window exists and is visible; memory 10 s later |
| S2 | Settled collapsed | S1, then open the office (Alt+N), wait 5 s, collapse (Esc), wait **45 s** (the WebView2 30 s drop timer plus 15 s) | 10 s of 1 Hz samples |
| S3 | Office visible | Alt+N, wait 10 s | 10 s of 1 Hz samples; frame times over the same 10 s |
| S4 | Office + long rich conversation | the history fixture holds one session of 200 turns (every block type in `rich.md`); open it from the history panel; scroll to the top and back | 10 s after it settles; the time to open it |
| S5 | Active and concurrent sessions | start 3 sessions with the fake agent streaming updates (30 s runs, 20 updates/s), and open one of them | during the runs; then 10 s after all have ended |
| S6 | Repeat | 20 × (collapse, 2 s, Alt+N, 2 s); then 10 × (start a session, stop it after 3 s) | reopen latency each time; memory after the loop, compared with the S3 sample |

## 3. Metrics and attribution

Per process:
- private working set (`WorkingSetPrivate`);
- private committed memory (`PrivateMemorySize64`);
- handles;
- CPU time delta;
- GPU dedicated and shared memory (`\GPU Process Memory(pid_*)\*`).

Summed per group:
- **Application**: the Hover process plus the processes it owns for drawing: every
  `msedgewebview2.exe` in its tree (browser, GPU, renderer, utility). The native build has
  no such helpers; any process it spawns for drawing would count here.
- **Providers**: every other process in Hover's tree (kiro-cli, codex-acp, cursor-agent,
  node, cmd, conhost, MCP servers). Reported separately, never added to Application.

Only private memory is summed. Shared or total working sets are recorded per process
but never summed as if they were unique RAM.

Timings:
- **startup**: process start → notch window visible.
- **reopen**: Alt+N → first presented frame of the office. C#: the page's first rAF after
  `visible` (read through DevTools); native: the renderer's present callback.
- **input response**: key or pointer event → the frame that shows it (instrumented in both).
- **frame time**: presented-frame intervals while lively (p50, p95, p99).
- **idle activity**: CPU time and presents per second over 60 s settled collapsed, and
  over 60 s office-visible-idle.

## 4. Visual tolerances (set before any comparison)

Comparisons use matching Windows, DPI, fonts, viewport, content, camera pose and a fixed
animation time. The native build takes `--freeze-time <s>`; the page uses the same clock
override injected through DevTools.

- **Region metrics, not one global score.** Each view defines regions: notch outline and
  shadow, HUD, name tags, drawer header, thread text, code blocks, diagram, composer,
  office scene (floor/walls, bots, lights). For each region:
  - scene regions (3D): mean CIEDE2000 ΔE ≤ 2.0, and ≤ 1.0 % of pixels with ΔE > 10;
  - UI and text regions: ≤ 0.5 % of pixels with ΔE > 15 after a 1 px dilation of the
    other image. Text line boxes (row-projection profile) and element edges within 1
    device px. Glyph advances drift ≤ 1 px per 100 px of line;
  - shadows and glows: mean ΔE ≤ 3.0 over the region. Peak alpha within 8/255.
- **Text readability** is judged separately at 100 % and 200 % scale with side-by-side
  crops.
- **Animation**: sampled at t = 0, 25, 50, 75, 100 % of each transition. Every sample
  meets the region metrics, and durations are within one frame (16.7 ms).
- **Interaction** scripts are run separately; matching pixels never pass an interaction.
- A whole-screen difference is reported for information only and never decides.
- Any region that fails is listed with overlay and difference images. Only the user can
  accept a difference; the implementer never approves one.

## 5. Decision thresholds

Medians of the 5 runs. The **noise band** `b` for a metric is the larger of the two
implementations' `(max − min) / median`, with a minimum of 3 %.

| Gate | Pass when |
|---|---|
| G1 memory | Application private WS **and** private commit ≤ 0.75 × C# in **S2** and in **S4** (four comparisons, all must pass) |
| G2 speed | startup, reopen, input response and frame time p95: native ≤ C# × (1 + b) |
| G3 idle | CPU time and presents/s over 60 s settled and office-idle: native ≤ C# × (1 + b) |
| G4 GPU | GPU dedicated + shared memory in S3/S4: native ≤ max(C# × 1.10, C# + 8 MB) |
| G5 features | every FEATURES.md row implemented; every SCREENS.md view passing §4 and its interaction script, or a difference accepted by the user |

G1–G4 on a prototype hold only for what the prototype contains. Costs it is missing
(sessions, persistence, quotas, settings, audio) are listed with the result, and the gate
is repeated on the complete port.

## 6. Changes after freezing

None.
