# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A desktop app for Windows, Linux and macOS (Go: Gio draws the interface and wgpu-native the office on
Windows and Linux; on a Mac, a Swift app around a Go backend). It has one
surface a hover away: **the notch** at the top centre of the main display (after
NotchOwl for Mac). At rest it is a slim black island: the quotas the user switched on
(each the tool's own logo in its ring), the agents at work (their logos, what the one
in front is doing, for how long), a question an agent is waiting on, or nothing.
Hovering it, clicking it or `Alt+N` (Option-N on a Mac) opens the **Agent office**, which fills the notch. The
office hands tasks to Kiro, Codex, Cursor, OpenCode or Claude Code, which run headlessly, several at once,
each in a chosen folder, as bots at desks in a voxel office. A click on a desk opens its
**desk card** (what the agent is doing, and panels for its terminal, files, diff, pull
request, browser and screen). The office's menu (time
of day, music, history, Settings) opens Settings over it (ten sections: General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode, Claude Code, Antigravity), with a
back button.

The only ordinary window is the dashboard: the same office in a window with Hover's
own title bar (File, Settings and Help menus; the system's on a Mac). It opens from the tray (the menu bar on a Mac), or a second launch. The app
lives in the tray.

On a Mac the usage rings are one status item in the menu bar instead of the island (the
camera housing leaves no room), and a Mac without a notch gets a notch-sized pill. The macOS
port follows Arz's (@Entourage397) macOS v1.0, which was Swift and C#; here the Mac app is
Swift UI (`macos/`: notch, menu bar, Settings, voice) around the web office (`web/office/`),
on the Go backend (`cmd/hover-backend`, `internal/backend`) that the Windows and Linux app
does not link.

Up to 2.x Hover was a .NET/WPF app with the office as a web page in WebView2. 3.0 is
a port of it, made line by line, in Rust. The code is Go now; the last of the Rust is the git tag
`rust-final`. It reads everything 2.x left on users' machines: `%APPDATA%\Hover` (and the old
`Noty` folder's move), the DPAPI `note.key`, `settings.json`, `agents/*.dat`.

## Build, test, run

From the repo root. The version is the number in `VERSION`. The build scripts put it into the
program (`hoverai --version` prints it), and CI checks that it equals the default in
`internal/shell/version.go`.

```powershell
# Windows (PowerShell). Go only: no C compiler, no Visual Studio.
.\build.ps1 run             # build and launch
.\build.ps1 publish         # publish\hoverai.exe and wgpu_native.dll
.\build.ps1 installer       # dist\Hover-Setup-<version>.exe (Inno Setup 6 or 7)
.\build.ps1 test            # the Go tests
```

```sh
# Linux (a C compiler and the EGL headers: Gio draws through EGL)
make && make test           # ./hover-linux; tags nowayland,nox11,novulkan
make wgpu                   # lib/libwgpu_native.so, which the office draws with
sudo make install           # PREFIX=/usr/local; DESTDIR= for staging
make package                # .deb and tarball in dist/
```

```sh
# macOS (Apple Silicon by default, HOVER_ARCH=x64 for Intel; Xcode's command line tools, Go,
# and node/npm for the office page)
scripts/build-macos.sh                              # dist/macos-osx-arm64/Hover.app, signed ad hoc
tests/macos/backend-smoke.py <Hover.app> <sandbox>  # the packaged backend against a stand-in agent
tests/macos/e2e/run.sh <Hover.app>                  # E2E with stand-in agents, gh, cua and lume
```

`.github/workflows/ci.yml` runs on every push to `main` and `go-port` and on every pull request
(a push that only changes `.md` files, `docs/` or `assets/readme/` is skipped). It runs no Go
tests. `check` tests formatting, vets the Go code for Windows, Linux and macOS, and compiles the
Mac backend for both Macs. `windows` builds `hoverai.exe` and the installer, drives the app
(starts it, opens the notch with Alt+N, folds it with Esc, asks for the app window with a
second launch) and installs the new setup over the 5.x release. `linux` builds the `.deb` and
tarball (the runner has no Wayland desktop to run them on). `macos` (macos-15, Apple Silicon)
builds `Hover.app`, signed ad hoc, and runs its packaged backend against stand-in tools.
`pictures` draws every view on Windows; it runs only by hand (Run workflow).

A push to `main` whose `VERSION` has no tag yet, or a `v*` tag matching it, is a release: the
same jobs make the installers, and `release` tags the commit and publishes them together as the
Latest GitHub release. Raising the version releases it. A version whose CHANGELOG.md heading
ends in `(nightly)` is published as a pre-release and is not made the Latest. A release also
builds Intel and puts both Macs in disk images (`Hover-<version>-macos-arm64.dmg`, `-x64.dmg`),
and `release` waits for them. `scripts/package-macos.sh` signs with a Developer ID and notarizes.

Any machine can check the code for the other systems, which is what `check` does:
`GOOS=windows go vet ./cmd/... ./internal/...`, `GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go vet
./cmd/... ./internal/...`, and `go vet -tags nowayland,nox11,novulkan ./cmd/... ./internal/...`
for Linux (that one needs the EGL headers).

Only one copy of Hover runs at a time. A second launch opens the running copy's
dashboard and exits. On Windows this uses the named mutex `Local\HoverRunningInstance`
(the same name 2.x used); on Linux, a lock in `$XDG_RUNTIME_DIR`; on a Mac, `internal/core`
keeps a lock and a socket in `$TMPDIR` (else `/tmp/hover-<uid>`), which the Swift app does
not use. On Linux `hover --toggle` opens or folds the notch of the running copy (for a
compositor with no shortcut portal: bind a key to it).

Headless: `hoverai --shots DIR` (`hover` on Linux, built with `-tags shots`; it needs cgo, Mesa's
EGL and `EGL_PLATFORM=surfaceless`) draws every view to PNGs. `cmd/ui-shots` is the same without
the product. The office pictures need `HOVER_SHOTS_OFFICE=1` and wgpu-native;
`HOVER_SHOTS_SKIP=voice,chat` leaves groups out. On Windows, `tools/app-smoke.ps1` drives the
real app and writes `report.json`. On a Mac, `open -a Hover --args --settings <page>` opens
Settings on a page (`start`, `general`, `usage`, `computer-use`, `voice`, or an agent: `codex`,
`kiro`, `cursor`, `opencode`, `claude`). `Hover --smoke-test` is the Mac app's own smoke mode
(`macos/Sources/Hover.swift`).

## Layout

```
VERSION          the version number, in one place
go.mod, go.sum   the Go module, github.com/4regab/Hover
cmd/
  hover          the product (binary hoverai on Windows, hover on Linux): main_windows.go,
                 main_linux.go, shots_linux.go
  hover-backend  the Mac app's backend: internal/core, agents and quota behind JSON lines on
                 stdin and stdout
  hover-data     writes and reads a data folder, to check that the files round-trip
  ui-shots       draws the pictures, for a build without the product
  office-shot    draws the office alone
  notch-spike    the first notch prototype, on Windows (not shipped)
internal/
  core           paths (+ the Noty move), settings.json (json.go: System.Text.Json's bytes; keys of
                 dropped features are skipped and not written back), crypto (AES-GCM; DPAPI /
                 Secret Service key), history (sealed agents/), store.go (the store, secrets.dat
                 and images), single instance, palette and VS Code themes, platform_*.go (per
                 system), macos.go (Keychain / LaunchAgent / `defaults` logic, compiled on every
                 system so any system's tests cover it), projects.go (projects, default
                 workspace, voice settings)
  agents         ACP host (acp.go), checkpoints (checkpoint.go), OpenCode's server (opencode.go,
                 over its own http.go), Claude Code's SDK mode (claude.go), the runtime they sit
                 behind (runtime.go), the tools (Kiro, Codex, Cursor, OpenCode, Claude Code),
                 sessions, the office's state message (state.go), KiroStream (stream.go), process
                 groups / Windows jobs (proc_*.go); route.go (voice's project routing); sandbox.go
                 (srt), computer_use.go (Cua Driver), spaces.go (Cua Spaces: the agents' desktops,
                 their `cua` calls), browser.go (the agent browser's MCP server and socket),
                 discord.go (the Discord status: Settings → Integrations → Show on Discord, off
                 by default; Discord's local socket or pipe, no sign-in), setup.go (one-click
                 agent install and sign-in), github.go (gh: status, install, sign-in), desk.go
                 (what the desk card and its panels read: git, gh, terminal, files, diff, pull
                 requests, subagents, pages), term.go (the user's own shell in the Terminal
                 panel), orch.go (helpers: a task asking other agents for help, under fixed
                 limits), workspace.go (what Git says about a task's folder, and the holds a
                 checkpoint restore takes), editor.go (finds the editors on this computer and
                 opens a folder or file in one), mcp.go (Kiro's MCP servers: reads and edits
                 ~/.kiro/settings/mcp.json for Settings → Kiro)
  quota          the four quota readers
  backend        the Mac app's backend: core, agents and quota behind JSON lines (wire.go,
                 backend.go), the state message the web office reads (office.go), desk panels
                 (panels.go), prefs, quotas, screen; browser_host.go (the agent browser's calls,
                 relayed to the Swift app)
  md, diagram    md.js and diagram.js, byte for byte
  chat           the chat thread: layout, selection, copy, images, painter (on raster and text)
  raster, text   the pixel painter under the chat (rounded rectangles, glyph outlines, images)
                 and text shaping and line breaking
  notch          notch geometry, animation, hover rules (no windowing)
  office         the office: scene, bots, helpers (mini.go: subagents as small bots at their
                 parent's desk), wall canvases, camera, picking (bots and desks), pacing,
                 three.js 0.170's shading in office.wgsl; its own thread (live.go)
  gpu            Hover's own small binding to wgpu-native (no C compiler on Windows)
  ui             the interface on Gio: palette, icons, marks, controls; Settings, the notch,
                 the office views, the desk card and panel (deskcard.go, deskpanel.go), the chat
                 view, the app window's title bar (dashboard.go); assets/ (fonts, logos)
  app            the app without a window: Settings as data (pages.go, view.go), the data folder
                 and the old service's removal (hover.go), the resting notch (rest.go), keys.go
  shell          the glue: the notch's window (notchctl.go), the office, chat, desk, new task,
                 voice and Settings hooks; env_windows.go and env_linux.go bind the system
  platform       win (Win32 windows, Direct3D 11, DirectComposition, tray), linux (D-Bus: tray,
                 notifications, portals, global shortcuts, pickers), wayland (Hover's own
                 Wayland client: the notch as a layer-shell surface, the app window, keyboard,
                 input method)
  voice          capture, speech (Local: Phonon; Cloud: Groq), cleanup, Phonon's setup and
                 engine (phonon*.go, assets/)
  audio, music   the system's audio output, and the office's beats
  screen         the Screen panel's capture (Windows) and voice's whole-display screenshot
  shots          the pictures (`--shots`)
third_party/gioui.org   Gio, patched; third_party/README.md says how
macos/           the Mac app, in Swift: Hover.swift (entry, hot keys, pointer),
                 Notch.swift (the notch window and island), MenuBar.swift, Settings.swift,
                 Voice.swift and VoicePanel.swift, OfficeHost.swift (the web office in a
                 WKWebView and the backend's pipe), AgentBrowser.swift, Spaces.swift,
                 Screen.swift, ShellEnvironment.swift, guardian.c (starts the backend);
                 entitlements, Resources/ (the office's music)
web/office/      the web office (main.js, desk.js, page.html; esbuild bundles it into
                 dist/kiro-office.html, not tracked); the Mac app's office
scripts/         build-macos.sh, package-macos.sh (Developer ID + notarize)
tools/           app-smoke.ps1, installer-check.ps1, set-resolution.ps1 (what CI runs on Windows)
tests/golden/   fixtures and expected outputs (made from the 2.x page)
tests/macos/    backend-smoke.py, SettingsSmoke.swift, e2e/ (run.sh, E2E.swift, the stand-in
                 agent, cua, lume and site)
packaging/       windows/Hover.iss; linux/package-linux.sh and hover.desktop (the one
                 .desktop file the .deb and make install both use)
assets/          hover.svg (the logo), make-icon.py (writes hover.png and
                 the app's hover.ico and hover-mark.png), the README's pictures (readme/)
```

## How it works (the parts that surprise people)

- **The notch never steals focus while resting.** On Windows it is borderless,
  topmost and `WS_EX_NOACTIVATE`, with the bit taken off while the office is open.
  On Linux it is a layer-shell surface at the top of the display, see-through and not
  taking the keyboard unless asked (Hover speaks Wayland itself: no X11, no XWayland; a
  compositor with no layer-shell, GNOME's own for one, can't place it). On a Mac it is an NSPanel (`Notch.swift`, non-activating) at status level
  (25) on all Spaces, whose `canBecomeKey` is false while the notch rests (true once the
  office opens); a click on it never activates Hover. Mouse events pass through except
  over the shape (`ignoresMouseEvents` follows the polled pointer).
- **The Mac's notch and menu bar** (`macos/Sources/Notch.swift`, `MenuBar.swift`,
  `Hover.swift`). The housing comes from `safeAreaInsets` and `auxiliaryTopLeftArea` /
  `TopRightArea` of the built-in display with a notch (else the first screen; a Mac without
  a notch gets a 180-pt pill), as `NotchGeometry`. The island puts its items in wings
  either side of the housing, and the office starts below it. Usage is not in the island on
  a Mac: one status item in the menu bar shows a logo in a ring and the percentage for each
  reader switched on (Codex, Kiro, Cursor), and opens one menu. The shortcuts are Carbon hot
  keys (Option-N, Control-Option-Space, and Esc only while voice listens) plus a local key
  monitor, because the window server gives keys to the Hover window that has the keyboard and
  the hot key never fires there. They are fixed; Settings shows them as text. An app
  started from Finder has a bare PATH, so the login shell's environment (`$SHELL -ilc`) is
  merged in before the backend starts (`ShellEnvironment.swift`).
- **The pointer is polled, not hooked**, every 50 ms (on a Mac, `NSEvent.mouseLocation`).
- **One full-size, click-through window on the main display.** The shape grows from
  its resting size to the office by animating one openness value. The window never
  resizes (that made it blink), except when the office size changes in Settings.
- **Settings sits over the office.** While it shows, the office is only hidden, so
  its sessions go on. After 30 s hidden, the office (its thread, GPU device, last
  frame) is dropped. When shown again it is made at once, and it restores the camera
  (`view`) and the open chat.
- **Colours come from one palette** (`internal/core/palette.go`). Hover's own light and
  dark are Apple's system colours. A VS Code theme is read from its file (following
  `include`), and only the few colour ids Hover uses are kept in `settings.json`.
  System follows the platform's dark mode. The resting notch is always black, and
  the office keeps its own look.
- **Quotas have no official API.** `internal/quota` reads what each tool exposes,
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
    alone is allowed, the rest goes to the user (`KiroSessions.Ask`), shown in the
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
  - **Models are Kiro's to say, not Hover's** (`discover.go`). Nothing lists them by hand. Kiro
    answers `session/new` before it knows its models (`network.listAvailableModels` finishes a few
    hundred ms later) and sends them as a `config_option_update`, so a run that picked a model waits
    for them (`awaitModels`), and `Runtime.Discover` reads them without a task: one throwaway
    session in an empty temp folder, deleted again with `session/delete` (`session/close` where
    that is all the tool has). The rate on each model is Kiro's `_meta.kiro.rateMultiplier`.
    `SetAgentOffers` keeps a saved model list when a later answer has none. Antigravity is never
    started for it (about 1 GB at each start); OpenCode and Claude Code list theirs at a run.
  - Up to three sessions run at once across all tools, and the newest six are kept.
    An end shows in the island (the tool's logo with a badge, and the task) and as a
    system notification.
  - The tools die with Hover: a Windows job, or a process group with PDEATHSIG on Linux.
- **OpenCode runs as its own server, as T3 Code runs it** (`internal/agents/opencode.go`).
  One hidden `opencode serve --hostname=127.0.0.1 --port=0 --mdns=false` for all its
  sessions, with a password made for that start (in its environment, sent as Basic
  auth, never on a command line or in a URL) and `OPENCODE_ENABLE_QUESTION_TOOL=1`.
  Hover speaks plain HTTP/1.1 to it over loopback (`http.go`, no library, no TLS) and
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
    one's own words. `Runtime` (`runtime.go`) is what the sessions see of every kind.
- **Claude Code runs in its Agent SDK mode, as T3 Code runs it** (`internal/agents/claude.go`):
  `claude --output-format stream-json --verbose --input-format stream-json
  --permission-prompt-tool stdio --include-partial-messages`, JSON lines both ways and
  its control protocol, which is what `@anthropic-ai/claude-agent-sdk`'s `query()` starts.
  One hidden process per conversation (as the SDK runs one per query), started in the
  session's folder (it has no `--cwd`), kept for its replies, and shut down after the
  idle time; at most three are kept, the least recently used idle one goes first. A reply
  after that starts it again with `--resume=ID`; one it no longer has starts a new
  conversation and says so under the answer. `--setting-sources=user,project,local`, so
  the user's CLAUDE.md, permissions, hooks and MCP servers apply.
  - Hover sends `initialize` (its models, each with its efforts, fill Settings) and
    `interrupt` (Stop; one that hasn't ended the turn in 8 s has its process ended).
    Every `can_use_tool` is Hover's to answer, on its own thread.
  - Access: Full is `bypassPermissions`. Ask first and Ask always are its `default`
    mode, where everything past its own read-only checks asks Hover, and
    `NeedsAsking` (`ask.go`) decides what reaches the user. Read only is `--disallowedTools`
    for its edit and command tools, the rest refused; voice's routing turn (access
    `none`) gets `--tools ""`. Trust is Hover's: its own suggestions would write a rule
    into the user's settings.
  - AskUserQuestion shows as OpenCode's questions do; the answers go back by each
    question's own text (as Claude Code looks them up). What it says (partial messages,
    tool calls and results, thinking) is put into ACP's shapes and read by `KiroStream`.
- **Kiro Web sessions** (`acp.go`, the `cloud` repositories of `KiroSessions.StartIn`; Windows and Linux, not the Mac
  app yet). A Kiro task can run in Kiro's cloud instead of on this computer: the
  new-task box's cloud button (Kiro only) and the voice preview's. It goes through the
  same `kiro-cli acp` process: `session/new` with `_meta.kiro.executionTarget`
  `{kind: "cloud-sandbox"}` and `repositories` (`[{providerType: "GITHUB", name}]`; none
  is an empty workspace). Neither field is in Kiro's docs; both were read from its agent
  server (Oct 2026), which advertises the cloud in `initialize`'s
  `_meta.kiro.executionTargets`.
  - The repo is the folder's GitHub remote (`Desk.GithubRepo`), or one picked from
    `_kiro/sourceProviders/listResources`, or none. Access is always Full (the cloud has
    no asking), the session gets none of this computer's MCP servers, and it has no
    checkpoints. The desk card's Terminal and Files are off (`CloudNote`). Its Pull request and
    Diff work from the chat, not this folder (`DeskSnap.Cloud`): the PR it created
    (`CreatedPR`), else one it mentions in its own repos (`MentionedPR`); Diff is that PR's
    `gh pr diff`, or the edits it reported before it has one. A local chat's Pull request
    tab takes the created PR first too, then its branch's, then a mentioned one.
  - The first prompt waits for the sandbox: one sent before its first `context_usage`
    update (about 15 s) is answered `cancelled` here while the cloud still runs it.
  - A reply after Kiro's process restarted loads the session with
    `_meta.kiro.sessionSource: "remote"`; without it Kiro makes an empty local session of
    the same id. A cloud session that can't be loaded fails; another is never started
    in its place.
  - The chat's cloud chip opens it in Kiro Web (`KiroWebSession` + the session id).
  - Saved in the history as `"Cloud"` (its repos), written only for cloud sessions.
- **The chat view** (the switch at the office's top left, `Office.d-wide`, `Shell.setChatView`).
  A chat app in place of the office, in the notch and the app window alike: the sessions down
  the left (the office/chat switch, New chat, the chats grouped by project folder; its hide button shows
  on hover, and a closed sidebar leaves a show button before the title), the open chat, or with none open a start
  screen (the agents, then one box with folder, access and model). The reply box rests as a
  circle at the chat's corner (a dot when a draft waits) and opens on a click or a typed key;
  Esc (after any menu), or sending, closes it. Open, it has +, the chat's folder, the model and
  one round button (grey, Stop while the agent works, Send to queue once something is typed).
  `@` lists the folder's files (a chip that sends the path, not the contents), `/` lists the
  agent's own commands (ACP's `available_commands_update`, kept on the session) and Hover's.
  The office's chat card uses the same header (title, context, ⋯, ✕) and reply box. Kept in `settings.json`
  (`ChatView`, written only while on), so the notch opens on it until the switch goes back;
  Esc and the notch folding never leave it. The office draws nothing under it, and is made
  only when switched back to. A chat's Expand button switches it on.
- **The office's note before the first task** (`KiroNoticeSeen`) stands in place of the
  office until Got it.
- **Sessions are kept until the user deletes them.** The history is sealed with
  `note.key`: an index plus one file per session, written off the UI thread in order.
  The bookshelf and the history button list them. A reply to an old session gives it
  a desk again.
- **A new task starts from one circle** at the office's bottom left: tool logos, then
  a box for the picked tool. A draft is kept and marked with a dot. The box's pill shows
  the model, and the effort only when the tool listed efforts in its last run.
- **Voice** (`internal/voice/`, off until switched on in Settings → Voice). Press Ctrl+Alt+Space to start
  and again to finish (Settings' Voice Recording Mode: Toggle, the default, or Hold to speak, where
  letting go finishes). Saying "take a screenshot" attaches a picture of the screen (`screen.Whole`)
  to the task: with Cloud speech each pause's stretch is read on its own while recording
  (`Pauses`, `TakeScreenshots`), with Local at the end; the phrase is always taken out. The
  shortcut is rebindable. Speech is Local (Phonon, on this computer, English only)
  or Cloud (Groq, the user's key), read once per recording; Hover never falls back from
  one to the other. Optional cleanup tidies the text. Routing matches registered
  voice projects by their words; only what is left unclear goes to the default agent
  in a turn with access `none`, and anything unclear goes to the default workspace
  (home + `Hover`). A preview shows the task, folder, agent and access, then starts a new
  chat after the countdown (Settings → Voice, 5 s by default; Off waits for Start). The agent is
  always the one picked in the new-task circle. Over an open chat whose reply box is open,
  the shortcut dictates instead: the words go into the reply (`StageDictated`), nothing is routed. Phonon
  (Python, CPU PyTorch, the model; about 1.5–1.8 GB installed) is downloaded only from
  Settings, into `<data>/phonon/`. Keys are sealed in `secrets.dat`, never in
  `settings.json`. On a Mac voice is the Swift app's (`macos/Sources/Voice.swift`,
  `VoicePanel.swift`, fixed Control-Option-Space): Apple's speech recognizer on the Mac
  (SpeechAnalyzer on macOS 26+, else SFSpeechRecognizer), with no Phonon, Groq or cleanup.
- **Checkpoints** (`internal/agents/checkpoint.go`). Before and after every turn Hover keeps the
  project folder in a shadow git store of its own, `<data>/checkpoints/<session key>.git`,
  with the project as its work tree: the project's own `.git` is never read or written,
  `.gitignore` decides what is left out, and a checkpoint is a tree id (saved in the
  history as `CheckpointBefore` / `CheckpointAfter` on the turn). Needs `git` on PATH;
  without it there are none. A whole drive or the home folder is refused, and a folder that
  takes over 90 s is given up on for that chat. **Restore** (under an earlier answer) puts
  the folder and the chat back to just after that answer; **Try again** (under any answer)
  puts them back to before that message and sends it again (`KiroSessions.Rewind`,
  `Rewind`). Both ask first, and only when nothing runs. The agent still
  remembers the removed turns, so its next message carries one note that the folder and chat
  went back (before the very first message it starts a new conversation). The folder as it
  was just before a restore is kept in the store (`UndoTree`). Deleting a chat deletes its
  store. **Retry** is the older button: the newest prompt again, files untouched.
- **The sandbox** (`internal/agents/sandbox.go`, Settings → Integrations, on by default). Each
  tool is started under Anthropic's sandbox-runtime (`srt`, pinned in `SandboxVersion`):
  sandbox-exec on a Mac, bubblewrap on Linux. It writes only to the folders its sessions
  work in, its own state and caches, and temp; keys, keychains, mail and other apps' data
  can't be read; there is no window server and no Apple Events; the network goes through
  srt's proxy to the tool's service, package registries and GitHub (more in
  `<data>/sandbox/allowed-domains.txt`). The folders are fixed when the tool starts, so a session in
  another folder gets the tool started again (when nothing of it runs). The settings file
  for srt is text built by pure functions (`SandboxConfig`, `SrtArgs`) that the tests run on every OS.
  If `srt`, `rg` (and on Linux `bwrap`, `socat`) is missing, or Hover is already inside a
  sandbox (`HOVER_SANDBOXED=1`), the tool starts as before and `hover.log` says why;
  Settings shows what is missing (`SandboxMissing()`). Off on Windows: srt's Windows
  support can't reach tools installed for the user.
- **Computer use** (`computer_use.go`, off until switched on). Hands every agent Cua
  Driver's MCP server (`cua-driver mcp`), which drives other apps in the background.
  Where perl exists it starts behind Hover's guard, which turns foreground input into
  background input and refuses desktop-wide input, raising a window, the clipboard and the
  like, with a note the agent reads. Cua's tools are MCP calls, so Ask first and Read only
  treat them as any other. Settings installs CuaDriver and asks it for Accessibility and
  Screen Recording (the grants go to CuaDriver, not Hover). macOS only: Cua is kept to the
  Mac, so Windows and Linux show the switch off with `CuaUnsupported`.
- **Agent desktops, Cua Spaces** (`spaces.go`; the messages in `internal/backend`; the Swift
  side in `macos/Sources/Spaces.swift`, `Hover.swift` and `Notch.swift`). Off until switched
  on in Settings → Computer Use, and macOS 26+ on Apple silicon only (`SpacesUnsupported`).
  Each project folder gets one Space, a macOS VM (Cua's `cua` CLI with Lume) that the agents
  working there share for computer use instead of the user's screen. It is made when the
  project's first agent starts, turned off when none of its agents is in the office, after
  15 idle minutes and when Hover quits, and deleted with the project's last session. The
  Screen panel shows its viewer. Drag an app or files onto the notch and the office opens
  on the desktops to send them to; the full-screen button opens Hover in a window.
- **The agent browser** (`browser.go` + `internal/backend/browser_host.go` +
  `macos/Sources/AgentBrowser.swift`, macOS only). Each agent
  gets a browser MCP server (12 tools: open, snapshot, click, type, …) that talks over a
  user-only Unix socket (0600, in a 0700 folder, with a token sent in the server's
  environment, never on a command line) to Hover, which drives a WKWebView per session:
  the backend relays each call to the Swift app as a `browser` message.
  The page is shown in the desk card's Browser tab. OpenCode has one server for all its
  sessions, so its calls go to the session at work.
- **Setting an agent up** (`setup.go`, macOS). The "Set up" row on an agent's page installs
  what is missing with the maker's own installer and then runs the tool's sign-in in a
  Terminal window; Hover never sees the credentials.
- **The desk card** (`internal/ui/deskcard.go` and `deskpanel.go`, driven from `internal/shell/desk.go`; data from
  `internal/agents/desk.go`). A click
  on a desk with a session opens a card where you clicked: the last steps, the question
  the agent waits on, or the answer, a reply box, and eight tiles that open a wide panel:
  Terminal, Files, Diff, Agents, Linked PRs, Pull request, Browser, Screen. Every git and
  gh call blocks, so it runs on a worker. Files shown stay inside the session's folder (links followed).
  A tile the OS can't run is disabled on the card with its reason (`TileContext.Off`); in the
  panel the same tab is hidden, not greyed (the one open stays while it is open). The panel
  sits at the chat's right edge at the chat's full height in the chat view, and floats over
  the office otherwise. Its tabs each show their name and scroll sideways, with a fade at the
  right and the close button fixed.
- **Files & changes, Files tab** (`internal/shell/desk.go`, `internal/ui/deskpanel.go`). The tree only; what changed is the Diff
  tab's, and a changed file (or one saved here) shows its letter. A `.md` file opens as a
  preview (`internal/chat` paints it, as it does a pull request's description) with a Preview /
  Markdown switch. Edit is for every file that `DeskFileText` showed whole and as UTF-8: a box
  in place, Cancel and Save (Ctrl+S saves, Esc cancels), and an amber line, not a block, while
  the agent works in the folder. `DeskWriteFile` saves inside the session's folder, through
  a temp file renamed over the original. Open in lists only the editors `AvailableEditors()`
  finds, then the file manager; the last used is first and marked (`LastEditor` in
  `settings.json`).
- **The Terminal tab** has two tabs. The agent's is its commands from the session's steps,
  read only. "My commands" is the user's own shell (`internal/agents/term.go`): PowerShell on
  Windows, bash elsewhere, one long-lived process per chat started in the chat's folder, run as
  the user and outside the agents' sandbox. A command goes to its stdin as base64 inside a
  fixed wrapper (never pasted into a command line or into the shell's syntax), and the wrapper
  prints a marker with the exit code and the folder, so `cd` lasts. Ctrl+C ends the shell and
  starts a new one in the same folder. A command that waits for typed input gets none.
- **The pull request tab** sets up the GitHub CLI in one click (`github.go`): install with
  winget or Homebrew where there is one (else a hint: Hover never uses sudo), then
  `gh auth login` with the device code shown to copy and the page to open. Create pull
  request can commit, make a branch, push and open the PR; it is disabled with the reason
  while the agent runs.
- **Subagent helpers** (`internal/office/mini.go`). A subagent at work (kind `agent`, or a Kiro
  or Codex step titled like one, `IsSubagent` in `state.go`) shows as up to four small recoloured
  bots beside its parent's desk, which hop out and back and file sheets at the tray. They
  keep the office at 30 fps while they exist; the card says how many are out.
- **The Screen panel** (`internal/screen`; on a Mac `macos/Sources/Screen.swift`) is a still of
  the desktop, and about twice a second while computer use runs, the windows of the apps
  the agent used (PrintWindow on Windows, ScreenCaptureKit on a Mac, which
  needs Hover's own Screen Recording grant; not on Linux, where Wayland shows no other
  program's windows).
- **Answers are Markdown, drawn without a library** (`internal/md`, `internal/diagram`).
  Everything the agent wrote is escaped, and Mermaid flowcharts are laid out natively.
- **The office renders on its own thread with its own wgpu device** (wgpu-native: DX12 on Windows,
  Vulkan or GL on Linux). The frame is read back and shown as an image in the Gio view. On a Mac
  the office is the web page (`web/office/`, three.js) in a WKWebView.
  - It redraws the shadow map only when something moved.
  - It runs at 30 fps while a bot walks or works, 10 fps when idle, 1 fps with
    animations off, and draws nothing while hidden.

## What each OS can't run

A Settings switch for a feature the OS can't run stays in Settings, shown off, with the reason
beside it. The tabs of Files & changes (Terminal, Files, Diff, Agents, Linked PRs, Pull request,
Browser, Screen) are the other way round: a tab this computer can't use is hidden, not greyed out.
The notes are constants next to the code (`SandboxUnsupported`, `BrowserUnsupported`,
`SetupUnsupported`, `CuaUnsupported`, `SpacesUnsupported`). In the table, "off" is the
Settings switch.

| Feature | Windows | Linux | macOS | Why |
|---|---|---|---|---|
| Notch, office, desk card, panels (Terminal, Files, Diff, Agents, PRs) | yes | yes, on a compositor with layer-shell | yes | GNOME's own compositor has no layer-shell, so Hover can't place the notch there |
| Sandbox | off: "The sandbox needs macOS or Linux." | yes, if `srt`, `rg`, `bwrap`, `socat` are there | yes, if `srt` and `rg` are there | srt's Windows support can't reach tools installed for the user |
| Agent browser, Browser tab | off: "Agent browser needs macOS." | off, same note | yes | the page is a WKWebView |
| One-click agent setup | off: "One-click setup is available on macOS." | same | yes | the installers and sign-in scripts are written for a Mac |
| Computer use | off: "Computer use needs macOS." | off, same note | yes | Cua Driver is built for macOS; Cua is kept to the Mac |
| Agent desktops (Cua Spaces), drag to the notch | off: "Agent desktops need macOS 26 or later on Apple silicon." | off, same note | yes, on macOS 26+ Apple silicon with Cua's CLI and Lume | Cua Spaces runs macOS VMs with Lume; Cua is kept to the Mac |
| Screen panel capture | yes (PrintWindow) | off: "The screen panel needs Windows: a Wayland desktop doesn’t let one app see another’s windows." | yes, with Screen Recording | Wayland shows no other program's windows (voice's "take a screenshot" still works, through the Screenshot portal) |
| GitHub CLI one-click install | winget | hint only | Homebrew, else hint | no sudo, no release download (no TLS in `internal/agents`) |
| Local speech (Phonon) | yes | yes | off: Apple's on-device speech recognizer instead (no Groq or cleanup) | Phonon's runtime isn't built for a Mac |
| Usage in the island | yes | yes | in the menu bar instead | the camera housing |
| Tray | yes | yes (D-Bus) | menu bar | |

## Conventions

- Match the surrounding style. Comments explain **why**, not what; keep them.
- Keep changes surgical: touch only what the task needs, and remove only the dead
  code your own change creates.
- Prefer the smallest change that works. A new module in `go.mod` needs its reason in the
  commit message. The Windows build needs no C compiler: `GOOS=windows go build` must work
  from any machine.
- Verify by running the real thing: format, build and vet for all three systems (the commands
  above; `check` in CI does it), and for the Windows app `tools/app-smoke.ps1`. For UI changes,
  render it (`hoverai --shots`), since a green build is not proof the pixels are right. Nothing
  of the macOS behaviour has been run by the people who wrote it (see docs/MACOS.md): say what
  you ran and what you didn't.
- Don't commit `publish/`, `dist/`, `lib/` or the built program (`hover-linux`).
- Say which system a file is for: a `_windows.go`, `_linux.go` or `_darwin.go` name or a
  `//go:build` line, and give a Mac its own file or an honest "not on macOS" branch. Logic that
  only runs on a Mac (argument lists, parsing, text) goes in functions compiled everywhere, so
  tests on any system cover it (`internal/core/macos.go` is one).

## Gotchas

- **Line endings.** `.gitattributes` makes text files CRLF in the working tree, except the Go
  files (`*.go`, `go.mod`, `go.sum`: gofmt writes LF), `VERSION` (a CR after the number breaks the
  scripts), the scripts run through `#!` or `sh`, `Makefile`, and `packaging/linux/*`, which are LF.
- **Run Go commands on `./cmd/... ./internal/...`, not `./...`.** `web/office/node_modules` can
  hold `.go` files that are not part of Hover.
- **Comments that name a `.rs` file or a crate** (`store.rs`, `hover-core`) point at the Rust
  the Go was ported from. It is gone from the tree; read it with
  `git show rust-final:crates/hover-core/src/store.rs`.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky; anchor
  on unique ASCII lines.
- **Windows windows share one Direct3D 11 device** (`internal/platform/win/gfx.go`): each extra
  one cost 100 to 200 MB. The office has its own wgpu device on its own thread.
- **A Mac app started from Finder has no shell PATH.** Anything that looks for a tool
  (`agents.Find`, `OnPath`) sees the login shell's PATH only after `ShellEnvironment.swift`
  merged it in (the backend starts after).
- **`Shortcut.Label()` reads ⌥N on a Mac**, not Alt+N; tests that want the Windows text
  use `LabelFor(false)`.
- **Dropped features leave quiet leftovers.** Agents of your own, the ACP Registry, task worktrees, the
  default editor, helper limits, pull request watches, continuing at a usage limit's reset, saved tasks,
  the background service and webhooks are gone. Their keys in `settings.json` are read past and not
  written back; the data they kept in the data folder (saved tasks, own agents, worktrees, timers) is
  left on disk and nothing reads it. A chat made with an own agent still opens; a reply to it says the agent is gone.
  The first start after the update removes the old service's scheduled task (Windows) or systemd user
  unit (Linux) once (`removeOldService` in `internal/app/hover.go`) and writes one line to `hover.log`.
- **Headless GPU runs** need `XDG_RUNTIME_DIR` set.
- **The chat's layout goldens** (`internal/chat`'s `thread_test.go`) were measured in Linux
  Chromium with DejaVu Sans, so the three that compare text widths skip on Windows.
