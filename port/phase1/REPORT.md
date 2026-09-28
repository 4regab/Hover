# Phase 1 report: can Rust + Slint + wgpu carry the notch and the chat?

**Status: prototypes built, portable gates checked on Linux. Every Windows gate is
pending.** Nothing here has run on a Windows desktop: the dev VM is Linux, and GitHub
Actions are not available (the `native` workflow is now manual-only and has never
finished a run). The steps to settle the pending gates are in `RUN-ON-WINDOWS.md`.
This is a feasibility prototype, not a migration. The C# app is unchanged and remains
the product.

## Gate summary

| Gate | Status | Evidence |
|---|---|---|
| Phase 0 baseline snapshot, checklists, frozen benchmark | done | `port/phase0/*.md` |
| Phase 0 baseline screenshots | done in Chromium; the WebView2/Edge captures on Windows are **pending** | `port/phase0/baseline/` (21 views) |
| Phase 0 baseline measurements (S1–S6) | **pending** (needs a Windows desktop) | `port/bench/Measure-Hover.ps1` is ready; never run |
| 1A notch: transparency, click-through, non-activating, focus, DPI, monitors, flicker, idle redraw | **pending**. The code is written and type-checks for `x86_64-pc-windows-msvc`. | `apps/notch-proto --selftest` automates 13 checks |
| 1A notch logic (geometry, animation, hover rules) | pass | `hover-notch`: 5 tests against Notch.cs constants |
| 1B Markdown and diagrams as the page renders them | pass | byte-identical to md.js and diagram.js (below) |
| 1B selection and copy | pass (Linux) | copy identical to Chromium's on 605 cases (below) |
| 1B layout, step lists, streaming, long threads | pass (Linux) | tests and headless timings (below) |
| 1B IME, UI Automation, clipboard, links on Windows | **pending** | manual checks in `RUN-ON-WINDOWS.md` |
| Visual parity (BENCHMARK.md §4) | **not evaluated** | needs Windows captures of both builds |

## 1B: rich chat

Each concern has its own implementation. Slint's StyledText is used for none of them.

| Concern | Implementation | Result |
|---|---|---|
| Parsing | `hover-md`: md.js ported line for line, writing the same HTML, then read back into blocks | byte-identical on 7 fixtures and 3 956 random documents |
| Diagrams | `hover-diagram`: diagram.js ported; the same SVG, rasterised by resvg with page.html's `.flow` CSS | byte-identical on 8 cases |
| Images in answers | `imageFor` ported (web, and the session folder only) | 12 path cases identical |
| Layout | page.html's box model by hand (margin collapsing, lists, tables in auto layout, quotes, code, step lists as flex rows); text by parley | step rows within 0.01 px of the page |
| Selection and copy | parley cursors across every text box; a copy serialiser fitted to Chromium's | identical on 5 fixture sessions and 600 random answers from the real page (`golden/gen-copy.mjs`) |
| Painting | tiny-skia plus swash glyph masks, viewport only, with glyph, image and diagram caches | see the timings |
| Chrome and composer | Slint: header, `TextInput` composer (Enter, Shift+Enter), send/queue/stop, Ctrl+C, Ctrl+A | headless screenshots in `shots/` |
| Accessibility | one Slint text node per visible text box, over the painted thread | the nodes are there; not yet checked with a UIA client |

Headless timings. These come from the Linux VM on the software renderer, so they can
show where a problem is but cannot pass G2:

| | |
|---|---|
| 200 rich turns: layout and first frame | 204 ms |
| Scroll frame (thread paint plus Slint frame of the whole window) | 1.7 ms mean, 6.3 ms worst |
| A streamed chunk into a 200-turn thread | 1.9 ms (only that turn is laid out again) |
| Resident memory with 200 rich turns | 48 MB (Linux RSS; not comparable to Windows private WS) |

Findings about the baseline that change what parity means:

1. **The office never uses Inter.** The page's font stack asks for Inter first, but
   Inter is loaded only into WPF, not into WebView2. The office's text is therefore
   Segoe UI Variable Text on Windows 11 and Segoe UI on Windows 10. The native chat
   resolves the same stack against system fonts, so it lands on the same face.
2. **md.js hangs** on a list-item line containing U+2028/U+2029, and **throws** on a stray
   `\0n\0`. The port does neither (MARKDOWN.md). **Decision needed.**
3. **The open/closed state of a step list follows the turn index, not the session.**
   Switching straight from one chat to another carries turn *i*'s choice over. The port
   keeps this (`Thread::steps_user`) for 1:1 behaviour. **Decision needed:** keep or fix.
4. **Copy details are Chromium's.** A rule settles pending newlines. A paragraph that is
   only an image copies as blank lines. A table at the end of a selection adds a trailing
   newline. A diagram's labels are part of the copy.

Chat gaps that remain (none are known blockers):
- Code blocks and tables are clipped at the drawer's edge; horizontal scrolling is not
  done yet.
- The thread has no scrollbar (`scrollbar-width: thin`) and no smooth wheel scrolling.
- The glass blur (`backdrop-filter: blur(18px) saturate(1.4)`) is not drawn: the drawer is
  flat. This needs the office scene behind it (Phase 2).
- Web images are not fetched: the prototype's loader returns nothing. The drawing and
  sizing paths are done.
- Not in the prototype: pasted, dropped and picked images in the composer; the model
  menu; the transcript view; the fresh-answer fade; the drawer's slide.
- The selection colour is a guess (`theme::SELECTION`) until it is compared with WebView2.
- Accessibility: the text nodes don't expose the text selection to UIA.
- Only single clicks. Double-click word selection and triple-click line selection are not done.

## 1A: notch

The design, as written (`apps/notch-proto`):
- **Rendering:** Slint's FemtoVG renderer on wgpu 30, DX12 only. The swapchain is built
  from a DirectComposition visual (`Dx12SwapchainKind::DxgiFromVisual`) on a window
  created with `WS_EX_NOREDIRECTIONBITMAP`. That is the only DXGI path that keeps
  per-pixel alpha; an HWND swapchain shows black where the notch is empty.
- **Office stand-in:** a wgpu texture rendered on Slint's own device and shown as a
  Slint image, with no CPU readback.
- **Click-through:** a DirectComposition window has no per-pixel hit testing, so the
  50 ms poll (which Hover already runs) sets `WS_EX_TRANSPARENT` whenever the pointer is
  off the shape or its shadow. `--hit transparent` runs the same test without
  `WS_EX_LAYERED`, for comparison.
- **Styles and focus:** `WS_EX_TOOLWINDOW` always, `WS_EX_NOACTIVATE` while resting, the
  previous foreground window restored on collapse, the Alt+N hotkey, and click-away
  through `WM_ACTIVATE`.
- **Placement:** the primary monitor's work area and its DPI, re-checked every 2 s.
- **Versions** are pinned at `slint =1.18.1` and `wgpu 30`. The unstable APIs
  (`unstable-wgpu-30`, `unstable-winit-030`) are used only in `notch-proto/src/main.rs`.

Risks. These decide the gate, and the self-test is built around them:
1. `WS_EX_LAYERED` together with a DirectComposition swapchain may break composition. If
   it does, the `transparent` variant must pass instead.
2. Toggling click-through from a 50 ms poll can drop a click that lands in the first
   ≤50 ms after the pointer enters the shape. The C# window has per-pixel hit testing and
   no such delay. If that shows, the fallback is `WM_NCHITTEST` returning `HTTRANSPARENT`,
   which doesn't pass clicks to other processes. The last resort is a small companion
   input window.
3. The ears (≤10 px) cast no shadow in Slint: a small visual difference to measure.
4. WARP-only machines, and DirectComposition behaviour under RDP.

Not in the notch prototype:
- the greeting animation;
- quota rings and the done state in the pill;
- the alert;
- the dashboard window;
- rebinding the shortcut;
- the light theme.

## What the complete port still costs

These are estimates for the reviewer. Nothing below has been started.

| Area | Main work |
|---|---|
| Office scene (P2) | the three.js scene in wgpu: voxel room, PBR materials with ACES, PCF soft shadows, sprites and particles, canvas textures, bots and poses, camera, picking, tags |
| Sessions (P3A) | AcpHost, KiroStream, KiroSession(s), the job object, provider adapters, with FakeAcp recordings as tests |
| Persistence (P3B) | AES-GCM framing, DPAPI (`note.key`), System.Text.Json shapes, round-trips C#→Rust and Rust→C# |
| Product (P3C) | Settings (5 sections), quotas (4 readers incl. SQLite), palette and VS Code themes, music, tray, dashboard, single instance |
| Validation (P4) | the full benchmark, visual parity per SCREENS.md, UIA mapping, installer |

## Next bounded task

1. On a real Windows 10/11 PC, run `notch-proto --selftest` in both hit modes, and the
   `chat-proto` manual checks in RUN-ON-WINDOWS.md.
2. Record the Phase 0 C# baseline with `Measure-Hover.ps1` on the same PC.
3. Settle the three decisions above (md.js hang and throw; the step-list state leak).

Building the prototypes needs the Rust toolchain on that PC (`cargo build --release -p
notch-proto -p chat-proto` in `native/`), because no CI artifacts exist.

Until those results are reviewed, the candidate is not expanded into Phase 2. The only
work continuing is portable Phase 1 work: the chat gaps above.
