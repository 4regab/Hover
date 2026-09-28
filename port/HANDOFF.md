# Handoff: Hover native port (Rust + Slint + wgpu)

Read this first to continue the work in a new session.

## Rules the user set (must follow)

- **Never trigger, re-run or wait on GitHub Actions.** The user has no Actions quota.
  Both workflows (`ci.yml`, `native.yml`) were removed from this branch at the user's
  request, so pushes start nothing. Don't add them back without asking. If a run ever
  starts, cancel it: `gh api -X POST repos/4regab/Hover/actions/runs/<id>/cancel`.
- **Never open an image over 5 MB or over 2000 px on a side.** Check `file <png>` first;
  downscale a copy or skip it.
- Background shell jobs don't survive in the sandbox. Run long things in the
  foreground in batches (for example `ONLY=a,b node capture-office.mjs`).
- The user's original brief governs the work:
  - Phases 0 to 4, each one gated.
  - 1:1 UI/UX parity with the current app.
  - No WebView2 or Chromium fallback unless it is measured and presented as a separate candidate.
  - Nothing replaces the C# app until the gates pass and the cutover is reviewed.
  - Windows gates are marked pending when no Windows machine is available.

## Where things are

- Repo `4regab/Hover`, branch `rust-port/phase-0-1`, draft PR #5
  (https://github.com/4regab/Hover/pull/5).
- No file under `src/`, `web/` or `tests/` has been changed. The C# app is still the product.
- Start with `port/README.md` (the map) and `port/phase1/REPORT.md` (status, gaps, costs,
  next task).
- The repo rules are in `AGENTS.md`:
  - CRLF line endings;
  - comments explain why, not what;
  - keep changes surgical;
  - no new dependencies without a reason.

| Path | Contents |
|---|---|
| `port/phase0/` | BASELINE, FEATURES (checklist with Rust status), SCREENS, MARKDOWN (what md.js/diagram.js really do, plus 2 baseline defects), BENCHMARK (**frozen** procedure and thresholds; don't loosen them), `baseline/*.png` (21 Chromium captures) |
| `port/phase1/` | REPORT.md, RUN-ON-WINDOWS.md, `shots/` |
| `port/bench/` | `Measure-Hover.ps1` (S1–S6), `capture-office.mjs` (page captures; `BROWSER_CHANNEL=msedge` on Windows) |
| `port/tools/` | `FakeAcp` (a stand-in ACP agent), `HoverFixture` (a sealed history written with Hover's own code) |
| `native/crates/hover-diagram` | diagram.js ported, byte-identical SVG (`js.rs` holds the JS semantics helpers) |
| `native/crates/hover-md` | md.js ported, byte-identical HTML; `blocks.rs` reads it into blocks; `image.rs` is `imageFor` |
| `native/crates/hover-chat` | thread layout (`doc.rs`), painter (`paint.rs`, tiny-skia + swash + resvg), `state.rs` (the office state JSON to turns), copy serialiser (`Tok`/`Copier`) |
| `native/crates/hover-notch` | notch geometry, openness animation, hover state machine, zones (from Notch.cs) |
| `native/apps/chat-proto` | Slint drawer and composer around hover-chat; `--screenshot`, `--select`, `--session N`, `--turns N`, `--stream`, `--bench` |
| `native/apps/notch-proto` | Slint + wgpu 30 DX12 (`DxgiFromVisual`), `WS_EX_NOREDIRECTIONBITMAP`, Win32 layer in `src/win.rs`, `--selftest <dir>` (13 checks), `--hit transparent` |
| `native/golden/` | `gen.mjs` (md and diagram goldens from the real JS), `gen-copy.mjs` (copy goldens from the real page), `fixtures/`, `expected/` |

Specs gathered from the C# source (not in the repo):
- `/projects/sandbox/work/ui-spec.txt`: the notch, settings, styles and automation ids.
- `/projects/sandbox/work/core-spec.txt`: ACP, sessions, quotas, palette and the on-disk JSON.

They may be gone in a new sandbox. If so, re-derive them from `src/Hover/**`.

## Commands

```sh
cd native
cargo test --release --workspace          # all tests (goldens, copy corpus, notch logic)
cargo check --target x86_64-pc-windows-msvc -p notch-proto -p chat-proto   # Windows type-check
./target/release/chat-proto --screenshot out.png --select --session 1
node golden/gen.mjs                       # md/diagram goldens (must stay unchanged)
# the page goldens need Playwright (npm i playwright; npx playwright install chromium):
node golden/gen-copy.mjs     # copy corpus (about 1 min)
node golden/gen-words.mjs    # double/triple clicks on every character (about 6 min)
node golden/gen-scroll.mjs   # box sizes with the scrollbars shown
```

On Linux the tests need fontconfig, plus DejaVu Sans and Sans Mono
(`dnf install dejavu-sans-fonts dejavu-sans-mono-fonts` on Amazon Linux). The Windows
type-check needs `rustup target add x86_64-pc-windows-msvc`. Keep `node_modules` outside
the repo (a symlink at `/projects/sandbox/node_modules` works for the scripts).

## State at handoff

**Done:**
- Phase 0 docs and the baseline captures.
- md.js and diagram.js ported with byte parity: 9 fixtures, 3 956 random documents and
  8 diagrams.
- Chat copy identical to Chromium's on 605 cases.
- Double and triple click: 3 602 of 3 606 page clicks identical (4 emoji cases differ).
- Thin scrollbars (thread, and sideways in code blocks), measured in Chromium with its
  scrollbars shown; box heights within 0.03 px of the page. Finding: the Phase 0
  captures were taken with Playwright's hidden scrollbars (REPORT.md, finding 5).
- Step lists (live step, toggle, flex shrink, ellipsis, tags), prompt thumbnails.
- Both prototypes build, and type-check for Windows.

**Pending (need a real Windows PC):**
- The notch self-test, in both hit modes.
- IME, clipboard and UI Automation.
- The DPAPI and persistence round-trips.
- The C# baseline benchmark.
- Every visual-parity comparison.

**The main notch risk:** whether click-through works with a DirectComposition window
while `WS_EX_TRANSPARENT` is toggled from the 50 ms poll. The fallbacks are in REPORT.md.

**Decisions taken (2026-09-28):**
1. md.js's hang and throw: the port keeps its own behaviour (neither).
2. Step-list state: per session in the port (the baseline's leak is fixed).
3. No Windows PC soon. The user asked to carry on through all phases on Linux. Windows
   gates stay marked **pending**, never passed, and nothing replaces the C# app until
   they are run and the cutover is reviewed.
4. Each push to `rust-port/phase-0-1` is allowed, and the `ci` run it starts is cancelled
   at once. After each feature: a short summary with screenshots.

## Next steps

1. Wait for the user's Windows results (RUN-ON-WINDOWS.md) before Phase 2.
2. Meanwhile, portable Phase 1 chat gaps:
   - smooth (animated) wheel, arrow and track scrolling;
   - the broken-image icon and alt text the page shows for an image that doesn't load;
   - fetching web images;
   - composer image paste, drop and pick;
   - the model menu;
   - the transcript view;
   - the fresh-answer fade;
   - the drawer slide;
   - exposing the selection to UIA;
   - checking the selection colour against WebView2.
3. After the gate review:
   - Phase 2: the office scene in wgpu, then the full-workload benchmark against the
     frozen thresholds.
   - Phase 3 checkpoints A, B and C.
