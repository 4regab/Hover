# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A desktop app for Windows, Linux and macOS (Rust, Slint, wgpu on Windows and Linux; Swift on a Mac), version 3. It has one
surface a hover away: **the notch** at the top centre of the main display (after
NotchOwl for Mac). At rest it is a slim black island: the quotas the user switched on
(each the tool's own logo in its ring), the agents at work (their logos, what the one
in front is doing, for how long), a question an agent is waiting on, or nothing.
Hovering it, clicking it or `Alt+N` (Option-N on a Mac) opens the **Agent office**, which fills the notch. The
office hands tasks to Kiro, Codex, Cursor, OpenCode or Claude Code, which run headlessly, several at once,
each in a chosen folder, as bots at desks in a voxel office. A click on a desk opens its
**desk card** (what the agent is doing, and panels for its terminal, files, diff, pull
request, browser and screen). The office's menu (time
of day, music, history, Settings) opens Settings over it (nine sections: General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode, Claude Code), with a
back button.

The only ordinary window is the dashboard: the same office in a window with Hover's
own title bar (the system's on a Mac). It opens from the tray (the menu bar on a Mac), or a second launch. The app
lives in the tray.

On a Mac the usage rings are one status item in the menu bar instead of the island (the
camera housing leaves no room), and a Mac without a notch gets a notch-sized pill. The macOS
port follows Arz's (@Entourage397) macOS v1.0, which was Swift and C#; here the Mac app is
Swift UI (`macos/`: notch, menu bar, Settings, voice) around the web office (`web/office/`),
on the Rust backend (`crates/hover-backend`) that the Slint app (Windows and Linux) does
not link.

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

```sh
# macOS (Apple Silicon by default, HOVER_ARCH=x64 for Intel; Xcode's command line tools,
# node/npm for the office page, Rust from rust-toolchain.toml)
scripts/build-macos.sh                  # dist/macos-osx-arm64/Hover.app, signed ad hoc
scripts/test-macos.sh                   # cargo tests under sandbox-exec + tests/macos/backend-smoke.py
tests/macos/e2e/run.sh <Hover.app>      # E2E with stand-in agents, gh, cua and lume
```

A push to `main` whose `Cargo.toml` version has no tag yet, or a `v*` tag
(matching it), runs `.github/workflows/ci.yml`'s tests on Windows and Linux (Ubuntu 22.04)
on GitHub's runners, then builds the installers from that build, tags the commit and
publishes them together as the Latest GitHub release. Raising the version releases it.
A `macos` job (macos-15, Apple Silicon) runs `cargo check` on every crate but `hover`,
`notch-proto` and `hover-measure`, builds `hover-backend`, then runs `scripts/build-macos.sh`
and uploads the ad-hoc-signed `Hover.app` as a workflow artifact, on pull requests too.
It runs no tests (no display on the runner). A release also builds Intel and puts both in
disk images (`Hover-<version>-macos-arm64.dmg`, `-x64.dmg`), and `release` waits for them. `scripts/package-macos.sh` signs with a Developer ID and
notarizes.

The cross-check that Windows code still compiles, from Linux (mimalloc's C needs
clang-cl 19 or newer, which `cargo-xwin` drives with Microsoft's headers):
`cargo xwin check --release --workspace --all-targets --target x86_64-pc-windows-msvc`.
CI doesn't run it: its Windows job builds on Windows. The macOS code type-checks from
Windows with `rustup target add aarch64-apple-darwin` and `cargo check --target
aarch64-apple-darwin -p hover-agents -p hover-office` (`hover-quota` has a C build step and
needs a Mac's compiler and SDK; the `macos` job is its check; `hover` is Windows and Linux
only and refuses to compile on a Mac).

The version is `Cargo.toml`'s `[workspace.package] version`; the installers
and `hover --version` read it from there.

Only one copy of Hover runs at a time. A second launch opens the running copy's
dashboard and exits. On Windows this uses the named mutex `Local\HoverRunningInstance`
(the same name 2.x used); on Linux, a lock in `$XDG_RUNTIME_DIR`; on a Mac, `hover-core`
keeps a lock and a socket in `$TMPDIR` (else `/tmp/hover-<uid>`), which the Swift app does
not use.

Headless: `hover --shots DIR` renders every view with the software renderer. On a
real X display, `hover --selftest DIR` drives the notch and writes `report.json`.
On a Mac, `open -a Hover --args --settings <page>` opens Settings on a page (`start`,
`general`, `usage`, `computer-use`, `voice`, or an agent: `codex`, `kiro`, `cursor`,
`opencode`, `claude`). `Hover --smoke-test` is the test harness's (`scripts/test-macos.sh`).

## Layout

```
crates/
  hover-core     paths (+ the Noty move), settings.json (System.Text.Json's bytes),
                 crypto (AES-GCM; DPAPI / Secret Service key), history (sealed
                 agents/), images, single instance, palette and VS Code themes,
                 platform/{windows,linux,macos} (macos.rs compiles everywhere, so its
                 Keychain / LaunchAgent / `defaults` logic is tested on Windows);
                 bin/hover-data (data folders for tests);
                 projects.rs (projects, default workspace, voice settings),
                 secrets.rs (API keys sealed in secrets.dat)
  hover-agents   ACP host, checkpoints (checkpoint.rs), OpenCode's server (opencode.rs, over its own http.rs), Claude
                 Code's SDK mode (claude.rs), the runtime they sit behind, the tools (Kiro,
                 Codex, Cursor, OpenCode, Claude Code), sessions, the office's
                 state message, KiroStream, process groups / Windows jobs; route.rs
                 (voice's project routing); sandbox.rs (srt), computer_use.rs (Cua
                 Driver), spaces.rs (Cua Spaces: the agents' desktops, their `cua`
                 calls), browser.rs (the agent browser's MCP server and socket),
                 setup.rs (one-click agent install and sign-in), github.rs (gh: status,
                 install, sign-in), desk.rs (what the desk card and its panels read:
                 git, gh, terminal, files, diff, pull requests, subagents, pages)
  hover-quota    the four quota readers
  hover-backend  the Mac app's backend (binary hover-backend; the Slint app doesn't link
                 it): hover-core, hover-agents and hover-quota behind JSON lines on stdin and
                 stdout (wire.rs, backend.rs), the state message the web office reads
                 (office.rs), desk panels (panels.rs), prefs, quotas, screen; browser_host.rs
                 (the agent browser's calls, relayed to the Swift app)
  hover-md, hover-diagram   md.js and diagram.js, byte for byte
  hover-chat     the chat thread: layout, selection, copy, images, painter
  hover-notch    notch geometry, animation, hover rules
  hover-office   the office: scene, bots, helpers (mini.rs: subagents as small bots at
                 their parent's desk), wall canvases, camera, picking (bots and desks),
                 pacing, three.js 0.170's shading in office.wgsl; its own thread (live.rs)
app/             the product (crate `hover`, binary hoverai): app, Settings (pages.rs),
                 tray (sni.rs / win.rs), notch (notch.rs, x11.rs, win.rs), office UI
                 (office_ui.rs), desk card and panel (desk_ui.rs, ui/desk.slint), music,
                 bench.rs (HOVER_BENCH), selftest, shots; voice
                 (speech.rs, voice/, voice_ui.rs), Phonon's setup and engine (phonon.rs,
                 assets/phonon/); ui/*.slint; assets/
                 screen.rs (the Screen panel's capture, Windows and Linux)
macos/           the Mac app, in Swift (not a crate): Hover.swift (entry, hot keys, pointer),
                 Notch.swift (the notch window and island), MenuBar.swift, Settings.swift,
                 Voice.swift and VoicePanel.swift, OfficeHost.swift (the web office in a
                 WKWebView and the backend's pipe), AgentBrowser.swift, Spaces.swift,
                 Screen.swift, ShellEnvironment.swift, guardian.c (starts the backend);
                 entitlements, Resources/ (the office's music)
web/office/      the web office (main.js, desk.js, page.html; esbuild bundles it into
                 dist/kiro-office.html, not tracked); the Mac app's office
scripts/         build-macos.sh, test-macos.sh, package-macos.sh (Developer ID + notarize),
                 sandbox.sh (runs a command on this checkout inside srt)
tools/
  hover-measure  memory sampler, scenario runner, fake-agent, fake-opencode, fake-anthropic (not shipped)
  notch-proto    the port's Windows notch prototype, kept for its --selftest (not shipped)
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
  On X11 it is an override-redirect dock window with an ARGB visual and an XShape
  input region. On Wayland desktops Hover runs through XWayland (`HOVER_WAYLAND`
  opts out). On a Mac it is an NSPanel (`Notch.swift`, non-activating) at status level
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
    one's own words. `Runtime` (runtime.rs) is what the sessions see of every kind.
- **Claude Code runs in its Agent SDK mode, as T3 Code runs it** (`hover-agents::claude`):
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
    `ask::needs_asking` decides what reaches the user. Read only is `--disallowedTools`
    for its edit and command tools, the rest refused; voice's routing turn (access
    `none`) gets `--tools ""`. Trust is Hover's: its own suggestions would write a rule
    into the user's settings.
  - AskUserQuestion shows as OpenCode's questions do; the answers go back by each
    question's own text (as Claude Code looks them up). What it says (partial messages,
    tool calls and results, thinking) is put into ACP's shapes and read by `KiroStream`.
- **Kiro Web sessions** (`acp.rs`, `KiroSession::cloud`; Windows and Linux, not the Mac
  app yet). A Kiro task can run in Kiro's cloud instead of on this computer: the
  new-task box's cloud button (Kiro only) and the voice preview's. It goes through the
  same `kiro-cli acp` process: `session/new` with `_meta.kiro.executionTarget`
  `{kind: "cloud-sandbox"}` and `repositories` (`[{providerType: "GITHUB", name}]`; none
  is an empty workspace). Neither field is in Kiro's docs; both were read from its agent
  server (Oct 2026), which advertises the cloud in `initialize`'s
  `_meta.kiro.executionTargets`.
  - The repo is the folder's GitHub remote (`Desk::github_repo`), or one picked from
    `_kiro/sourceProviders/listResources`, or none. Access is always Full (the cloud has
    no asking), the session gets none of this computer's MCP servers, and it has no
    checkpoints. The desk card's Terminal, Files and Diff are off (`CLOUD_NOTE`).
  - The first prompt waits for the sandbox: one sent before its first `context_usage`
    update (about 15 s) is answered `cancelled` here while the cloud still runs it.
  - A reply after Kiro's process restarted loads the session with
    `_meta.kiro.sessionSource: "remote"`; without it Kiro makes an empty local session of
    the same id. A cloud session that can't be loaded fails; another is never started
    in its place.
  - The chat's cloud chip opens it in Kiro Web (`KIRO_WEB_SESSION` + the session id).
  - Saved in the history as `"Cloud"` (its repos), written only for cloud sessions.
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
  chat after the countdown (Settings → Voice, 5 s by default; Off waits for Start). The agent is
  always the one picked in the new-task circle. Over an open chat whose reply box is open,
  the shortcut dictates instead: the words go into the reply (Stage::Dictated), nothing is routed. Phonon
  (Python, CPU PyTorch, the model; about 1.5–1.8 GB installed) is downloaded only from
  Settings, into `<data>/phonon/`. Keys are sealed in `secrets.dat`, never in
  `settings.json`. On a Mac voice is the Swift app's (`macos/Sources/Voice.swift`,
  `VoicePanel.swift`, fixed Control-Option-Space): Apple's speech recognizer on the Mac
  (SpeechAnalyzer on macOS 26+, else SFSpeechRecognizer), with no Phonon, Groq or cleanup.
- **Checkpoints** (`hover-agents::checkpoint`). Before and after every turn Hover keeps the
  project folder in a shadow git store of its own, `<data>/checkpoints/<session key>.git`,
  with the project as its work tree: the project's own `.git` is never read or written,
  `.gitignore` decides what is left out, and a checkpoint is a tree id (saved in the
  history as `CheckpointBefore` / `CheckpointAfter` on the turn). Needs `git` on PATH;
  without it there are none. A whole drive or the home folder is refused, and a folder that
  takes over 90 s is given up on for that chat. **Restore** (under an earlier answer) puts
  the folder and the chat back to just after that answer; **Try again** (under any answer)
  puts them back to before that message and sends it again (`KiroSessions::rewind`,
  `Rewind::{After, Before}`). Both ask first, and only when nothing runs. The agent still
  remembers the removed turns, so its next message carries one note that the folder and chat
  went back (before the very first message it starts a new conversation). The folder as it
  was just before a restore is kept in the store (`undo_tree`). Deleting a chat deletes its
  store. **Retry** is the older button: the newest prompt again, files untouched.
- **The sandbox** (`hover-agents::sandbox`, Settings → Integrations, on by default). Each
  tool is started under Anthropic's sandbox-runtime (`srt`, pinned in `sandbox::VERSION`):
  sandbox-exec on a Mac, bubblewrap on Linux. It writes only to the folders its sessions
  work in, its own state and caches, and temp; keys, keychains, mail and other apps' data
  can't be read; there is no window server and no Apple Events; the network goes through
  srt's proxy to the tool's service, package registries and GitHub (more in
  `<data>/sandbox/allowed-domains.txt`). The folders are fixed when the tool starts, so a session in
  another folder gets the tool started again (when nothing of it runs). The settings file
  for srt is text built by pure functions (`config`, `srt_args`) that the tests run on every OS.
  If `srt`, `rg` (and on Linux `bwrap`, `socat`) is missing, or Hover is already inside a
  sandbox (`HOVER_SANDBOXED=1`), the tool starts as before and `hover.log` says why;
  Settings shows what is missing (`sandbox::missing()`). Off on Windows: srt's Windows
  support can't reach tools installed for the user.
- **Computer use** (`computer_use.rs`, off until switched on). Hands every agent Cua
  Driver's MCP server (`cua-driver mcp`), which drives other apps in the background.
  Where perl exists it starts behind Hover's guard, which turns foreground input into
  background input and refuses desktop-wide input, raising a window, the clipboard and the
  like, with a note the agent reads. Cua's tools are MCP calls, so Ask first and Read only
  treat them as any other. Settings installs CuaDriver and asks it for Accessibility and
  Screen Recording (the grants go to CuaDriver, not Hover). macOS only: Cua is kept to the
  Mac, so Windows and Linux show the switch off with `computer_use::UNSUPPORTED`.
- **Agent desktops, Cua Spaces** (`spaces.rs`; the messages in `hover-backend`; the Swift
  side in `macos/Sources/Spaces.swift`, `Hover.swift` and `Notch.swift`). Off until switched
  on in Settings → Computer Use, and macOS 26+ on Apple silicon only (`spaces::UNSUPPORTED`).
  Each project folder gets one Space, a macOS VM (Cua's `cua` CLI with Lume) that the agents
  working there share for computer use instead of the user's screen. It is made when the
  project's first agent starts, turned off when none of its agents is in the office, after
  15 idle minutes and when Hover quits, and deleted with the project's last session. The
  Screen panel shows its viewer. Drag an app or files onto the notch and the office opens
  on the desktops to send them to; the full-screen button opens Hover in a window.
- **The agent browser** (`browser.rs` + `crates/hover-backend/src/browser_host.rs` +
  `macos/Sources/AgentBrowser.swift`, macOS only). Each agent
  gets a browser MCP server (12 tools: open, snapshot, click, type, …) that talks over a
  user-only Unix socket (0600, in a 0700 folder, with a token sent in the server's
  environment, never on a command line) to Hover, which drives a WKWebView per session:
  the backend relays each call to the Swift app as a `browser` message.
  The page is shown in the desk card's Browser tab. OpenCode has one server for all its
  sessions, so its calls go to the session at work.
- **Setting an agent up** (`setup.rs`, macOS). The "Set up" row on an agent's page installs
  what is missing with the maker's own installer and then runs the tool's sign-in in a
  Terminal window; Hover never sees the credentials.
- **The desk card** (`desk_ui.rs`, `ui/desk.slint`; data from `hover-agents::desk`). A click
  on a desk with a session opens a card where you clicked: the last steps, the question
  the agent waits on, or the answer, a reply box, and eight tiles that open a wide panel:
  Terminal, Files, Diff, Agents, Linked PRs, Pull request, Browser, Screen. Every git and
  gh call blocks, so it runs on a worker; lists are windowed from Rust and only the rows
  on screen reach Slint. Files shown stay inside the session's folder (links followed).
  A tile the OS can't run is disabled with its reason (`TileContext.off`).
- **The pull request tab** sets up the GitHub CLI in one click (`github.rs`): install with
  winget or Homebrew where there is one (else a hint: Hover never uses sudo), then
  `gh auth login` with the device code shown to copy and the page to open. Create pull
  request can commit, make a branch, push and open the PR; it is disabled with the reason
  while the agent runs.
- **Subagent helpers** (`hover-office::mini`). A subagent at work (kind `agent`, or a Kiro
  or Codex step titled like one, `state::is_subagent`) shows as up to four small recoloured
  bots beside its parent's desk, which hop out and back and file sheets at the tray. They
  keep the office at 30 fps while they exist; the card says how many are out.
- **The Screen panel** (`screen.rs`; on a Mac `macos/Sources/Screen.swift`) is a still of
  the desktop, and about twice a second while computer use runs, the windows of the apps
  the agent used (PrintWindow on Windows, X11 on Linux, ScreenCaptureKit on a Mac, which
  needs Hover's own Screen Recording grant).
- **Answers are Markdown, drawn without a library** (`hover-md`, `hover-diagram`).
  Everything the agent wrote is escaped, and Mermaid flowcharts are laid out natively.
- **The office renders on its own thread with its own wgpu device** (DX12 on Windows,
  Vulkan or GL on Linux). The frame is composited on the CPU into the Slint view. On a Mac
  the office is the web page (`web/office/`, three.js) in a WKWebView.
  - It redraws the shadow map only when something moved.
  - It runs at 30 fps while a bot walks or works, 10 fps when idle, 1 fps with
    animations off, and draws nothing while hidden.

## What each OS can't run

A feature the OS can't run is switched off in Settings (or its desk tile) with the reason
beside it, never hidden. The notes are constants next to the code (`sandbox::UNSUPPORTED`,
`browser::UNSUPPORTED`, `setup::UNSUPPORTED`, `computer_use::UNSUPPORTED`,
`spaces::UNSUPPORTED`).

| Feature | Windows | Linux | macOS | Why |
|---|---|---|---|---|
| Notch, office, desk card, panels (Terminal, Files, Diff, Agents, PRs) | yes | yes | yes | |
| Sandbox | off: "The sandbox needs macOS or Linux." | yes, if `srt`, `rg`, `bwrap`, `socat` are there | yes, if `srt` and `rg` are there | srt's Windows support can't reach tools installed for the user |
| Agent browser, Browser tab | off: "Agent browser needs macOS." | off, same note | yes | the page is a WKWebView |
| One-click agent setup | off: "One-click setup is available on macOS." | same | yes | the installers and sign-in scripts are written for a Mac |
| Computer use | off: "Computer use needs macOS." | off, same note | yes | Cua Driver is built for macOS; Cua is kept to the Mac |
| Agent desktops (Cua Spaces), drag to the notch | off: "Agent desktops need macOS 26 or later on Apple silicon." | off, same note | yes, on macOS 26+ Apple silicon with Cua's CLI and Lume | Cua Spaces runs macOS VMs with Lume; Cua is kept to the Mac |
| Screen panel capture | yes (PrintWindow) | X11 only | yes, with Screen Recording | |
| GitHub CLI one-click install | winget | hint only | Homebrew, else hint | no sudo, no release download (no TLS in `hover-agents`) |
| Local speech (Phonon) | yes | yes | off: Apple's on-device speech recognizer instead (no Groq or cleanup) | Phonon's runtime isn't built for a Mac |
| Usage in the island | yes | yes | in the menu bar instead | the camera housing |
| Tray | yes | yes (D-Bus) | menu bar | |

## Conventions

- Match the surrounding style. Comments explain **why**, not what; keep them.
- Keep changes surgical: touch only what the task needs, and remove only the dead
  code your own change creates.
- Prefer the smallest change that works. A new crate needs its reason written in
  `Cargo.toml`.
- Verify by running the real thing: `cargo test --release --workspace` clean, and the
  Windows `cargo check` above green. For UI changes, render it (`hover --shots`), since
  a green build is not proof the pixels are right. Nothing of the macOS behaviour has been
  run by the people who wrote it (see docs/MACOS.md): say what you ran and what you didn't.
- Don't commit `target/`, `publish/` or `dist/`.
- `cfg(not(windows))` used to mean Linux. Say `target_os = "linux"` for X11, D-Bus, the
  Secret Service, XDG, prctl and ALSA, and give a Mac its own path or an honest "not on
  macOS" branch. Logic that only runs on a Mac (argument lists, parsing, text) goes in
  functions compiled everywhere, so the Windows tests cover it.

## Gotchas

- **Line endings are CRLF** (`.gitattributes`), except the scripts run through `#!`
  or `sh` (`packaging/linux/*`, `Makefile`), which are LF.
- **`bin/` is git-ignored** at any depth: `crates/hover-core/src/bin/hover-data.rs`
  is tracked with `git add -f`.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky; anchor
  on unique ASCII lines.
- **One renderer per process**: femtovg (GL) on Linux, femtovg-wgpu DX12 on Windows
  (one wgpu device shared with the office).
- **A Mac app started from Finder has no shell PATH.** Anything that looks for a tool
  (`agents::find`, `on_path`) sees the login shell's PATH only after `ShellEnvironment.swift`
  merged it in (the backend starts after).
- **`Shortcut::label()` reads ⌥N on a Mac**, not Alt+N; tests that want the Windows text
  use `label_for(false)`.
- **Headless GPU runs** need `XDG_RUNTIME_DIR` set.
- **The chat's layout goldens** (`hover-chat`'s `thread.rs`) were measured in Linux
  Chromium with DejaVu Sans, so the three that compare text widths skip on Windows.
