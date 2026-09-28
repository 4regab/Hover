# Handoff: Hover native port (Rust + Slint + wgpu)

Read this first to continue the work in a new session.

## Rules the user set (must follow)

- **Never trigger, re-run or wait on GitHub Actions.** The user has no Actions quota.
  A push to `rust-port/phase-0-1` starts `ci.yml`, because PR #5 is open. Push only when
  the user asks, and cancel any run a push starts:
  `gh api -X POST repos/4regab/Hover/actions/runs/<id>/cancel`
  (list the runs with `gh api "repos/4regab/Hover/actions/runs?branch=rust-port/phase-0-1&per_page=5"`).
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
| `.github/workflows/native.yml` | manual only (`workflow_dispatch`); never ran to completion |

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
# the copy goldens need Playwright: npm i playwright; then node native/golden/gen-copy.mjs
```

On Linux the tests need fontconfig, plus DejaVu Sans Mono for the code font.

## State at handoff

**Done:**
- Phase 0 docs and the baseline captures.
- md.js and diagram.js ported with byte parity: 7 fixtures, 3 956 random documents and
  8 diagrams.
- Chat copy identical to Chromium's on 605 cases.
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

**Open decisions for the user:**
1. md.js hangs on U+2028/U+2029 in list lines, and throws on `\0n\0`. The port does
   neither. Keep that, or copy the old behaviour?
2. Step-list open/closed state follows the turn index across sessions (a baseline quirk,
   copied in the port). Keep it, or fix it?

## Next steps

1. Wait for the user's Windows results (RUN-ON-WINDOWS.md) before Phase 2.
2. Meanwhile, portable Phase 1 chat gaps:
   - horizontal scroll in `pre` and tables;
   - a thin scrollbar;
   - double- and triple-click selection;
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
