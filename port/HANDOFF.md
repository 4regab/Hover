# Handoff: Hover native port (Rust + Slint + wgpu)

Last updated 2026-09-29, at the commit carrying this file. All phases are built on Linux;
what is left is the Windows run, the open questions and the cutover. Phase 6
(`port/phase6/REPORT.md`) ran every feature end to end under Xvfb and fixed the eleven UI
bugs it found; `port/e2e/run.sh` keeps that run. Phase 7 (`port/phase7/REPORT.md`) ported main's last
commit, OpenCode (55111fc), fixed ten more differences from main's page, and grew the
run to 79 checks.
Read in this order: this file, `port/README.md`, `port/phase4/REPORT.md` (the cutover
checklist), `port/phase2/REPORT.md`, `port/phase3/REPORT.md`, `port/phase1/REPORT.md` (the chat and notch prototypes),
`AGENTS.md`. The prompt that sets the next session's work is `port/NEXT-PROMPT.md`.

## Rules the user set (they override anything else)

- **No GitHub Actions, ever.** No quota. The workflows are removed from this branch, so
  a push starts nothing. Never add one, never trigger, re-run or wait on one. If a run
  appears, cancel it: `gh api -X POST repos/4regab/Hover/actions/runs/<id>/cancel`
  (list: `gh api "repos/4regab/Hover/actions/runs?branch=rust-port/phase-0-1&per_page=5"`).
- **Push to `rust-port/phase-0-1` after each feature**, without asking. No new branch, no
  new PR (draft PR #5: https://github.com/4regab/Hover/pull/5).
- **Report after each feature**: a short summary with evidence (test output, numbers,
  screenshots in `port/phaseN/shots/`, pushed and linked on GitHub). Keep the checklist
  in `port/NEXT-PROMPT.md` ticked as items finish, not only the todo tool.
- **Never open an image over 5 MB or 2000 px on a side.** Check with `file`; shots at 2×
  are 2208 px wide, so view a half-size copy (Pillow; ImageMagick isn't installed).
- **Differences from the C# app**: fix obvious bugs and list them; when unsure, ask
  multiple-choice questions (answered like `1A 2B`) and carry on with other work.
- **Parity: port the C# line by line**, tested against C#-derived output. **No new .NET
  fixture tools.** Where no fixture exists, derive the expected value from the source
  and say so in the test.
- **Windows gates stay pending**, never "passed". Every Windows-only check goes into a
  `RUN-ON-WINDOWS.md`. `cargo check --target x86_64-pc-windows-msvc` stays green.
- **Repo rules** (`AGENTS.md`): CRLF; comments say why, not what; surgical changes; new
  crates only with a reason written in `Cargo.toml`. **Never change `src/`, `web/` or
  `tests/`.** Don't loosen `port/phase0/BENCHMARK.md`.
- Nothing replaces the C# app until the gates pass and the user decides the cutover.
  No WebView2, Chromium or web view in any part of the port.

## Decisions (all dated 2026-09-28)

1. md.js hangs on U+2028/U+2029 in list lines and throws on `\0n\0`: the port does neither.
2. Step-list open/closed state is per session (the baseline's leak is fixed).
3. Work on Linux through all phases; Windows gates stay pending.
4. Order: 3B, 3A, 3C, 2, 4.
5. **Linux is a first-class target**, not only the dev VM (the table in NEXT-PROMPT.md).
6. Parity by line-by-line port; obvious bugs fixed and listed; unsure → MCQs.
7. **`note.key` is never destroyed** (Q1, both platforms): unreadable for good → kept as
   `note.key.unreadable-<yyyyMMddHHmmss>`, new key made; unreadable *now* (keyring not
   running or locked) → left alone, no history that run, retried next start; a new key
   that can't be stored → no history that run. Built in `hover-core::crypto`.
8. Linux without a Secret Service: the key in a 0600 `note.key` (Q2A).
9. `settings.json` on Linux is LF, as .NET writes there (Q3A).
10. Agent tools are not paused when Hover is suspended (Q4A).
11. **.NET goes completely once the port is finished and cut over** (the C# app,
    `src/`, `tests/`, `port/tools/`, `Hover.slnx`, the .NET parts of `build.ps1` and the
    installer). So:
    - Nothing in `native/` may need .NET to build, test or run. (Already true: the
      FakeAcp recordings replay without it; `hover-data` does HoverFixture's job.)
    - What needs the C# app must be done **before** it goes: the Windows C# baseline
      (`Measure-Hover.ps1`, S1–S6), the WebView2 captures, the C#↔Rust round trips in
      `port/phase3/RUN-ON-WINDOWS.md`. The Phase 4 cutover checklist lists them first.
    - Reading what the C# app left on users' machines stays forever: `%APPDATA%\Hover`
      (and the Noty move), the DPAPI `note.key`, `settings.json`, `agents/*.dat`.
    - The golden generators (`native/golden/gen*.mjs`) run the office page in `web/`;
      when `web/` goes, the goldens are frozen as they are. Phase 4 decides (MCQ) whether
      to keep a copy of the page's JS under `native/golden/` for regenerating them.
    - Don't remove anything yet; that is the user's step after the gates.

12. The office has its own wgpu device on its own thread, and its frame is composited
    on the CPU (`hover-office::live`, `page`). It is dropped 30 s after it is hidden,
    as KiroPage drops its WebView2.
13. The benchmark on Linux is `port/bench/measure-hover.py`, with `fake-acp.py` as the
    agent (no .NET).

**Open questions** (all in the reports; answers look like `5A 7B`):
- 5: the notch on native Wayland. 6: bare X without a compositor (`phase3/REPORT.md`).
- 7: the scene's over-ΔE-10 share (`phase2/REPORT.md`).
- 8: keeping the page's JS for the goldens. 9: S6 growth. 10: an AppImage (`phase4/REPORT.md`).

## Where things are

| Path | Contents |
|---|---|
| `port/phase0/` | BASELINE, FEATURES (the per-feature checklist, kept current), SCREENS (every screen and its automation ids), MARKDOWN, BENCHMARK (**frozen**), `baseline/*.png` (21 Chromium captures of the page, taken with scrollbars hidden) |
| `port/phase1/` | REPORT (chat and notch prototypes), RUN-ON-WINDOWS, `shots/` |
| `port/phase3/` | REPORT (3B and 3A done: what, evidence, differences, answered questions), RUN-ON-WINDOWS (3B, 3A checks), `shots/` |
| `port/phase2/` | REPORT (the office), `capture-scene.mjs` + `compare.py` (scene ΔE against the page), `shots/` |
| `port/phase4/` | REPORT (benchmark, AT-SPI, packages, cutover checklist), RUN-ON-WINDOWS (phases 2 and 4), `bench-linux.json`, `atspi-dump.py`, `atspi-office.txt` |
| `port/bench/` | `Measure-Hover.ps1` (S1–S6, Windows), `measure-hover.py` + `fake-acp.py` (S1–S6, Linux), `capture-office.mjs` (page captures, scrollbars shown) |
| `port/tools/` | `FakeAcp` (stand-in ACP agent, net10.0, runs on Linux), `HoverFixture` (WPF + DPAPI, Windows only) |
| `native/crates/hover-core` | paths (+ Noty move), `settings.json` (System.Text.Json's bytes), json/time (the serializer's rules), shortcut (WPF key names), crypto (AES-GCM, `KeyGuard`), `platform/{windows,linux}` (DPAPI / Secret Service + 0600 file; HKCU Run / XDG autostart), history (sealed `agents/`), images (`kiro-images`), single (one instance), `bin/hover-data` (`write`, `dump`, `where`) |
| `native/crates/hover-agents` | stream (KiroStream), acp (AcpHost), agents (exe, args, hints, checks), proc (hidden start; job per tool on Windows; process group + PDEATHSIG + watchdog on Linux), cancel, session (KiroSession/KiroSessions), state (the office's `state`/`transcript`/`say` messages), `examples/probe` |
| `native/crates/hover-md`, `hover-diagram` | md.js and diagram.js, byte-identical |
| `native/crates/hover-chat` | the chat thread: layout, selection, copy, word units, scrollbars, smooth scroll, images, painter, `state.rs` (state JSON to turns) |
| `native/crates/hover-notch` | notch geometry, animation, hover rules (Notch.cs) |
| `native/apps/chat-proto` | the Slint drawer: `main.rs`, `net.rs` (images), `compose.rs` (composer images), `models.rs` (model pill), `live.rs` (a real ACP session), `ui/chat.slint` |
| `native/apps/notch-proto` | Slint + wgpu 30 DX12 notch; Win32 in `win.rs`; `--selftest`; a plain window on Linux |
| `native/crates/hover-quota` | Quota.cs: the four readers, SQLite (bundled on Linux, winsqlite3 on Windows), .NET number formatting |
| `native/crates/hover-office` | the office: scene, bot, canvas, office (camera, picking, pacing, tags), `office.wgsl` + render (three.js 0.170's shading), page (the composite), live (the thread), `examples/shot` |
| `native/apps/hover` | the product: app, pages (Settings), rest, music, sni (tray), notch, x11, win, office_ui, bench (`HOVER_BENCH`), selftest, shots; `ui/*.slint` |
| `native/installer/` | `Hover.iss` for the Rust exe (same AppId as C#), `package-linux.sh` (.deb + tarball) |
| `native/golden/` | page/JS generators, `fixtures/` (incl. `office-state.json`), `expected/`, `acp/` (FakeAcp recordings) |

## Setting up a new sandbox (Amazon Linux 2023)

```sh
dnf install -y dejavu-sans-fonts dejavu-sans-mono-fonts dejavu-serif-fonts \
  xorg-x11-server-Xvfb weston dbus-daemon dbus-tools dbus-x11 gnome-keyring libsecret \
  at-spi2-core python3-gobject mesa-vulkan-drivers mesa-libEGL libxkbcommon-x11 \
  squashfs-tools rpm-build sqlite xorg-x11-server-Xwayland mesa-dri-drivers alsa-lib-devel xorg-x11-utils
rustup target add x86_64-pc-windows-msvc
# Playwright outside the repo, for the page goldens:
mkdir -p /projects/sandbox/pw && cd /projects/sandbox/pw && npm init -y && npm i playwright
npx playwright install chromium          # not --with-deps (no apt)
ln -s /projects/sandbox/pw/node_modules /projects/sandbox/node_modules
# .NET 10, only to run FakeAcp live and re-record (tests don't need it):
curl -sSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 10.0 --install-dir /opt/dotnet10
cd /projects/sandbox/Hover
DOTNET_ROOT=/opt/dotnet10 /opt/dotnet10/dotnet build port/tools/FakeAcp/FakeAcp.csproj -c Release -o /projects/sandbox/fakeacp
pip install pillow numpy fonttools       # half-size copies of 2x shots; compare.py
(cd /projects/sandbox/pw && npm i three@0.170.0)   # capture-scene.mjs
# Node: PATH=/root/.nvm/versions/node/v22.23.3/bin:$PATH
mkdir -p /projects/sandbox/work          # scratch: /tmp is emptied between calls
```

## Commands

```sh
cd native
cargo test --release --workspace --no-fail-fast                         # 151 tests
cargo check --release --workspace --all-targets --target x86_64-pc-windows-msvc
cargo clippy --release --workspace --all-targets                        # new code: no warnings
# FakeAcp live, and re-recording golden/acp (only when the scenarios change):
DOTNET_ROOT=/opt/dotnet10 FAKEACP=/projects/sandbox/fakeacp/FakeAcp HOVER_RECORD=1 \
  cargo test --release -p hover-agents --test fakeacp -- --test-threads=1
# chat on a real session, headless, 2x (FAKEACP_ANSWER must be absolute: tools start in $HOME):
DOTNET_ROOT=/opt/dotnet10 HOVER_DATA_DIR=/projects/sandbox/work/d FAKEACP_SECONDS=1.5 \
  FAKEACP_ANSWER=$PWD/golden/fixtures/rich.md ./target/release/chat-proto \
  --acp /projects/sandbox/fakeacp/FakeAcp --folder /projects/sandbox/work/proj --screenshot out.png --scale 2 [--mid]
./target/release/chat-proto --screenshot out.png [--scale 2] [--session N] [--turns N] [--select] [--menu] [--history] ...
./target/release/chat-proto --bench                                      # headless timings
./target/release/hover-data write|dump|where ...                         # data folders
cargo run --release -p hover-agents --example probe -- <folder> [kiro|codex|cursor]
node golden/gen.mjs                                                      # md/diagram goldens: git stays clean
./target/release/hover --shots DIR                                        # the product's screens, headless
Xvfb :9 -screen 0 1920x1080x24 +extension GLX +extension RANDR &          # then:
DISPLAY=:9 HOVER_DATA_DIR=... XDG_RUNTIME_DIR=/projects/sandbox/rt ./target/release/hover --selftest DIR   # 13/13
./target/release/examples/shot out.png night|day [--empty]                # the office scene
cd .. && node port/phase2/capture-scene.mjs OUT && python3 port/phase2/compare.py page.png native.png diff.png
python3 port/bench/measure-hover.py --runs 1 --append --out port/phase4/bench-linux.json   # ~7 min a run
sh native/installer/package-linux.sh 0.9.0 /projects/sandbox/dist        # .deb + tarball
dbus-run-session -- python3 port/phase4/atspi-dump.py out.txt           # under DISPLAY; hangs on quit
port/e2e/run.sh [sA sB ...]    # end to end: Xvfb + AT-SPI + XTEST, fake-agent.py and fake-opencode.py as the tools; 79 checks, ~12 min
cargo run --release -p hover-agents --example opencode_live -- DIR "prompt" [model] [access]   # the real opencode (npm i -g opencode-ai)
node port/phase7/capture-main.mjs OUT   # main's page (golden/page) with OpenCode and a question, for side by side
```
(`pip install python-xlib pillow`; PyGObject comes with python3-gobject.)

## State

**Done and tested on Linux** (details and evidence in the reports):
- Phases 0, 1, 3 (3A, 3B, 3C), 2 and 4's Linux half.
- 151 tests. The notch self-test is 13/13 under Xvfb, Xwayland and from the `.deb`.
- The office scene's mean ΔE is 0.83–1.17 (limit 2.0), but the over-10 share is 1.6–3.0 % (limit 1 %: question 7).
- The Linux benchmark: 3 of 5 runs (`bench-linux.json`).
- AT-SPI exposes the HUD ids.
- `.deb` and tarball built. The Rust `Hover.iss` is written.

**Gaps in the office** (`phase2/REPORT.md`, about 2 days):
- The drawer lacks selection and copy, images, the model pill and step toggles.
- No image attach in the new-task box; no model menu.
- The bot's halo light.
- Nodes of a bot that left stay in the scene graph.

**Pending on Windows**: every RUN-ON-WINDOWS file (phases 1, 3, 2 and 4), and G1–G4 against the C# medians.

**Benchmark runs 4 and 5** were cut off. Take them with `--append` into the same file.

## Next

1. Answers to questions 5–10.
2. On Windows, the cutover checklist in `phase4/REPORT.md`, in order: the C# baseline, WebView2 captures and round trips first; then the native checks.
3. The office gaps.
4. The removal, only on the user's go-ahead.

## Lessons

- Background jobs (`&`, `nohup`, `run_in_background`) don't outlive the call here: run
  long work in the foreground with a timeout, in pieces (`--append`).
- The office's drop freed little until the last frame and blur left Slint's globals and
  `malloc_trim(0)` ran (S2 319 → 180 MB).
- `port/bench/*.py` are LF (`.gitattributes`): the `#!` line breaks with CRLF.
- `pkill -f` can match the calling shell: use `pkill -x`.

- Edit files with `/projects/sandbox/edit.py FILE` (stdin: `old\n===\nnew`, blocks split
  by `\n%%%\n`; asserts one match; writes CRLF). New files: `/projects/sandbox/crlf.sh`.
  (Both live outside the repo; recreate them if the sandbox is new.)
- `PR_SET_PDEATHSIG` fires when the forking *thread* ends; tools are forked from one
  long-lived thread (`proc::imp::spawn`).
- A test D-Bus needs its own config with no service directories, or the stock session
  config activates the desktop's keyring in the real home.
- `/tmp` is emptied between tool calls; `pgrep -f name` matches your own shell (kill `$!`).
- Playwright hides scrollbars unless `ignoreDefaultArgs: ['--hide-scrollbars']`.
- Chromium on Linux uses Unix editing behaviour; WebView2 takes the spaces after a
  double-clicked word (applied in the port).
- Slint's software renderer doesn't clip to rounded corners and drops an element with
  both a clip and a shadow.
- winit gives no pointer position during a file drag.
- `ureq` with rustls needs `ring`, which won't cross-compile for MSVC here; Windows uses
  native-tls.
- The office-state fixture was hand-made; `hover-agents/tests/state.rs` lists the four
  values in it that C# can't write.
- Slint, learned the hard way in Phase 6:
  - A model set anew makes its repeated elements anew. Between a press and its release
    that loses the click, and it drops keyboard focus. Change shown models in place
    (`view::sync`, `view::sync_blocks`).
  - A FocusScope in front of a TouchArea takes the press to get focus. Use
    `focus-on-click: false`.
  - A Flickable is no taller than its content inside a layout.
  - When the focused element goes, nothing has the keyboard: give it back explicitly.
  - Assigning to a property bound from outside breaks the binding.
- The accessible actions (AT-SPI `do_action`) bypass all of the above. Drive the UI with
  real input (XTEST), as `port/e2e/` does.
- With no window manager, the harness gives the keyboard to a clicked ordinary window.
  Leave the pointer off the top strip, or hover opens the notch.
