# Prompt for the next agent: finish the Hover native port (3C, 2, 4)

Copy everything below the line into the next session.

---

You are continuing the native port of Hover (repo `4regab/Hover`, branch
`rust-port/phase-0-1`, draft PR #5). Your session runs on **Linux only**; there is no
Windows PC. Read these before anything else:
1. `port/HANDOFF.md` (rules, decisions 1–11, setup, commands, state, lessons)
2. `port/README.md` (the map and the platform layout)
3. `port/phase3/REPORT.md` (the latest evidence: 3B and 3A)
4. `AGENTS.md`

Set up the sandbox as HANDOFF.md says (including `/projects/sandbox/work`, the edit
helpers and FakeAcp). Run `cargo test --release --workspace` (103 tests) and
`cargo check --release --workspace --all-targets --target x86_64-pc-windows-msvc`, and
confirm both are green before you change anything.

## Goal

Phases 0, 1, 3B and 3A are done. Finish the rest, so the Rust app does everything the C#
app does **on Windows and on Linux**. The Windows build matches the C# app 1:1; the
Linux build behaves the same wherever the platform allows, and where it can't, the
report says what the nearest equivalent is and asks the user.

The end state is **one Rust app with no .NET in it** (decision 11): once the port is cut
over, the user removes the C# app, `src/`, `tests/`, `port/tools/` and the .NET build.
So nothing you build may need .NET to build, test or run, and the Phase 4 cutover
checklist must list first what still needs the C# app (the Windows baseline, the WebView2
captures, the C#↔Rust round trips). **Don't remove anything yourself.**

Work in this order; each phase ends with its report, tests and a push:
1. **Phase 3C:** the product (an app that owns `hover-core` and `hover-agents`)
2. **Phase 2:** the office scene
3. **Phase 4:** validation, packaging, the cutover checklist

Reorder only if a dependency forces it, and say why.

## Rules (from the user; they override anything else)

- **No GitHub Actions, ever.** Never add, trigger, re-run or wait on a workflow. If a
  run appears, cancel it at once (the command is in HANDOFF.md).
- **Push to `rust-port/phase-0-1` after each feature.** No new branch, no new PR.
- **Report after each feature**: a short summary with evidence (test output, measured
  numbers, screenshots in `port/phaseN/shots/`, pushed and linked on GitHub). Never open
  an image over 5 MB or 2000 px on a side. **Tick the checklist below in this file as
  items finish** (and in the todo tool), and show it in each report.
- **Differences from the C# app:** fix obvious bugs and list each one in the report.
  When unsure (a quirk users may rely on, a Linux-vs-Windows choice), ask the user with
  **multiple-choice questions** (they answer like `1A 2B`) and keep working meanwhile.
- **Parity method: port the C# logic line by line** and test it against C#-derived
  output (`native/golden/`, the FakeAcp recordings in `native/golden/acp/`, the C# tests
  in `tests/Hover.Tests/`, ported). You may read all of `src/Hover/**` and `web/**`.
  **No new .NET fixture tools.** Where a fixture is missing, derive the expected values
  from the source and say so in the test's comment.
- **New crates are allowed with a stated reason** in `Cargo.toml`; prefer what is
  already in the tree (zbus, x11rb, wayland-client, smithay-client-toolkit, accesskit,
  image, chrono, windows are there).
- **Repo rules (`AGENTS.md`):** CRLF; comments say why, not what; surgical changes;
  never change `src/`, `web/` or `tests/` (the C# app is still the product).
- **Windows gates stay pending, never "passed".** Every Windows-only check goes into the
  phase's `RUN-ON-WINDOWS.md`. Keep the MSVC `cargo check` green for every crate and app.
  Linux gates **are** run: Xvfb, and `weston --backend=headless` for Wayland.
- Don't loosen `port/phase0/BENCHMARK.md`. Measure Linux against the same thresholds;
  the Windows numbers stay pending.
- No WebView2, Chromium or web view in any part of the app.

## Linux: what "works on Linux too" means

Build the Windows path from the C# code and a Linux path with the same behaviour, behind
`platform::{windows,linux}` per crate (see README's platform layout).

| Area | Windows (from the C# app) | Linux target | State |
|---|---|---|---|
| Data folder | `%APPDATA%\Hover`, `HOVER_DATA_DIR` | `$XDG_DATA_HOME/Hover` | done (3B) |
| `note.key` | DPAPI (CurrentUser) | Secret Service, else a 0600 file; never destroyed (decision 7) | done (3B) |
| Single instance | the C# mutex and show event | lock + socket in `$XDG_RUNTIME_DIR` | done (3B); wire it into the app |
| Agent CLIs | the C# paths, env, job object | Linux paths, `PATH`, `~/.local/bin`, process group + watchdog | done (3A) |
| Notch window | DirectComposition, `WS_EX_*`, click-through poll | X11: override-redirect, ARGB visual, XShape input region. Wayland: wlr-layer-shell where available; else document the limits and ask | 3C |
| Global shortcut (Alt+N) | `RegisterHotKey`, rebindable, conflict warning | X11 `XGrabKey`; Wayland: GlobalShortcuts portal, else document | 3C |
| Tray | Shell notify icon, menu, balloon | StatusNotifierItem (D-Bus) + notifications (`org.freedesktop.Notifications`) | 3C |
| Launch at login | HKCU Run | XDG autostart | done (3B); wire it into Settings |
| Quota readers (incl. SQLite) | the Windows paths in `Quota.cs` | the same files at their Linux locations; report any that don't exist | 3C |
| Fonts | Segoe UI (Variable) | what `system-ui`/`sans-serif` resolves to via fontconfig; report the face | 3C, 2 |
| Theme, accent, reduced motion | the Windows settings the C# app reads | the settings portal (`color-scheme`, reduced motion); GNOME/KDE fallbacks | 3C |
| Music | `office-beats.ogg` in the page | the same file, a native decoder and audio output | 3C |
| Packaging | `installer/Hover.iss` (for the Rust exe now) | an AppImage and a `.deb` (or the smallest honest pair); say which and why | 4 |

## Acceptance criteria

- `cargo test --release --workspace` green; MSVC `cargo check` green; `cargo clippy
  --workspace --all-targets` shows no new warnings in code you wrote.
- Each ported C# component has tests against C#-derived expected output (quotas against
  `LayoutAndQuotaTests.cs`, the palette and VS Code theme reader, the shortcut rebinding).
- The app runs on Linux under Xvfb: the notch, the office, the chat, Settings and the
  tray, with screenshots in the reports. The Wayland path runs under headless weston or
  its limits are written down with MCQs.
- The benchmark is measured on Linux (S1–S6 equivalents: `Measure-Hover.ps1`'s steps as a
  Linux script) and reported against the frozen thresholds.
- Each phase has `port/phaseN/REPORT.md` (what was built, evidence, differences found and
  fixed, what is pending on Windows, costs of what's left); `RUN-ON-WINDOWS.md` gains
  every new Windows check; `port/README.md` and `port/phase0/FEATURES.md` stay current.
- `port/HANDOFF.md` is rewritten at the end for the agent after you, and this file's
  checklist is ticked. When the session is running out, rewrite them early.

## Checklist (keep it in your todo tool too; tick each item here as you finish it)

**Setup**
- [x] Sandbox set up (fonts, Windows target, Playwright, Xvfb); tests and Windows check green
- [x] Read `src/Hover/**` for the phase; note the parts to port
- [x] Platform trait layout decided (`platform::{windows,linux}`) and noted in `port/README.md`

**Phase 3B: persistence** (done, `9bf324a`; report in `port/phase3/REPORT.md`)
- [x] Paths (Windows `%APPDATA%`, Linux XDG, `HOVER_DATA_DIR`), including the Noty → Hover migration
- [x] Settings JSON: the same shapes and defaults as `System.Text.Json`, round-trip tests
- [x] AES-GCM framing of sealed data; round trip with `hover-data` (HoverFixture's C# run pending on Windows)
- [x] Key protection: DPAPI (Windows, check pending) and Secret Service / 0600 file (Linux, tested); never destroyed (decision 7)
- [x] Session history on disk, and `kiro-images` with its 14-day cleanup
- [x] Report section; RUN-ON-WINDOWS items; push; report

**Phase 3A: sessions** (done, `bbd1d66`)
- [x] AcpHost, KiroStream, KiroSession(s), the job object / process group
- [x] Provider adapters (Kiro, Codex, Cursor): Linux paths and the Windows ones
- [x] Replays of FakeAcp recordings: turns, steps, answers, stop, queue, errors, offers
- [x] The office `state` message built by Rust, byte for byte on the fixture
- [x] chat-proto driven by a real (Fake)ACP session
- [x] Report section; push; report

**Phase 3C: product**
- [ ] The app shell (`native/apps/hover`): owns settings, key, history, sessions and the tool hosts; single instance (second launch opens the dashboard); quits cleanly (tools shut down, history flushed, settings flushed)
- [ ] Settings window (General, Integrations, Kiro, Codex, Cursor) in Slint, 1:1 with `Pages.cs` (the automation ids in SCREENS.md)
- [ ] Quotas (Claude, Kiro, Codex, Cursor with SQLite) at their Windows and Linux locations; the 5-minute refresh and readable failures
- [ ] Palette and VS Code themes (JSONC, `include`, the installed-theme finder on both platforms); light and dark; System follows the platform
- [ ] Music (office-beats.ogg), tray and notifications, dashboard window, the Alt+N hotkey with rebinding and the conflict warning; launch at login in Settings
- [ ] Notch extras: greeting animation, quota rings, the running and done states, the alert; end announcements (A10)
- [ ] Linux notch: the X11 path under Xvfb with the self-test adapted; the Wayland path under headless weston, or documented limits plus MCQs
- [ ] Report section; push; report

**Phase 2: office scene**
- [ ] The three.js scene (`web/office/main.js`, 1 187 lines) in wgpu: voxel room, PBR with ACES, PCF soft shadows, sprites and particles, canvas textures, bots and poses, camera, picking, tags, frame pacing
- [ ] The page's UI around it (FEATURES O14–O17: panels, toast, confirm, HUD, the new-task circle), driven by the Rust state message
- [ ] Glass blur behind the drawer, menu and toast
- [ ] Visual comparison against the page's Chromium captures (scrollbars shown); numbers per SCREENS.md
- [ ] The full-workload benchmark on Linux against the frozen thresholds
- [ ] `port/phase2/REPORT.md`; push; report

**Phase 4: validation and packaging**
- [ ] The full benchmark and parity sweep on Linux (the Windows run stays pending, with steps written)
- [ ] UIA (Windows, pending) and AT-SPI (Linux, tested with pyatspi) mapping
- [ ] Packages: the Windows installer for the Rust exe (from `Hover.iss`, build pending) and a Linux AppImage/.deb (built and smoke-tested)
- [ ] The cutover checklist: first what still needs the C# app before .NET goes (baseline, WebView2 captures, round trips), then what must pass on Windows, what passed on Linux, and the list of what the removal takes out; an MCQ on keeping the page's JS for the goldens
- [ ] `port/phase4/REPORT.md`; README, FEATURES, HANDOFF and this checklist updated; push; final report

## What not to do

- Don't mark any Windows gate as passed, and don't claim anything you didn't run.
- Don't change the C# app, `web/`, `tests/` or the frozen benchmark, and don't remove
  .NET or any C# file: the removal is the user's step after the gates.
- Don't add CI, and don't push anywhere but `rust-port/phase-0-1`.
- Don't leave work uncommitted at the end of a feature.
- Don't swap in WebView2, Chromium or a web view for any part.
