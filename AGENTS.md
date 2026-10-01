# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A desktop app for Windows and Linux (Rust, Slint, wgpu), version 3. It has one
surface a hover away: **the notch** at the top centre of the main display (after
NotchOwl for Mac). At rest it is a slim black island: the quotas the user switched on
(each the tool's own logo in its ring), the agents at work (their logos, what the one
in front is doing, for how long), a question an agent is waiting on, or nothing.
Hovering it, clicking it or `Alt+N` opens the **Agent office**, which fills the notch. The
office hands tasks to Kiro, Codex, Cursor or OpenCode, which run headlessly, several at once,
each in a chosen folder, as bots at desks in a voxel office. The office's menu (time
of day, music, history, Settings) opens Settings over it (eight sections: General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode), with a
back button.

The only ordinary window is the dashboard: the same office in a window with Hover's
own title bar. It opens from the tray, or a second launch. The app
lives in the tray.

Up to 2.x Hover was a .NET/WPF app with the office as a web page in WebView2. 3.0 is
a port of it, made line by line. It reads everything 2.x left on users' machines: `%APPDATA%\Hover` (and the old
`Noty` folder's move), the DPAPI `note.key`, `settings.json`, `agents/*.dat`.

## Build, test, run

From the repo root.

```powershell
# Windows (PowerShell)
.\build.ps1 release run     # build and launch
.\build.ps1 test            # cargo test --release --workspace
.\build.ps1 publish         # publish\hoverai.exe
.\build.ps1 installer       # dist\Hover-Setup-<version>.exe (Inno Setup 6 or 7)
```

```sh
# Linux
make && make test
sudo make install           # PREFIX=/usr/local; DESTDIR= for staging
make package                # .deb and tarball in dist/
```

A `v*` tag (three numbers, e.g. `v3.1.0`, matching `native/Cargo.toml`) runs
`.github/workflows/ci.yml`'s tests on Windows and Linux (Ubuntu 22.04), then builds the
installers from that build and publishes them together as a GitHub pre-release.

The cross-check that Windows code still compiles, from Linux (mimalloc's C needs
clang-cl 19 or newer, which `cargo-xwin` drives with Microsoft's headers):
`cargo xwin check --manifest-path native/Cargo.toml --release --workspace --all-targets --target x86_64-pc-windows-msvc`.
CI doesn't run it: its Windows job builds on Windows.

The version is `native/Cargo.toml`'s `[workspace.package] version`; the installers
and `hover --version` read it from there.

Only one copy of Hover runs at a time. A second launch opens the running copy's
dashboard and exits. On Windows this uses the named mutex `Local\HoverRunningInstance`
(the same name 2.x used); on Linux, a lock in `$XDG_RUNTIME_DIR`.

Headless: `hover --shots DIR` renders every view with the software renderer. On a
real X display, `hover --selftest DIR` drives the notch and writes `report.json`.

## Layout

```
native/crates/
  hover-core     paths (+ the Noty move), settings.json (System.Text.Json's bytes),
                 crypto (AES-GCM; DPAPI / Secret Service key), history (sealed
                 agents/), images, single instance, palette and VS Code themes,
                 platform/{windows,linux}; bin/hover-data (data folders for tests);
                 projects.rs (projects, default workspace, voice settings),
                 secrets.rs (API keys sealed in secrets.dat)
  hover-agents   ACP host, OpenCode's server (opencode.rs, over its own http.rs), the
                 runtime both sit behind, the tools (Kiro, Codex, Cursor, OpenCode), sessions, the office's
                 state message, KiroStream, process groups / Windows jobs; route.rs
                 (voice's project routing)
  hover-quota    the four quota readers
  hover-md, hover-diagram   md.js and diagram.js, byte for byte
  hover-chat     the chat thread: layout, selection, copy, images, painter
  hover-notch    notch geometry, animation, hover rules
  hover-office   the office: scene, bots, wall canvases, camera, picking, pacing,
                 three.js 0.170's shading in office.wgsl; its own thread (live.rs)
native/apps/
  hover          the product: app, Settings (pages.rs), tray (sni.rs / win.rs),
                 notch (notch.rs, x11.rs, win.rs), office UI (office_ui.rs), music,
                 bench.rs (HOVER_BENCH), selftest, shots; voice (speech.rs, voice/,
                 voice_ui.rs), Phonon's setup and engine (phonon.rs, assets/phonon/);
                 ui/*.slint; assets/
native/tools/
  hover-measure  memory sampler, scenario runner, fake-agent (not shipped)
  notch-proto    the port's Windows notch prototype, kept for its --selftest (not shipped)
native/golden/   fixtures and expected outputs (made from the 2.x page)
native/installer/  Hover.iss (Windows), package-linux.sh
assets/          hover.png (the logo), make-icon.py (writes the app's hover.ico and
                 hover-mark.png), the README's pictures (readme/)
```

## How it works (the parts that surprise people)

- **The notch never steals focus while resting.** On Windows it is borderless,
  topmost and `WS_EX_NOACTIVATE`, with the bit taken off while the office is open.
  On X11 it is an override-redirect dock window with an ARGB visual and an XShape
  input region. On Wayland desktops Hover runs through XWayland (`HOVER_WAYLAND`
  opts out).
- **The pointer is polled, not hooked**, every 50 ms.
- **One full-size, click-through window on the main display.** The shape grows from
  its resting size to the office by animating one openness value. The window never
  resizes (that made it blink), except when the office size changes in Settings.
- **Settings sits over the office.** While it shows, the office is only hidden, so
  its sessions go on. After 30 s hidden, the office (its thread, GPU device, last
  frame) is dropped. When shown again it is made at once, and it restores the camera
  (`view`) and the open chat.
- **Colours come from one palette** (`hover-core::palette`). Hover's own light and
  dark are Apple's system colours. A VS Code theme is read from its file (following
  `include`), and only the few colour ids Hover uses are kept in `settings.json`.
  System follows the platform's dark mode. The resting notch is always black, and
  the office keeps its own look.
- **Quotas have no official API.** `hover-quota` reads what each tool exposes,
  read-only:
  - Claude Code: `api.anthropic.com/api/oauth/usage` with its own sign-in, never
    refreshed (refreshing would rotate its tokens).
  - Kiro: `kiro-cli chat --no-interactive /usage`. This is the heavy one: it takes a
    few hundred MB for about eight seconds.
  - Codex: the newest `token_count` event in `~/.codex/sessions/**/rollout-*.jsonl`.
  - Cursor: `cursor.com/api/usage-summary` with the token from Cursor's `state.vscdb`.

  Each is off until switched on, and is re-read five minutes after the last read. A
  format change shows as a readable failure, not a crash.
- **Agents run as ACP servers, never in a terminal.** Each tool is one long-lived
  hidden child, JSON-RPC over stdio, shared by all its sessions: `kiro-cli acp
  --agent-engine v3 --auth-method cli`, `codex-acp`, and `cursor-agent acp`.
  - After the tool's idle time (5 or 15 minutes with nothing running) it is shut down.
    The next reply starts it again and loads the conversation back (`session/load`).
  - Tool access, per tool and per task: Full (never asks, the default), Ask first
    (commands, deletes, moves, the network, anything outside the folder), Ask always,
    or Read only. Asking takes the tool out of its own autopilot, and
    `session/request_permission` is answered off the read loop: what the setting leaves
    alone is allowed, the rest goes to the user (`KiroSessions::ask`), shown in the
    notch (an amber island, Review opens a card: Enter allows, Shift+Enter trusts, Esc
    denies), over the bot's head and in its chat. A reply never answers it; it is
    queued behind it. Trust
    is Hover's, for the session; only Codex's own allow-always is used (Cursor's writes
    a lasting rule, Kiro's can change a setting). Codex: `workspace-write` (or its
    renamed `read-only`) for Ask first, `read-only` for Ask always, never `agent`.
  - Read only: Kiro runs with autopilot off and its write approvals are refused;
    Cursor runs in Ask mode.
  - Stop sends `session/cancel`. A tool that doesn't stop within 8 s is shut down when
    no other turn uses it; otherwise the stop is marked unconfirmed and nothing queued
    is sent. Pause cancels the turn the same way, keeps the conversation, and sends the
    next queued reply once the tool says the turn ended.
  - Prompts go over stdin, never on a command line.
  - Up to three sessions run at once across all tools, and the newest six are kept.
    An end shows in the island (the tool's logo with a badge, and the task) and as a
    system notification.
  - The tools die with Hover: a Windows job, or a process group with PDEATHSIG on Linux.
- **OpenCode runs as its own server, as T3 Code runs it** (`hover-agents::opencode`).
  One hidden `opencode serve --hostname=127.0.0.1 --port=0 --mdns=false` for all its
  sessions, with a password made for that start (in its environment, sent as Basic
  auth, never on a command line or in a URL) and `OPENCODE_ENABLE_QUESTION_TOOL=1`.
  Hover speaks plain HTTP/1.1 to it over loopback (`http.rs`, no crate, no TLS) and
  reads its event stream (`/event`). Every call names the session's folder
  (`?directory=`). OpenCode keeps its own providers; model ids are "provider/model",
  and effort is the model's own variants ("Variant" in the menus).
  - A turn subscribes first, then sends `prompt_async` with a message id Hover makes;
    only an idle after that message (or a busy for it) ends the turn. A dropped stream
    reconnects and reads the state back; a prompt whose answer was lost is looked up by
    its id, never sent twice. A resumed conversation it no longer has fails.
  - Access is per-session permission rules; the agent's own last-word denies go after
    Hover's, so Full never undoes one. Read only makes every change ask and Hover
    refuses each. Approvals answer `once` (Trust is Hover's).
  - Its question tool's questions show as choices over the bot's head (Skip, Answer…),
    in the chat (the choices, one's own answer, Skip, Answer) and in the notch (Skip,
    Review opens the chat). A reply in the chat answers a single question that takes
    one's own words. `Runtime` (runtime.rs) is what the sessions see of either kind.
- **The office's note before the first task** (`KiroNoticeSeen`) stands in place of the
  office until Got it.
- **Sessions are kept until the user deletes them.** The history is sealed with
  `note.key`: an index plus one file per session, written off the UI thread in order.
  The bookshelf and the history button list them. A reply to an old session gives it
  a desk again.
- **A new task starts from one circle** at the office's bottom left: tool logos, then
  a box for the picked tool. A draft is kept and marked with a dot. The box's pill shows
  the model, and the effort only when the tool listed efforts in its last run.
- **Voice** (`voice/`, off until switched on in Settings → Voice). Hold Ctrl+Alt+Space
  (rebindable), speak, let go. Speech is Local (Phonon, on this computer, English only)
  or Cloud (Groq, the user's key), read once per recording; Hover never falls back from
  one to the other. Optional cleanup tidies the text. Routing matches registered
  voice projects by their words; only what is left unclear goes to the default agent
  in a turn with access `none`, and anything unclear goes to the default workspace
  (home + `Hover`). A preview shows the task, folder, agent and access, then starts a new
  chat after 3 s. The agent is always the one picked in the new-task circle. Phonon
  (Python, CPU PyTorch, the model; about 1.5–1.8 GB installed) is downloaded only from
  Settings, into `<data>/phonon/`. Keys are sealed in `secrets.dat`, never in
  `settings.json`.
- **Answers are Markdown, drawn without a library** (`hover-md`, `hover-diagram`).
  Everything the agent wrote is escaped, and Mermaid flowcharts are laid out natively.
- **The office renders on its own thread with its own wgpu device** (DX12 on Windows,
  Vulkan or GL on Linux). The frame is composited on the CPU into the Slint view.
  - It redraws the shadow map only when something moved.
  - It runs at 30 fps while a bot walks or works, 10 fps when idle, 1 fps with
    animations off, and draws nothing while hidden.

## Conventions

- Match the surrounding style. Comments explain **why**, not what; keep them.
- Keep changes surgical: touch only what the task needs, and remove only the dead
  code your own change creates.
- Prefer the smallest change that works. A new crate needs its reason written in
  `Cargo.toml`.
- Verify by running the real thing: `cargo test --release --workspace` clean, and the
  Windows `cargo check` above green. For UI changes, render it (`hover --shots`), since
  a green build is not proof the pixels are right.
- Don't commit `target/`, `publish/` or `dist/`.

## Gotchas

- **Line endings are CRLF** (`.gitattributes`), except the scripts run through `#!`
  or `sh` (`native/installer/*.sh`, `Makefile`), which are LF.
- **`bin/` is git-ignored** at any depth: `native/crates/hover-core/src/bin/hover-data.rs`
  is tracked with `git add -f`.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky; anchor
  on unique ASCII lines.
- **One renderer per process**: femtovg (GL) on Linux, femtovg-wgpu DX12 on Windows.
- **Headless GPU runs** need `XDG_RUNTIME_DIR` set.
- **The chat's layout goldens** (`hover-chat`'s `thread.rs`) were measured in Linux
  Chromium with DejaVu Sans, so the three that compare text widths skip on Windows.
