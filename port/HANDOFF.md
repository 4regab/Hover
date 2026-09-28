# Handoff: Hover native port (Rust + Slint + wgpu)

Read this first. Then `port/README.md` (the map) and `port/phase1/REPORT.md` (the
evidence, findings and what still differs).

## Rules the user set (must follow)

- **No GitHub Actions, ever.** The user has no Actions quota. Both workflows were removed
  from this branch at the user's request, so a push starts nothing. Never add a workflow
  back, and never trigger, re-run or wait on one. If a run shows up anyway, cancel it at
  once: `gh api -X POST repos/4regab/Hover/actions/runs/<id>/cancel` (list with
  `gh api "repos/4regab/Hover/actions/runs?branch=rust-port/phase-0-1&per_page=5"`).
  `main` still has `ci.yml` and `native.yml`; merging PR #5 as it is removes them there too.
- **Pushing to `rust-port/phase-0-1` is allowed**, with no need to ask each time.
- **Never open an image over 5 MB or over 2000 px on a side.** Check with `file` first;
  crop or downscale a copy.
- **How to report:** after each feature, a short summary with screenshots (put them in
  `port/phase1/shots/`, push, and link them). When a decision is needed, give the user
  multiple-choice questions (they answer like `1A 2B`).
- The user's brief governs the work:
  - Phases 0 to 4, each one gated; 1:1 UI/UX parity with the current app.
  - No WebView2 or Chromium fallback unless measured and presented as a separate candidate.
  - Nothing replaces the C# app until the gates pass and the cutover is reviewed.
  - **No Windows PC is available soon.** The user asked to carry on through all phases on
    Linux. Windows gates stay marked **pending**, never passed.
- Repo rules (`AGENTS.md`): CRLF line endings; comments say why, not what; surgical
  changes; no new dependency without a stated reason. No file under `src/`, `web/` or
  `tests/` has been changed; keep it that way (the C# app is still the product).

## Decisions taken (2026-09-28)

1. md.js hangs on U+2028/U+2029 in list lines and throws on `\0n\0`: the port does neither.
2. Step-list open/closed state: per session in the port (the baseline's leak is fixed).
3. Carry on through all phases on Linux; Windows gates stay pending.
4. **Open, waiting on the user:** what to start next. The recommendation given was
   Phase 3A (sessions, tested with `port/tools/FakeAcp`) and 3B (persistence without
   DPAPI) before Phase 2 (the office scene). Ask with multiple-choice questions if the
   user hasn't said.

## Where things are

- Repo `4regab/Hover`, branch `rust-port/phase-0-1`, draft PR #5
  (https://github.com/4regab/Hover/pull/5). Last commit at handoff: `9e9814d` (then this note).

| Path | Contents |
|---|---|
| `port/phase0/` | BASELINE, FEATURES, SCREENS, MARKDOWN, BENCHMARK (**frozen**; don't loosen), `baseline/*.png` (21 Chromium captures, taken with hidden scrollbars: REPORT finding 5) |
| `port/phase1/` | REPORT.md, RUN-ON-WINDOWS.md (every manual Windows check), `shots/` |
| `port/bench/` | `Measure-Hover.ps1` (S1–S6), `capture-office.mjs` (page captures, now with scrollbars shown; `BROWSER_CHANNEL=msedge` on Windows) |
| `port/tools/` | `FakeAcp` (stand-in ACP agent), `HoverFixture` (a sealed history written with Hover's own code) |
| `native/crates/hover-md` | md.js ported, byte-identical HTML; `blocks.rs` reads it into blocks; `image.rs` is `imageFor` |
| `native/crates/hover-diagram` | diagram.js ported, byte-identical SVG |
| `native/crates/hover-chat` | `doc.rs` layout, selection, copy, word/paragraph units, sideways scrollers, fresh fade; `paint.rs` painter; `scroll.rs` scrollbars and the smooth-scroll curve; `images.rs` the shared image cache; `state.rs` state JSON to turns; `assets/` Chromium's broken-image icon |
| `native/crates/hover-notch` | notch geometry, animation, hover rules, zones (from Notch.cs) |
| `native/apps/chat-proto` | the Slint drawer: `main.rs` (window, pointer, scrollbars, clicks, headless modes), `net.rs` (image loading), `compose.rs` (composer images, SaveImages, file picker), `models.rs` (model pill and menu), `ui/chat.slint` |
| `native/apps/notch-proto` | Slint + wgpu 30 DX12 notch; Win32 in `src/win.rs`; `--selftest <dir>` |
| `native/golden/` | generators from the real page and JS (below), `fixtures/`, `expected/` |

The specs once gathered from the C# source (`/projects/sandbox/work/*.txt`) are gone.
Re-derive what you need from `src/Hover/**` (a `context-gatherer` sub-agent works well).

## Setting up a new sandbox

```sh
dnf install -y dejavu-sans-fonts dejavu-sans-mono-fonts dejavu-serif-fonts   # fonts the tests need
rustup target add x86_64-pc-windows-msvc                                     # Windows type-check
# Playwright, outside the repo, for the page goldens:
mkdir -p /projects/sandbox/pw && cd /projects/sandbox/pw && npm init -y && npm i playwright
npx playwright install chromium        # not --with-deps (no apt here)
ln -s /projects/sandbox/pw/node_modules /projects/sandbox/node_modules
```

## Commands

```sh
cd native
cargo test --release --workspace      # 34 tests: goldens, copy, clicks, scroll, images, fade, notch
cargo check --release --target x86_64-pc-windows-msvc -p notch-proto -p chat-proto
./target/release/chat-proto --bench   # headless timings for REPORT.md
node golden/gen.mjs                   # md/diagram goldens: must leave git clean
node golden/gen-copy.mjs              # copy corpus, ~1 min
node golden/gen-words.mjs             # double/triple click on every character, ~6 min
node golden/gen-scroll.mjs            # box sizes with scrollbars shown
node golden/gen-broken.mjs            # broken-image layout
```

`chat-proto` headless screenshots (software renderer):
`--screenshot out.png [--scale 2] [--session N] [--turns N] [--select] [--top]
[--hscroll 80] [--images] [--answer file.md] [--pics N] [--menu] [--history] [--closed]`.
The window itself also takes `--stream`, `--history`, `--answer`.

## Things learned the hard way

- Editing files: keep CRLF. A small Python helper that reads with `newline=''`,
  replaces exact strings and writes back is the reliable way; new files need
  `sed -i 's/\r\?$/\r/'`.
- Playwright hides scrollbars unless `ignoreDefaultArgs: ['--hide-scrollbars']`, and with
  `page.clock.install` a DOM click is safer than `mouse.click` waits.
- Chromium on Linux uses Unix editing behaviour; WebView2 also takes the spaces after a
  double-clicked word (applied in the port, checked by its own test).
- Slint's software renderer doesn't clip to rounded corners, and drops an element that
  has both `clip` and a drop shadow. Round images in their pixels; put shadows on a
  separate element.
- winit gives no pointer position during a file drag.
- `ureq` with rustls needs `ring`, which won't cross-compile for MSVC here; Windows uses
  native-tls (schannel) instead (see `chat-proto/Cargo.toml`).
- Replacing Slint's accessible node list each frame is costly; `frame()` only replaces it
  when it changed.

## State at handoff

**Phase 0:** docs, frozen benchmark, Chromium captures. The C# baseline measurements and
WebView2 captures are **pending** (Windows).

**Phase 1, built and tested on Linux:**
- md.js and diagram.js byte-identical (9 fixtures, 3 956 random documents, 8 diagrams).
- Copy identical to Chromium's on 605 cases; double/triple click on 3 602 of 3 606 page
  clicks (4 emoji cases differ).
- Thin scrollbars (thread, and sideways in code blocks), smooth scrolling on cc's curve.
- Images from the web, `hover.images` and the session's files host; broken-image boxes
  fitted on the page.
- Composer images (paste, drop, pick, SaveImages), model pill and menu, toast, transcript
  view, fresh fade, drawer slide, step lists, prompt thumbnails, selection exposed to UIA.
- Headless: 200 rich turns open in 201 ms, 2.0 ms scroll frames, 1.9 ms per streamed
  chunk, 50 MB RSS.

**Phase 1, pending (Windows):** the notch self-test in both hit modes (the main risk:
click-through with DirectComposition while `WS_EX_TRANSPARENT` toggles from the 50 ms
poll; fallbacks in REPORT.md), IME, clipboard, drop, the picker, UIA, the scrollbar look,
the selection colour, every visual-parity comparison.

**What still differs by design:** listed in REPORT.md under the 1B section (no glass
blur until the office scene exists; drops taken on the whole window; a click beside the
menu only closes it; web images over 32 MB are broken).

**Not started:** Phase 2 (office scene in wgpu, full benchmark), Phase 3 (A: AcpHost,
KiroStream, sessions, job object, provider adapters; B: AES-GCM framing, DPAPI, the
System.Text.Json shapes, C#↔Rust round-trips; C: settings, quotas, palette and themes,
music, tray, dashboard, single instance), Phase 4 (validation, installer, cutover).
Costs per area are in REPORT.md.

## Next steps

1. Get the user's answer on what to start next (decision 4), with multiple-choice
   questions if needed.
2. If 3A: port AcpHost/KiroStream/KiroSession from `src/Hover/**` into a new
   `native/crates/hover-acp`, tested against `port/tools/FakeAcp` recordings.
3. If 3B: port the on-disk formats (settings, sealed history) into
   `native/crates/hover-store`, with round-trip tests against `port/tools/HoverFixture`
   output. DPAPI itself stays a Windows-only, pending check.
4. Keep every Windows item in RUN-ON-WINDOWS.md up to date as features land.
