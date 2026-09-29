# Phase 2 report: the Agent office, native

**Status: built and tested on Linux. The scene passes its mean colour-difference limit
but fails the share of pixels over ΔE 10 (aliased edges); that needs your decision
(question 7). Nothing has run on Windows yet, and the C# comparisons are pending.**
The C# app is unchanged and is still the product.

## What was built

| Page (web/office) | Rust | What it does |
|---|---|---|
| `main.js` room, props, lights (O1, O3, O5) | `hover-office/src/scene.rs` | The 14×11 voxel room, built as main.js builds it: walls, door, window with its light patch and beam, TV, coffee counter with steam, bookcase, board, clock, painting, lounge, beanbags, plants, six desks with chairs and lamps, dust, the vacuum loop. |
| three.js 0.170 shading (O2, O3) | `office.wgsl`, `render.rs` | `MeshStandardMaterial` ported from three's shader chunks: physical lights, GGX, multiscattering, hemisphere/sun/fill/point lights, PCF-soft shadows in WebGL's texel grid (1536², bias -0.0004, normalBias 0.03), ACES filmic with the time of day's exposure, sRGB output. Additive sprites blend on the encoded colour, as WebGL does. wgpu 30: DX12 on Windows, Vulkan/GL on Linux. |
| shadow redraw rule (O4) | `office.rs` | The shadow map redraws only when something moved (walks: every frame; else ≤ 10 Hz; at once on a light change). |
| `drawSky/drawTV/drawBoard/drawClock` (O6) | `canvas.rs` | The four canvases in tiny-skia with the page's fonts (Pixelify Sans, Inter): sky, TV stats at 4 Hz, the kanban board with its empty state, the LED clock at 2 Hz with the blinking colon. |
| `setTime` (O7) | `office.rs`, `office_ui.rs` | Auto (7–19 is day), night, day; kept as `time`; checked every 60 s. |
| `class Bot` (O8) | `bot.rs` | Six named and coloured bots; walking in and out through the door, sitting, the poses per stage and act, blinking, the bulb's colour and halo, the desk screen's glow. |
| camera (O9) | `office.rs`, `live.rs` | Orthographic iso; drag to pan past 6 px; wheel zoom about the pointer (0.85–2.8); double-click and `0` reset; `+`/`-`; clamp; eased follow (k = 5); focus on the open bot, board, TV or history; `office.view` kept across drops. |
| `pick()` (O10) | `office.rs` | Picks bots and props (TV, board, clock, window, door, bookshelf); hover highlight; tooltip (the clock's is the full date). |
| tags and bubbles (O11) | `office.rs` → `office.slint` | Tags over the heads with the text typed out at 45 characters a second, stage colours, the tool badge, click to open, the hot state. |
| frame pacing (O12, O13) | `office.rs`, `live.rs` | 30 fps while lively, 10 idle, 1 with animations off (the app's reduced motion); nothing while hidden. |
| `page.html` UI (O14–O17) | `office.slint`, `office_ui.rs` | Board, overview and history panels; toast (2.8 s); the delete confirmation; the HUD (brand, time segment, beats, history, settings, close); the new-task circle (rest → pick with tool logos greyed with the reason → box with prompt, folder chip, send; a draft dot; Esc, chevron or a click folds it); the drawer with hover-chat's thread. |
| `backdrop-filter` | `office_ui.rs` (`blur`), `Backdrop` in `office.slint` | Glass behind the drawer, menu and toast: a CPU-blurred quarter-size copy of the frame. |
| `KiroPage` lifecycle | `office_ui.rs`, `live.rs` | The office runs on its own thread with its own wgpu device. It is made when shown, sends frames only when its pacing wants one, and is dropped 30 s after it is hidden (its GPU memory, the last frame and the blur go with it). When it is made again it reopens the open chat and restores the camera. |

## Evidence

- `cargo test --release --workspace`: 151 passed (hover-office's own tests cover the
  scene, poses, pacing, picking, the camera and the state model).
  `cargo check --target x86_64-pc-windows-msvc`: green.
- **Scene against the page's own render.** `capture-scene.mjs` renders
  `src/Hover/Assets/kiro-office.html` in Chromium (three.js 0.170, the page's fixed
  seed and time), `examples/shot` renders the same state natively, `compare.py`
  measures CIEDE2000 (alignment offset found: exactly zero):

  | View | Mean ΔE (limit 2.0) | Pixels over ΔE 10 (limit 1.0 %) |
  |---|---|---|
  | Night | 0.89 ✅ | 1.70 % ❌ |
  | Day | 1.17 ✅ | 3.00 % ❌ |
  | Empty | 0.83 ✅ | 1.56 % ❌ |

  Where the over-10 pixels are, for night: 1.17 % on geometry edges (both draw without
  antialiasing, but the rasterisers pick different pixels along a slope) and 0.53 % in
  flat areas, mostly the canvas text on the TV and board (tiny-skia's and Chromium's
  glyph rasterisers). Day is worse because the sun makes the edge contrast higher.
  Pictures: `shots/page-scene-*.png`, `shots/native-scene-*.png`, `shots/diff-scene-*.png`.
- **The UI:** `shots/notch-open-office.png`, `office-fab-open.png`, `office-fab-pick.png`,
  `office-drawer.png`, `office-panel-board.png`, `office-panel-history.png`,
  `office-panel-tv.png`, `office-toast.png`.
- **Benchmark** (Linux absolute numbers; the C# side is pending): see `../phase4/REPORT.md`.

## Differences from the C# app

18. **Glass blur** is a blurred copy of the office frame, not a live blur of everything
    behind: the office is the only thing behind those surfaces, so it looks the same.
19. **Antialiasing:** neither draws with it; see the ΔE result above.

## Gaps (not yet done)

- The drawer lacks chat-proto's selection and copy, images, the model pill and the step toggles.
- The new-task box has no image attach.
- The bot glow uses sprites only (the page adds a halo on the bot's own light as well).
- A bot that leaves is hidden, but its nodes stay in the scene graph until the page is dropped.

About 2 days for all four.

## Question (answer like `7A`)

7. **The scene's over-ΔE-10 share** (1.6–3.0 %, limit 1.0 %).
   - **A.** Accept it as a difference: the mean passes everywhere, and the excess is edge pixels and canvas glyphs.
   - **B.** Add 4× MSAA to the native scene. This makes the native edges smoother than the page's (whose canvas has no antialiasing), so the ΔE may not improve. About half a day, plus GPU time.
   - **C.** Render the canvas text with a glyph rasteriser closer to Chromium's (Skia's own through `skia-safe`). This is a new, large dependency. About 1 day.

## Pending on Windows

See `../phase4/RUN-ON-WINDOWS.md`: the office on DX12 (adapter, frame times,
GPU memory for G4), the scene comparison on Windows, WebView2 captures of the UI views.
