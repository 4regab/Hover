# Prompt for the next agent: finish the Hover native port (all remaining phases)

Copy everything below the line into the next session.

---

You are continuing the native port of Hover (repo `4regab/Hover`, branch
`rust-port/phase-0-1`, draft PR #5). Your session runs on **Linux only**; there is no
Windows PC. Read these before anything else:
1. `port/HANDOFF.md` (rules, decisions, setup, commands, lessons)
2. `port/README.md`
3. `port/phase1/REPORT.md`
4. `AGENTS.md`

Then set up the sandbox as HANDOFF.md says. Run `cargo test --release --workspace`
and the Windows type-check, and confirm both are green before you change anything.

## Goal

Finish **every remaining phase** (Phases 2, 3A, 3B, 3C and 4) so that the Rust app
does everything the C# app does, **on Windows and on Linux**. Linux is now a first-class
target, not only a dev VM. The Windows build must keep matching the C# app 1:1. The
Linux build must behave the same wherever the platform allows. Where it can't, write
down the nearest equivalent in the report and ask the user.

Work in this order. Each phase ends with its report, tests and a push:
1. **Phase 3B:** persistence (the data every other phase depends on)
2. **Phase 3A:** sessions and ACP
3. **Phase 3C:** the product
4. **Phase 2:** the office scene
5. **Phase 4:** validation and packaging

Reorder only if a dependency forces it, and say why.

## Rules (from the user; they override anything else)

- **No GitHub Actions, ever.** Never add, trigger, re-run or wait on a workflow. If a
  run appears, cancel it at once (the command is in HANDOFF.md).
- **Push to `rust-port/phase-0-1` after each feature.** No new branch, no new PR.
- **Report after each feature.** Give a short summary with evidence: test output,
  measured numbers and screenshots. Put the screenshots in `port/phaseN/shots/`, push them,
  and link to them on GitHub. Never open an image over 5 MB or 2000 px on a side.
- **Differences from the C# app:**
  - Fix obvious bugs, list each one in the report, and move on.
  - When you are unsure (a quirk users may rely on, or a Linux-vs-Windows choice), ask
    the user with **multiple-choice questions**. They answer like `1A 2B`. Keep working on
    something else while you wait.
- **Parity method: port the C# logic line by line.** Test it against output the real
  C# code produces, as the md.js port was tested. The C# app can't run here, so use the
  existing fixtures (`port/tools/FakeAcp` recordings, `port/tools/HoverFixture` output,
  `native/golden/`). You may read all of `src/Hover/**`. **Do not write new .NET fixture
  tools.** Where a fixture is missing, derive the expected values from the C# source and
  say so in the test's comment.
- **New crates are allowed with a stated reason.** Put a comment in `Cargo.toml` saying
  why, and prefer what is already in the tree.
- **Repo rules (`AGENTS.md`):**
  - CRLF line endings.
  - Comments say why, not what.
  - Keep changes surgical.
  - Never change `src/`, `web/` or `tests/`; the C# app is still the product.
- **Windows gates stay pending, never "passed".**
  - Every Windows-only check goes into the relevant `RUN-ON-WINDOWS.md`.
  - Keep `cargo check --target x86_64-pc-windows-msvc` green for every crate and app.
  - Linux gates **can** be run: install Xvfb (and a Wayland compositor if you can, e.g.
    `weston` headless) and run the apps and self-tests for real.
- Don't loosen `port/phase0/BENCHMARK.md`. Measure Linux against the same thresholds
  and report the numbers. The Windows numbers stay pending.
- Nothing replaces the C# app. The cutover is the user's decision after the gates.

## Linux: what "works on Linux too" means

For each platform-specific piece, build the Windows path from the C# code and a Linux
path with the same behaviour:

| Area | Windows (from the C# app) | Linux target |
|---|---|---|
| Data folder | `%APPDATA%\Hover` (`Paths.Support`, `HOVER_DATA_DIR`) | `$XDG_DATA_HOME/Hover` (`~/.local/share/Hover`), same env override |
| Key protection (`note.key`) | DPAPI (CurrentUser) | Secret Service (libsecret) when present, else a 0600 key file; the report states the trade-off |
| Notch window | DirectComposition, `WS_EX_*`, click-through poll | X11: override-redirect, ARGB visual, input shape (XShape) for click-through. Wayland: wlr-layer-shell where available; else document the limits and ask |
| Global shortcut (Alt+N) | `RegisterHotKey` | X11 `XGrabKey`; on Wayland the GlobalShortcuts portal (xdg-desktop-portal), else document |
| Tray | Shell notify icon | StatusNotifierItem (D-Bus) |
| Single instance | the C# mutex/pipe | a Unix socket or lock file in `$XDG_RUNTIME_DIR` |
| Agent CLIs (Kiro, Codex, Cursor) | the paths, env and job object in the C# code | the same CLIs on Linux: their Linux paths, `PATH`, and a process group (`setsid` + kill group) for the job object |
| Quota readers (incl. SQLite) | the Windows paths in the C# code | the same files at their Linux locations; report any that don't exist |
| Fonts | Segoe UI (Variable) | what Chromium's `system-ui`/`sans-serif` resolves to via fontconfig; report the face |
| Theme, accent, reduced motion | the Windows settings the C# app reads | the freedesktop settings portal (`color-scheme`, reduced motion); GNOME/KDE fallbacks |
| Packaging | `installer/Hover.iss` | an AppImage and a `.deb` (or the smallest honest pair); state which and why |

Put platform code behind small traits (per crate `platform::{windows,linux}`), so the
logic stays shared and testable.

## Acceptance criteria

- `cargo test --release --workspace` is green. `cargo check` for
  `x86_64-pc-windows-msvc` is green. `cargo clippy --workspace` shows no new warnings in
  code you wrote.
- Each ported C# component has tests against C#-derived expected output:
  - JSON shapes byte-identical to `System.Text.Json`'s;
  - encryption framing round-trips against `HoverFixture` output;
  - the ACP flows replay `FakeAcp` recordings.
- The app runs on Linux under Xvfb: the notch, the office, the chat, settings and the
  tray, with screenshots in the reports.
- The benchmark is measured on Linux (S1–S6 equivalents: `Measure-Hover.ps1`'s steps
  redone as a Linux script) and reported against the frozen thresholds.
- Each phase has `port/phaseN/REPORT.md` (what was built, evidence, differences found and
  fixed, what is pending on Windows, costs of what's left). `RUN-ON-WINDOWS.md` gains
  every new Windows check.
- `port/README.md` and `port/phase0/FEATURES.md` (the checklist) are kept current.
- `port/HANDOFF.md` is rewritten at the end for the agent after you. When the session
  is running out, rewrite it early rather than lose the thread.

## Checklist (keep this list in your todo tool, tick each item as you finish it, and show it in each report)

**Setup**
- [ ] Sandbox set up (fonts, Windows target, Playwright, Xvfb); tests and Windows check green
- [ ] Read `src/Hover/**` for this phase (use a context-gatherer sub-agent); note the parts to port
- [ ] Platform trait layout decided (`platform::{windows,linux}`) and noted in `port/README.md`

**Phase 3B: persistence**
- [ ] Paths (Windows `%APPDATA%`, Linux XDG, `HOVER_DATA_DIR`), including the Noty → Hover migration
- [ ] Settings JSON: the same shapes and defaults as `System.Text.Json`, round-trip tests
- [ ] AES-GCM framing of sealed data; round-trip with `HoverFixture` output
- [ ] Key protection: DPAPI (Windows, check pending) and Secret Service / 0600 file (Linux, tested)
- [ ] Session history on disk (`Sessions.Saved`), and `kiro-images` with its 14-day cleanup
- [ ] `port/phase3/REPORT.md` 3B section; RUN-ON-WINDOWS items; push; report

**Phase 3A: sessions**
- [ ] AcpHost (process start, stdio JSON-RPC), KiroStream, KiroSession(s), the job object / process group
- [ ] Provider adapters (Kiro, Codex, Cursor): Linux paths and the Windows ones
- [ ] Replays of `FakeAcp` recordings: turns, steps, answers, stop, queue, errors, models/efforts offers
- [ ] The office `state` message built by Rust, matching `KiroPage.State`/`Push` byte-for-byte on the fixture
- [ ] chat-proto driven by a real (Fake)ACP session instead of the fixture
- [ ] Report section; push; report

**Phase 3C: product**
- [ ] Settings window (5 sections) in Slint, 1:1 with the WPF one (use the automation ids)
- [ ] Quotas (4 readers incl. SQLite) at their Windows and Linux locations
- [ ] Palette and VS Code themes; light theme
- [ ] Music (office-beats.ogg), tray, dashboard, single instance, the Alt+N hotkey and rebinding
- [ ] Notch extras: greeting animation, quota rings, done state, the alert
- [ ] Linux notch: X11 path working under Xvfb (self-test adapted); Wayland path or documented limits plus MCQs
- [ ] Report section; push; report

**Phase 2: office scene**
- [ ] The three.js scene in wgpu: voxel room, PBR with ACES, PCF soft shadows, sprites and particles, canvas textures, bots and poses, camera, picking, tags
- [ ] Glass blur behind the drawer, menu and toast (it needs the scene)
- [ ] Visual comparison against the page's Chromium captures, taken with scrollbars shown; numbers per SCREENS.md
- [ ] The full-workload benchmark on Linux against the frozen thresholds
- [ ] `port/phase2/REPORT.md`; push; report

**Phase 4: validation and packaging**
- [ ] The full benchmark and parity sweep on Linux (the Windows run stays pending, with steps written)
- [ ] UIA (Windows, pending) and AT-SPI (Linux, tested with a checker such as `accerciser`/pyatspi) mapping
- [ ] Packages: the Windows installer (from `Hover.iss`, build pending) and Linux AppImage/.deb (built and smoke-tested)
- [ ] A cutover checklist for the user (what must pass on Windows, and what passed on Linux)
- [ ] `port/phase4/REPORT.md`; README, FEATURES and HANDOFF updated; push; final report

## What not to do

- Don't mark any Windows gate as passed, and don't claim anything you didn't run.
- Don't change the C# app, `web/` or the frozen benchmark.
- Don't add CI, and don't push anywhere but `rust-port/phase-0-1`.
- Don't leave work uncommitted at the end of a feature.
- Don't swap in WebView2, Chromium or a web view for any part.
