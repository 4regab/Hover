# Handoff: Hover native port (Rust + Slint + wgpu)

Last updated 2026-09-28, at commit `bbd1d66` plus the commit carrying this file.
Read in this order: this file, `port/README.md`, `port/phase3/REPORT.md` (the latest
evidence and decisions), `port/phase1/REPORT.md` (the chat and notch prototypes),
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

No questions are open.

## Where things are

| Path | Contents |
|---|---|
| `port/phase0/` | BASELINE, FEATURES (the per-feature checklist, kept current), SCREENS (every screen and its automation ids), MARKDOWN, BENCHMARK (**frozen**), `baseline/*.png` (21 Chromium captures of the page, taken with scrollbars hidden) |
| `port/phase1/` | REPORT (chat and notch prototypes), RUN-ON-WINDOWS, `shots/` |
| `port/phase3/` | REPORT (3B and 3A done: what, evidence, differences, answered questions), RUN-ON-WINDOWS (3B, 3A checks), `shots/` |
| `port/bench/` | `Measure-Hover.ps1` (S1–S6, Windows), `capture-office.mjs` (page captures, scrollbars shown) |
| `port/tools/` | `FakeAcp` (stand-in ACP agent, net10.0, runs on Linux), `HoverFixture` (WPF + DPAPI, Windows only) |
| `native/crates/hover-core` | paths (+ Noty move), `settings.json` (System.Text.Json's bytes), json/time (the serializer's rules), shortcut (WPF key names), crypto (AES-GCM, `KeyGuard`), `platform/{windows,linux}` (DPAPI / Secret Service + 0600 file; HKCU Run / XDG autostart), history (sealed `agents/`), images (`kiro-images`), single (one instance), `bin/hover-data` (`write`, `dump`, `where`) |
| `native/crates/hover-agents` | stream (KiroStream), acp (AcpHost), agents (exe, args, hints, checks), proc (hidden start; job per tool on Windows; process group + PDEATHSIG + watchdog on Linux), cancel, session (KiroSession/KiroSessions), state (the office's `state`/`transcript`/`say` messages), `examples/probe` |
| `native/crates/hover-md`, `hover-diagram` | md.js and diagram.js, byte-identical |
| `native/crates/hover-chat` | the chat thread: layout, selection, copy, word units, scrollbars, smooth scroll, images, painter, `state.rs` (state JSON to turns) |
| `native/crates/hover-notch` | notch geometry, animation, hover rules (Notch.cs) |
| `native/apps/chat-proto` | the Slint drawer: `main.rs`, `net.rs` (images), `compose.rs` (composer images), `models.rs` (model pill), `live.rs` (a real ACP session), `ui/chat.slint` |
| `native/apps/notch-proto` | Slint + wgpu 30 DX12 notch; Win32 in `win.rs`; `--selftest`; a plain window on Linux |
| `native/golden/` | page/JS generators, `fixtures/` (incl. `office-state.json`), `expected/`, `acp/` (FakeAcp recordings) |

## Setting up a new sandbox (Amazon Linux 2023)

```sh
dnf install -y dejavu-sans-fonts dejavu-sans-mono-fonts dejavu-serif-fonts \
  xorg-x11-server-Xvfb weston dbus-daemon dbus-tools dbus-x11 gnome-keyring libsecret \
  at-spi2-core python3-gobject mesa-vulkan-drivers mesa-libEGL libxkbcommon-x11 \
  squashfs-tools rpm-build sqlite
rustup target add x86_64-pc-windows-msvc
# Playwright outside the repo, for the page goldens:
mkdir -p /projects/sandbox/pw && cd /projects/sandbox/pw && npm init -y && npm i playwright
npx playwright install chromium          # not --with-deps (no apt)
ln -s /projects/sandbox/pw/node_modules /projects/sandbox/node_modules
# .NET 10, only to run FakeAcp live and re-record (tests don't need it):
curl -sSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 10.0 --install-dir /opt/dotnet10
cd /projects/sandbox/Hover
DOTNET_ROOT=/opt/dotnet10 /opt/dotnet10/dotnet build port/tools/FakeAcp/FakeAcp.csproj -c Release -o /projects/sandbox/fakeacp
pip install pillow                       # half-size copies of 2x shots for viewing
mkdir -p /projects/sandbox/work          # scratch: /tmp is emptied between calls
```

## Commands

```sh
cd native
cargo test --release --workspace                                        # 103 tests
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
```

## State

**Done and tested on Linux** (details and evidence in the reports):
- Phase 0: docs, frozen benchmark, Chromium captures.
- Phase 1: md.js and diagram.js byte-identical; the chat thread (copy identical to
  Chromium on 605 cases, 3 602 of 3 606 page clicks, scrollbars, smooth scroll, images,
  composer images, model menu, toast, transcript, fade, UIA text nodes); the notch
  prototype (Windows code, type-checked).
- Phase 3B: persistence, `hover-core` (33 tests incl. a real GNOME Keyring on a private bus).
- Phase 3A: sessions and ACP, `hover-agents` (35 tests: the C# AcpHost/session tests
  ported, 5 FakeAcp recordings replayed, the state message byte for byte); tools die with
  a `kill -9`'d Hover on Linux; `chat-proto --acp` on a live session.

**Pending on Windows** (steps in the RUN-ON-WINDOWS files): the notch self-test in both
hit modes, IME/clipboard/drop/picker/UIA, visual parity, the C# baseline, DPAPI and the
C#↔Rust data round trips, real turns with kiro-cli/Codex/Cursor, the job object on a
killed Hover.

**Not started:** 3C (product), Phase 2 (office scene), Phase 4 (validation, packaging,
cutover). Checklist with the done items ticked: `port/NEXT-PROMPT.md`.

## Next: Phase 3C, where to start

C# to read (with a context-gatherer sub-agent): `Owl/Pages.cs` (588 lines, Settings'
five sections), `Core/Quota.cs` (428), `Core/Palette.cs` (301), `Owl/Theme.cs`,
`Owl/Ui.cs`, `Owl/Notch.cs` (854: rest pill, greeting, rings, alert, dashboard),
`Owl/Bot.cs`, `Services/TrayIcon.cs`, `Services/Actions.cs`, `Interop/HotKeys.cs`,
`Interop/HostWindow.cs`, `Owl/OwlApp.cs` (quota polling, end announcements),
`App.xaml.cs`. `port/phase0/SCREENS.md` lists the screens and automation ids.

Suggested order: quotas and palette first (pure logic, testable against the C# tests in
`tests/Hover.Tests/LayoutAndQuotaTests.cs`), then an app shell that owns `hover-core`
and `hover-agents` (single instance, tray, hotkey, settings window), then the notch
extras and the Linux notch (X11 override-redirect + ARGB + XShape under Xvfb; Wayland
under `weston --backend=headless`, layer-shell or documented limits).

## Lessons

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
