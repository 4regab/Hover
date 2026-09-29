# Phase 4 report: validation, packages, cutover

**Status: everything that can run on Linux has run. The gates G1–G4 compare against the
C# app, which can't run here, so they stay pending until the Windows run.** The C#
app, `src/`, `web/` and `tests/` are unchanged. Nothing has been removed.

## Benchmark on Linux (`bench-linux.json`)

`python3 port/bench/measure-hover.py --runs 5 --out port/phase4/bench-linux.json`: the
BENCHMARK.md scenarios, driven through `HOVER_BENCH`'s stdin channel (the shortcut's and
the history panel's own paths), with `fake-acp.py` as the agent and `hover-data`'s
200-turn history. The machine is a sandbox VM with **software GPU (lavapipe)**, so the
office's frame times and CPU are the CPU rendering the scene, not a GPU.

**Only 3 of the 5 runs were taken.** Runs 4 and 5 were cut off by the session, and
BENCHMARK.md asks for 5, so these are indicative, not the gate numbers. The spread
(max − min over the median) is under 5 % for most metrics. The exceptions: S2 commit
7.6 %, S4 open 5.1 %, S6 growth 10 %, S6 median reopen 20 % (99–123 ms).

| Scenario | Metric | Median (3 runs) |
|---|---|---|
| S1 cold start | startup to notch shown | 189 ms |
| S1 | private WS / commit | 92 / 47 MB |
| S2 settled collapsed (after the 30 s drop) | private WS / commit | 180 / 124 MB |
| 60 s settled | CPU / presents | 0.3 s / 0 per s |
| S3 office visible | reopen (to first frame) | 692 ms |
| S3 | private WS / commit | 378 / 296 MB |
| S3 | frame time p50 / p95 | 105 / 117 ms |
| office idle 60 s | CPU / frames | 180 s / 9.3 per s |
| S4 200-turn session | open | 270 ms |
| S4 | private WS / commit | 409 / 323 MB |
| S5 3 streaming sessions | private WS; agents | 409 MB; 4.7 MB |
| S5 | frame time p95 | 88 ms |
| S6 20 reopens | median / max | 121 / 280 ms |
| S6 | growth vs S3 | +50 MB |

What this says, before the Windows run:
- **Resting costs are small**: a 92 MB start, 0.3 s of CPU a minute, no presents.
- **S2 went from 319 to 180 MB** once the drop released the last frame, the blur and
  glibc's freed arenas (`malloc_trim`). The remaining ~90 MB over S1 is mostly heap that
  Slint's and the chat's caches keep. On Windows the GPU memory is separate (G4).
- **The office's frame costs are lavapipe's.** 117 ms p95 and 3 cores at 9 fps are
  software rasterisation of a 1536² shadow map plus the scene. This number means
  nothing for Windows: it has to be measured on a real GPU (RUN-ON-WINDOWS).
- **S6 growth (+50 MB)** is not explained yet. The likely cause is heap kept after
  each reopen re-creates the office thread and its device. It needs a longer loop to
  see whether it plateaus (question 9).

G1–G4: **pending**. `Measure-Hover.ps1` must give the C# medians on the same Windows PC.

## Accessibility

- **Linux (AT-SPI), tested:** `atspi-dump.py` under Xvfb with the at-spi2 bus and
  pyatspi (`Atspi` through GObject). The office's HUD reaches AT-SPI with Slint's
  accessible ids (`atspi-office.txt`): `HoverNotch`, `brand`, `timeAuto`,
  `timeNight`, `timeDay`, `beats`, `histBtn`, `setBtn`, `closeBtn`, `fabMain`, with
  roles and names. One snag: the app didn't quit on the bench `quit` while the a11y bus
  was up, and the run was stopped by its timeout. That needs a look (it quits normally
  without the a11y bus).
- **SCREENS.md** lists only `HoverNotch` of these, because the C# office is a web page
  that UIA can't see into. The native office is visible to UIA and AT-SPI, which is
  better than before.
- **Windows (UIA): pending.** Accessibility Insights on the notch, the office and Settings.

## Packages

| Package | Where | State |
|---|---|---|
| `.deb` and `.tar.gz` | `native/installer/package-linux.sh` | Built (19 MB each). Smoke test: unpacked into a fresh root, `usr/bin/hover --selftest` under Xvfb passed **13/13**. The binary carries its fonts, icon and music. |
| Windows installer | `native/installer/Hover.iss` | Written from `installer/Hover.iss` with the same AppId, folder, `Hover.exe` name and Run value, so it installs over a C# install in place. **Build pending** (Windows, Inno Setup). |
| AppImage | not made | Needs `appimagetool` and its runtime, downloaded. The `.deb` and tarball cover the need for now (question 10). |

## Cutover checklist

### 1. What still needs the C# app (do before .NET goes)

- [ ] `port/bench/Measure-Hover.ps1`, 5 runs, S1–S6, on the Windows test PC: the C# medians that G1–G4 compare against.
- [ ] WebView2 captures of every SCREENS.md view (`capture-office.mjs`, scrollbars shown), for the §4 comparison on Windows.
- [ ] C#↔Rust round trips: HoverFixture writes a data folder that `hover-data dump` reads, and `hover-data write`'s folder opens in the C# app (settings, `note.key` through DPAPI, the sealed history).
- [ ] Recorded FakeAcp sessions again, if the scenarios change (the replays in `native/golden/acp` don't need .NET).

### 2. What must pass on Windows (all of RUN-ON-WINDOWS.md, phases 1, 3 and 4)

- [ ] `cargo test --release --workspace` on Windows.
- [ ] The notch self-test in the product; DPI 100–200 %; a second monitor.
- [ ] G1–G4 against the C# medians: `Measure-Hover.ps1 -Impl native -Exe native\target\release\hover.exe`.
- [ ] The office on DX12: the scene ΔE on Windows, frame times, GPU memory.
- [ ] Tray, hotkey rebinding and its conflict box, pickers, WASAPI music, the four quotas with real sign-ins, DPAPI key shared with C#.
- [ ] UIA ids with Accessibility Insights.
- [ ] `native/installer/Hover.iss` built; install over a C# install; uninstall; launch at login.
- [ ] G5: every FEATURES.md row and SCREENS.md view, or its difference accepted.

### 3. What passed on Linux

- [x] 151 tests; the Windows target checks.
- [x] The notch self-test 13/13 under Xvfb, under Xwayland in weston, and from the `.deb`.
- [x] Persistence, sessions (FakeAcp replays and `fake-acp.py` live), quotas, palette, Settings, tray (SNI), notifications, music.
- [x] The office scene: mean ΔE ≤ 2.0 in all three views (the over-10 share fails: question 7).
- [x] The benchmark scenarios run end to end (3 of 5 runs; absolute numbers only).
- [x] AT-SPI exposes the office's HUD with its ids.

### 4. What the removal takes out (only after 1 and 2, and your go-ahead)

- `src/` (the WPF app), `tests/` (NUnit and E2E), `port/tools/` (FakeAcp, HoverFixture), `Hover.slnx`.
- The .NET parts of `build.ps1` (build, test, publish) and the root `installer/Hover.iss` (replaced by `native/installer/Hover.iss`).
- `web/office/` only by question 8. `capture-scene.mjs` and `capture-office.mjs` read `src/Hover/Assets/kiro-office.html`, so the goldens need that page to exist somewhere.
- AGENTS.md rewritten for the native app.

## Questions (answer like `8A 9A 10B`)

8. **The page's JS after cutover** (for regenerating goldens).
   - **A.** Keep a copy of `web/office` and the built `kiro-office.html` under `native/golden/page/`, and point the capture scripts there.
   - **B.** Keep only the PNG goldens. Nothing can regenerate them, so a scene change means new goldens from the native render.
   - **C.** Keep `web/office/` where it is.
9. **S6 memory growth (+50 MB over 20 reopens).**
   - **A.** Run a 100-reopen loop now and cap the growth if it doesn't plateau (half a day).
   - **B.** Wait for the Windows numbers, where the device's memory is on the GPU.
10. **An AppImage as well as the `.deb` and tarball?**
    - **A.** Yes. This means downloading `appimagetool` in the build (about 2 hours).
    - **B.** No, not for now.
