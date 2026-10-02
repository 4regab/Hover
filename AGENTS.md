# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A Windows desktop app (.NET 10, WPF) with one surface a hover away: **the notch**
at the top centre of the main display (after NotchOwl for Mac). At rest it is a
slim black island: the Claude Code / Kiro / Codex / Cursor quotas the user switched
on (each the tool's own logo in its ring), the agents at work (their logos, what the
one in front is doing, for how long), a question an agent is waiting on, or nothing.
Hovering it, clicking it or `Alt+N`
opens the **Agent office**, which fills the notch: it hands tasks to Kiro, Codex,
Cursor or OpenCode, which run headlessly, several at once, each in a chosen folder, as bots at
desks in a three.js office, and, with computer use on, can drive apps in the background
through Cua Driver. The office's menu (time of day, music, history, Settings) opens Settings over it (six
sections: General, Integrations, Kiro, Codex, Cursor, OpenCode), with a back button.

The only ordinary window is the dashboard (the tray icon or its menu, or a second
launch of the exe), the same office in a normal window; the app
lives in the tray. (Sticky notes and the edge tray went in 1.1. The workspace
(tasks, focus timer, notepad, events, screenshots, command buttons, Insights) went
in 2.0; its `planner.dat` is deleted on first run, as the user chose. `note.key`
stays: the agents' history is sealed with it.)

## Build, test, run

All commands run from the repo root in PowerShell. Never use `cd`; the paths are
relative to root.

```powershell
# Build everything
dotnet build .\Hover.slnx -c Release

# Run the tests
dotnet test .\Hover.slnx -c Release

# End-to-end: drives the real Hover.exe (takes over the pointer; build first)
dotnet test .\tests\Hover.E2E\Hover.E2E.csproj -c Release

# Build and launch
.\build.ps1 release run

# Self-contained single-file Hover.exe in .\publish
.\build.ps1 publish

# Installer in .\dist (needs Inno Setup 6 or 7)
.\build.ps1 installer
```

Only one copy of Hover runs at a time (a named mutex). A second launch opens the
running copy's dashboard and exits. Stop every `Hover` (and any old `Noty`) before
an E2E run, or the test's copy exits and the test drives the other one:

```powershell
Get-Process Hover, Noty -ErrorAction SilentlyContinue | Stop-Process -Force
```

Requires the .NET 10 SDK. The app targets `net10.0-windows10.0.17763.0` (the WebView2
composition control needs the Windows SDK projection) and must build on
Windows (or with `EnableWindowsTargeting`).

## Layout

```
src/Hover/
  Core/        Model + storage: Settings, Paths, Crypto (AES-GCM, DPAPI key),
               Shortcut, Log, Layout (the notch item ids), Quota (Claude Code /
               Kiro / Codex / Cursor usage readers), Palette (every colour, Hover's
               light and dark, and the VS Code theme reader and finder). Layout,
               Quota and Palette have no WPF.
  Interop/     Win32 P/Invoke, monitor enumeration, global hotkeys, HostWindow
               (the borderless, click-through window the notch is drawn in).
  Services/    Actions (tray menu commands), TrayIcon, Agents (Kiro, Codex, Cursor,
               OpenCode: where each is, how it starts, install and sign-in checks),
               IAgentRuntime (in KiroRunner.cs: what the sessions see of a tool),
               AcpHost (one tool running as an ACP server, shared by its sessions,
               stopped when idle), OpenCodeHost (OpenCode's own server, the same
               way), ComputerUse (Cua Driver as the sessions' MCP server: lookup,
               status, install and grant), KiroRunner (KiroStream, which reads ACP session updates,
               the shared result types, and Kiro's fallback model list). No WPF.
  Owl/         OwlApp (shared state, quota polling), Notch (the top-centre host, on
               the main display only, and the dashboard window), OfficeView (the
               office, with Settings laid over it), Pages (Settings), KiroSession
               (one agent session of any tool: its turns, state and cancellation)
               and KiroSessions (all of them, shared; no WPF), AgentHistory (every
               session, sealed, until deleted; no WPF), KiroPage (hosts the
               office in WebView2), AgentWords (what the notch says a session is
               doing or asking, in a few words; no WPF, in KiroSession.cs), Marks
               (the tools' logos for WPF, LiveMark and MarkStack: the notch's rings
               and its stack of agents), Bot (the first-use note's bot glyph and
               the notch's frame clock), KiroText (Markdown to plain text), Popover (counts open
               menus), Theme (the palette in use), Ui (brushes, builders, ring
               gauge, segmented control), Icons (generated line icons), Corners
               (pill-shaped corner radii).
  Themes/      Styles.xaml (menus, tooltips), Owl.xaml (Settings' controls).
  Assets/      hover.ico (the app icon), hover-mark.png (the logo without its dark
               square), Fonts/ (Inter and Inter Display),
               kiro-office.html (built from web/office; do not edit), office-beats.ogg.
web/office/          The Agent office page: main.js (three.js scene and UI), page.html,
                     md.js (Markdown to escaped HTML) and diagram.js (Mermaid flowcharts
                     to SVG), both written here with no library.
                     `npm ci` once, then `node web/office/build.mjs` writes the asset.
tests/Hover.Tests/   NUnit tests.
tests/Hover.E2E/     UI Automation run against the real app. Not in Hover.slnx.
assets/hover.png     The logo (README picture, source of the app icon).
assets/make-icon.py  Writes src/Hover/Assets/hover.ico (one frame per size) and
                     hover-mark.png from assets/hover.png. Replace hover.png and run it;
                     don't edit the outputs. Needs Pillow.
assets/make-line-icons.py  Writes src/Hover/Owl/Icons.cs from Lucide at a pinned
                     version. Add an icon to NAMES and run it; don't edit Icons.cs.
installer/Hover.iss  Inno Setup script (driven by build.ps1).
```

Note: the C# namespace is `Hover.*`. Some environment-variable and folder names
carry a legacy `Noty` reference **only** in `Core/Paths.cs`, which migrates an old
`%APPDATA%\Noty` install to `%APPDATA%\Hover` on first run. Leave that path alone.

## How it works (the parts that surprise people)

- **The notch window is borderless, topmost, and `WS_EX_NOACTIVATE`** while
  resting, so brushing it never steals focus. The bit comes off while the
  office is open (keyboard). See `Interop/HostWindow.cs` (`SetAcceptsKeys`).
- **Pointer is polled, not hooked.** `NotchManager` reads the cursor every 50 ms.
- **Hover's timers run at `DispatcherPriority.Normal`.** WPF runs
  Background-priority work only when no input is waiting in the queue, and in
  testing that starved the notch poll for 8–14 s at a time.
- **One full-size, click-through window, on the main display.** The shape grows
  from its resting size (island, question card, or nothing) to the office by
  animating one `Openness` value; the window itself never resizes (that made it
  blink), except when Settings → General → Office size changes. The resting size
  springs to each new width (`RestWidth`, `RestHeight`), so the island breathes as
  its words change. Open, the office fills the shape edge to edge: no margin, no
  rim, no corners of its own (the page drops its border in `body.host.notch`); the
  shape's clip is its only frame. The notch shows nothing of its own while open.
- **Settings sits over the office.** Settings in the office's menu (`settings` message) makes
  `OfficeView` lay `SettingsPage` over it, with a back button (and a close
  button in the notch). The office is only collapsed meanwhile, so its sessions go on
  and it comes back at once (after 30 s hidden its WebView2 is dropped and made again).
- **Colours come from one `Palette`.** Hover's own light and dark are Apple's
  system colours. A VS Code theme is read from its file (following `include`),
  and only the few colour ids in `Palette.Keys` are kept, in `Settings.Theme`, so
  the theme survives the editor being removed. `Palette.Installed` lists the
  themes VS Code, Cursor, Kiro and Windsurf have on the PC. Code-built elements
  take their brushes when they are made, so a theme change rebuilds the views
  (`Theme.Changed`); the XAML styles read the same colours as dynamic resources
  that `Theme.Publish` rewrites. The resting notch is always black, and the office
  keeps its own look.
- **Quotas have no official API.** `Core/Quota.cs` reads what each tool exposes,
  read-only: `api.anthropic.com/api/oauth/usage` with Claude Code's own sign-in
  (never refreshed, which would rotate Claude Code's tokens); `kiro-cli chat
  --no-interactive /usage` output; the `rate_limits` of the newest `token_count`
  event in `~/.codex/sessions/**/rollout-*.jsonl`; and `cursor.com/api/usage-summary`
  with the token from Cursor's `state.vscdb`. Each is off until switched on in
  Settings → Integrations, and `OwlApp` re-reads it five minutes after the last read
  finished. A format change in any of them shows as a readable failure, not a
  crash. The Kiro read is the heavy one: kiro-cli and the MCP servers it starts
  take a few hundred MB for about eight seconds, then all exit.
- **Agents run as ACP servers, never in a terminal.** Each tool is one long-lived
  hidden child (`AcpHost`, JSON-RPC over stdio), started by the first task that
  needs it and shared by all that tool's sessions: `kiro-cli acp --agent-engine v3
  --auth-method cli`, `codex-acp` (the `@agentclientprotocol/codex-acp` adapter)
  and `cursor-agent acp` (looked up in `%LOCALAPPDATA%\cursor-agent`, never as
  `agent`, which other tools also install). A conversation is an ACP session; a
  reply goes to the same one. After the idle time in the tool's settings (5 or 15
  minutes with nothing running) the process is shut down, and the next reply starts
  it again and loads the conversation back (`session/load`, its replay ignored).
  Model, effort and access are session config options, set per turn where the tool
  offers them; what it offers is kept in `Settings.AgentOffers` for its settings
  page. Tool access, per tool: Full (never asks, what 2.0 did, and the default),
  Ask first (`AgentApproval.Risky`: commands, deletes, moves, the network and
  anything outside the folder), Ask always, or Read only. Asking takes the tool out
  of its own autopilot, and `AcpHost.Permission()` answers `session/request_permission`
  off the read loop: what the setting leaves alone is allowed, the rest goes to the
  user (`AcpHost.Asking` → `OwlApp` → the session with that ACP id →
  `KiroSession.Ask`), with no timeout; a stop withdraws it. The question shows in the
  notch (an amber island with Deny and Review; Review opens the card in the notch,
  which takes the keyboard: Enter allows, Shift+Enter trusts, Esc denies; hovering
  doesn't open the office while one waits), over the bot's head in the office (it
  raises its hand), and at the end of its chat, where a reply counts as Deny. Trust
  is Hover's, for the rest of the session: Hover answers the same call itself from
  then on, and picks the tool's own allow-always only for Codex, where it too lasts
  the session. Cursor's allow-always writes a lasting rule into the user's
  `~/.cursor/cli-config.json`, and Kiro's can change a Kiro setting. How each tool
  is made to ask (checked against their sources): Kiro, autopilot off (past its
  built-in defaults every call asks); Codex, mode `workspace-write` for Ask first
  (it asks to write outside the folder or go online; its sandbox lets the rest
  run, which is Codex's call, not Hover's) and `read-only` for Ask always (every
  write and command asks), never `agent`, whose own reviewer answers in the user's
  place; Cursor asks anyway (Hover never passes `--force`), so Full is Hover
  answering yes. Read only: Kiro runs with autopilot off and Hover refuses its write
  approvals; Cursor runs in Ask mode; Codex's read-only mode wrote files anyway on
  Windows (no sandbox), so it isn't offered. Stop sends `session/cancel`, and a tool
  that doesn't stop within 8 s is shut down if nothing else of it runs. Prompts go
  over stdin, never on a command line. `KiroStream` reads the updates loosely (the
  answer is the last message, after the last tool call); they drive the bot's pose
  and the steps. Folder first: no prompt until a folder that exists is picked
  (`Settings.KiroFolder`). Full tool access is explained once
  (`Settings.KiroNoticeSeen`). Sessions live in `OwlApp.Kiro`, shared by both
  views, so hiding or closing a view never stops one: up to three run at once
  across all tools, and the newest six are kept. Each announces its end in the island (the tool's logo with a badge, and the task) and as a Windows notification, naming the
  tool, and all tools are shut down when Hover quits. They run in a Windows job that
  kills them with Hover, so a killed or crashed Hover leaves none behind. Install and sign-in are
  checked with each tool's own status command (kept five minutes); a tool that
  fails is greyed in the office with what to do.
- **Agents run in a sandbox: Anthropic's sandbox-runtime (srt).** `Services/Sandbox.cs`
  (no WPF). On a Mac or Linux every tool's process (`AcpHost`, `OpenCodeHost`) starts
  as `srt --settings <Support>/sandbox/<tool>.json -- /usr/bin/env TMPDIR=… <tool>`:
  writes only to its sessions' folders, its own state and caches, and a short temp
  folder (`/private/tmp/claude/hover-<tool>`, the only place Unix sockets work, besides
  CuaDriver's); no reading keys, mail, messages, browsers', other apps' or Hover's own
  data; no window server and no Apple Events, so nothing it runs can draw a window,
  take focus or launch or script an app; the network through srt's proxy to the
  tool's service, package registries, GitHub and the hosts in
  `<Support>/sandbox/allowed-domains.txt`. The folders are fixed at start: a session in
  another one restarts an idle tool, and fails with a reason while it is busy. The
  keychain folder stays readable (Cursor keeps its sign-in there). macOS's trustd is
  in reach (`enableWeakerNetworkIsolation`): without it .NET, Go and Security-framework
  TLS can't verify certificates. MSBuild's worker nodes use sockets in /tmp it can't
  allow, so builds in it stay in one process. srt 0.0.78 and ripgrep are part of what
  a tool needs installed (`Agents.Check`, one-click setup); Settings → General → Run
  agents in a sandbox switches it off. Never nested (`HOVER_SANDBOXED=1`). Not on
  Windows: srt's Windows alpha runs as another account that can't reach tools
  installed for the user. Agent work on this repo goes through `scripts/sandbox.sh`
  the same way (see `.kiro/steering/sandbox.md`); `scripts/test-macos.sh` run there
  skips its on-screen app smokes. Under srt each tool runs through `Sandbox.Relay` (a
  perl relay with blocking pipes): srt hands the tool non-blocking stdio, and a write
  over 64 KB (Cua's tool list is 67 KB) failed with EAGAIN and killed Kiro.
- **Computer use is Cua Driver, handed to the agents as an MCP server.**
  `Services/ComputerUse.cs` (no WPF). Off until `Settings.ComputerUse` is switched on
  (Settings → Integrations here, Settings → Computer Use on a Mac); then every new ACP
  session gets `cua-driver mcp` in `session/new` and `session/load` (`mcpServers`,
  stdio, with ACP's required empty `env` list), and OpenCode gets it as a local MCP
  server in `OPENCODE_CONFIG_CONTENT` (inline config, so no opencode.json is written).
  Hover never drives anything itself and never passes Cua's `--dangerously-bypass-
  approvals`: each click or keystroke is a tool call under the session's access (Ask
  first asks in the notch, Read only refuses it). Where perl is (Mac, Linux) the server
  is `perl <Support>/cua/guard.pl cua-driver mcp` (`ComputerUse.Guard`, readable but not
  writable in the sandbox), which keeps computer use out of the user's way: input only
  to a named app, in the background (foreground becomes background and leaves the tool
  list), and no desktop-scope input, `bring_to_front`, window moves, `kill_app`,
  clipboard, replays or Cua settings. A session's MCP servers are fixed when
  it is made or loaded, and OpenCode reads them at startup, so after a switch the tool
  is restarted before its next run when nothing else of it runs (`AcpHost` then loads
  the conversation back with the new set; `ComputerUse.Signature` tells them apart).
  `cua-driver` is looked up on PATH, then where its installers put it
  (`~/.local/bin`, `/Applications/CuaDriver.app`, `%LOCALAPPDATA%\Programs\Cua\cua-driver\bin`).
  On a Mac the Accessibility and Screen Recording grants belong to CuaDriver.app: the
  first `cua-driver mcp` starts its daemon in the background through LaunchServices and
  proxies through it. Inside the sandbox it can't (no Launch Services there), so Hover
  starts the daemon first (`ComputerUse.EnsureDaemon`) and the sandbox lets the agent's
  cua-driver reach its one socket (`~/Library/Caches/cua-driver/cua-driver.sock`). `cua-driver permissions status --json` only answers through that
  daemon (it reports "unknown" otherwise, and the check starts it when computer use is
  on); `permissions grant` asks for them. Install and grant run Cua's own commands
  (`ComputerUse.Install`, `Grant`). Checked against cua-driver 0.31.0.
- **OpenCode runs as its own server, as T3 Code runs it.** `OpenCodeHost` starts one
  hidden `opencode serve --hostname=127.0.0.1 --port=0 --mdns=false` for all its
  sessions, with a password made for that start (in its environment, sent as Basic
  auth; never on a command line or in a URL) and `OPENCODE_ENABLE_QUESTION_TOOL=1`.
  It talks to it over HTTP and its event stream (`/event`), with no Node and no SDK:
  the JS SDK is a thin client of the same API. Every call names the session's folder
  (`?directory=`), so that folder's opencode config, agents, skills and MCP servers
  apply. OpenCode keeps its own providers (API keys, sign-ins, local models); model
  ids are "provider/model" as it names them, and effort is the model's own variants.
  A turn subscribes first, then sends `prompt_async` with a message id Hover makes;
  only an idle after that message (or a busy for it) ends the turn. A dropped stream
  reconnects and reads status, messages and waiting requests back; a prompt whose
  answer was lost is looked up by its id, never sent twice. Access is per-session
  permission rules (OpenCode applies the last match): the agent's own last-word
  denies go after Hover's, so Full never undoes one; Read only makes every change
  ask and Hover refuses each. Approvals answer `once` (Trust is Hover's); the
  question tool's questions show in the chat, over the bot and in the notch, and a
  skipped one is rejected. A resumed conversation OpenCode no longer has fails
  rather than start a new one. Checked against OpenCode 1.18.31 (floor 1.14.19).
  The server takes 30 to 40 s to start cold and about 1 GB while working.
- **Sessions are kept until the user deletes them.** `AgentHistory` seals an index
  (`agents/index.dat`, the only part held in memory) and one file per session
  (`agents/<key>.dat`, read when it is opened) with Hover's key (`note.key`), and writes
  them off the UI thread in order. A session is saved when it starts, on each reply
  and when a turn ends. The office's bookshelf (and the history button) lists them;
  opening one shows its chat without a desk, and a reply wakes it: it gets a desk
  (the oldest finished session gives one up and stays in the history) and its tool
  loads the conversation back. Delete (the chat's bin, or the bin by a history row)
  asks first, stops a run, and removes the session from the office and the disk.
- **A new task starts from one circle.** At the office's bottom left: a click shows
  each tool's own logo (the name on hover; a tool not installed or signed in is
  grey and says why), a pick grows the box for that tool (prompt, image, folder,
  model, send). Esc, the chevron or a click on the office folds it back; a draft is
  kept and marked with a dot. There is no dock: the wall board lists the sessions
  as notes (bot and title), or says the office is quiet.
- **A desk opens a card in the chat's style.** A click on a desk with a session at it
  (the bot itself still opens the chat) shows the chat's header (tool, title, stage
  and clock, folder, access, context), what it does now (its last steps as the chat
  draws them, the helpers it has out, the question it waits on, or its answer), the
  eight surfaces as tiles (Browser, Terminal, Files, Diff, Pull request, Linked pull
  requests, Agents and Screen, T3 Code's Device), each with its letter and what it
  holds, greyed with a reason where there's nothing, and a reply box (Enter sends,
  the round button stops a run when empty). The card is built once per open and its
  parts redraw in place, so a reply being typed survives state updates. The pick raycasts the
  bot's own boxes against the room, so a seated bot behind its monitor still opens the
  chat and the desk around it the menu; hovering shows a small tip ("Chat with X" or
  "X's desk"). A row opens the wide
  side panel with the eight as tabs (`web/office/desk.js` builds them, escaping
  everything). The page asks `{type:'desk', what}` and the host answers from
  `Owl/DeskInfo.cs` (no WPF; shared by `KiroPage` and `Hover.Backend`): terminal,
  subagents (a `subagent_type`-style input) and pages (fetches, URLs in inputs, local
  servers in output) come from the steps, which keep a call's raw input and a longer
  output (`KiroStep.Input`, `Log`) that the state message leaves out; files, diff and
  PRs run git and gh hidden, with optional locks off, timeouts and caps, and a file is
  read only inside the session's folder (links followed). On a Mac the Browser panel
  is Hover's own browser (below), laid over the panel by the host; elsewhere it is a
  sandboxed iframe. The Pull request tab sets up the GitHub CLI in one click
  (`Services/GitHubCli.cs`: Homebrew or the official release into ~/.local/bin,
  winget on Windows; then gh's device-code sign-in and `gh auth setup-git`), and with
  no pull request for the branch it offers Create pull request (`DeskInfo.CreatePr`:
  optional new branch and commit, push, `gh pr create`; never while the agent runs,
  branch names checked so they can't be options). Screen is the main display, but
  never the user's own windows: the agent's desktop, viewed like a cloud agent's remote
  desktop without a VM. It is the desktop picture (Window Manager's own layer under
  the desktop level, which is not Stage Manager's black one) plus only the windows of
  the apps the agent's computer use opened or acted on (`DeskInfo.Apps`: pids, bundle
  ids and names from its steps, sent as `apps` in the state and in `{type:'screen'}`)
  and Cua's agent cursor, never the user's pointer; a still at rest, live at up to
  8 fps while the session's last steps are computer use (`testing`), the user watches
  or takes control. Computer-use steps come as `k: 'screen'` rows (`DeskInfo.ScreenAction`)
  for captions and an activity timeline, and the page keeps a frame a second to replay.
  Control (`{type:'screenInput'}`, `ScreenControl` in Screen.swift) sends the user's
  clicks, typing, keys and wheel to the agent's own windows only, through `cua-driver
  call` in the background; `{type:'screen'}` is renewed every 3 s and stops itself after 8 s
  (`Owl/ScreenFeed.cs` on Windows, `macos/Sources/Screen.swift` with ScreenCaptureKit
  and Hover's own Screen Recording grant on a Mac). In a browser, `?desk=<id>[:<tab>]`
  opens it with demo data.
- **Agents get a desktop of their own: one Cua Space per project.** `Services/Spaces.cs`
  (no WPF), on with Settings → Computer Use → Give agents a desktop (macOS 26+, Apple
  silicon; `Settings.AgentSpaces`, image `SpaceImage` macos or linux). A desktop is a
  separate VM (macOS 26 through Lume, or a Linux container), never the user's own
  macOS. The agents working in one folder share its Space (`Spaces.NameFor(folder)`:
  `hover-<folder name>-<hash of the path>`), each with its own cursor in it. It drives
  Cua's `cua` CLI: `spaces create <image> --name <that name>` before the project's
  first run (`Spaces.Ensure`, one create for agents starting together; its progress
  as `space` in the state, with the agents it is shared `with`), `spaces stop` when no
  agent of the project is left in the office or Hover quits, `spaces delete` with the
  project's last session, live or in the history. The agents' computer use goes to
  it, never the user's screen: `cua mcp --sandbox local:<name> --permissions
  computer:…` (no shell, no Spaces admin), which Hover
  runs itself outside the agents' sandbox and joins to the agent over the browser's
  relay and socket (`BrowserTool.Bridge`, server `cua-space`); host Cua Driver is not
  given then. The Screen panel shows Cua's own interactive viewer (`cua sb view
  --no-open`), one per project's desktop, laid over the panel's `.spbox` by the host
  (`SpaceViewers`, keyed by the Space's name; it refuses to be framed), with every
  agent's actions on it; a click in it is the user stepping in. An app window dragged to the
  notch (`TeleportDrag`: the window under the pointer moving, via CGWindowList), or
  files and apps from Finder or the Dock (`NotchDropView`), open the office on the
  agents' desktops (`#tdrop`); a drop sends `teleport` (`cua teleport push --app
  <bundle> --sandbox <space>`, Cua's own consent and Touch ID) or `spaceFiles`
  (`send_file` to its Downloads). One-click setup runs Cua's installer
  (`--select cli,spaces --no-onboarding`), `cua runtime setup lume` and a first create
  to download the image. A Mac runs two macOS Spaces at most.
- **The office opens in a window.** The button at its top right: from the notch it
  folds the notch and opens the dashboard window (big, full-size content) with the
  same chat or desk panel open (`{type:'window', open}` → `restore`); in the window it
  toggles full screen.
- **Agents get Hover's browser, as T3 Code's get its preview.** `Services/BrowserTool.cs`
  (no WPF) is an MCP server (`hover-browser`: browser_open, snapshot, click, type,
  press, scroll, screenshot, evaluate, wait, console, back, reload) given to every
  session where the host has a browser (the Mac app; Settings → Computer Use switches
  it off). The agent's tool starts `perl <Support>/browser/relay.pl <socket> <token>`
  inside its sandbox, which joins its stdio to one Unix socket in srt's temp folder
  (`hover-browser-<user>/b.sock`, the only socket path srt is told to allow for it);
  the token names the session (its key; OpenCode's one server says "opencode" and
  gets the session at work). Calls go to the host as `{type:'browser'}` and come back
  as `browserResult`. `macos/Sources/AgentBrowser.swift` keeps one WKWebView per
  session with its own non-persistent store (no cookies of the user's), http(s)
  only, no window at all until the Browser panel shows it (1280×800 then), and lays
  it over the panel's page box (`browserView` with the box's rect, sent by the page
  whenever it moves or something covers it). Snapshots number interactive elements
  `[ref]` via a `data-hover-ref` attribute; screenshots are JPEG at one pixel per
  point. It runs outside the agents' sandbox, so it reaches any website: a deliberate
  hole, and each call still goes through the session's access. Its steps show as
  `k: 'web'` rows and never count as computer use (`DeskInfo.IsScreen`).
- **Voice tasks are the Mac's own (`macos/Sources/Voice.swift`, `VoicePanel.swift`).**
  Control-Option-Space is a Carbon hot key with press and release: held, it listens until
  let go; tapped, hands-free until pressed again; Esc is taken as a hot key only while
  listening (the panel doesn't have the keyboard then). The window server gives keys
  straight to a Hover panel that has the keyboard (the notch's office, the dashboard,
  Settings), where the hot key never fires, so a local key monitor catches the same keys
  there (`App.voicePress` keeps a press from counting twice). Speech is on device: SpeechAnalyzer
  + SpeechTranscriber on macOS 26+, SFSpeechRecognizer before. The card is a
  non-activating panel that takes the keyboard without activating Hover (Spotlight's
  way); clicks reach only the card (`VoicePanel.track`, fed by the pointer poll). It sends
  the office's own `new` and `reply` messages, so the backend's checks apply; a `toast`
  within 3 s of a voice start is shown on the card. Projects and voice settings live in
  UserDefaults (`voice.*`). The Info.plist carries the microphone and speech usage strings
  and the app is signed with `macos/app.entitlements` (audio input, for the hardened runtime).
- **Thoughts and subagents are steps.** `KiroStream` turns `agent_thought_chunk` into one
  `thought` step per stretch of thinking (text in Output, capped); `OfficeState.Row` sends
  it as `k: 'thought'` and a subagent call (`DeskInfo.IsSubagent`) as `k: 'agent'`. A stop
  with replies queued moves on to the next one (`KiroSession.Go`); a reply never denies a
  pending approval.
- **Answers are Markdown, drawn without a library.** `md.js` escapes everything
  the agent wrote and emits only its own tags, so an answer can't inject HTML or
  script. Links go to the browser through Hover (`link`), web images load
  directly, and images in the session's folder load from a per-session virtual host
  (`f<key>.hover`). ```mermaid flowcharts become SVG (`diagram.js`: layered
  layout, loops set aside); other Mermaid kinds show as code. The composer's model
  pill sets the tool's default model and effort, as Settings does; its one round
  button sends, queues a reply while a run goes, or stops the run when the box is
  empty. Each tool's page can hide the tools it runs from the chat.
- **The office is a web page on purpose.** `KiroPage` shows `web/office` in a
  `WebView2CompositionControl` (a windowed WebView2 can't draw in the layered notch
  window), from one shared WebView2 environment. It is made when the page shows,
  paused and set to low memory while hidden, and disposed when the page unloads
  (the app window closes) or 30 s after it was hidden (the notch folds, Settings opens) (reopening costs
  about 1 s warm, 2.4 s cold). A page made again reopens the chat that was open
  (`KiroPage._open`) and puts the camera back (`office.view` in localStorage). The page draws at one pixel per CSS pixel without
  antialiasing, redraws its shadow map only when something moved, and drops to
  10 fps when nothing happens (30 fps while a bot walks or works). Measured: WebView2
  itself is about 94 MB private working set; the office on top of a blank page adds
  about 16 MB, so the renderer is not the cost.
- **The office has no title or close button.** The app window opens from the tray.
  The notch folds when the pointer
  leaves, on a click outside, or on Esc with nothing open in the page (`fold`).

## Conventions

- Match the surrounding style. Comments explain **why**, not what; keep them.
- Keep changes surgical — touch only what the task needs, and remove only the
  dead code your own change creates.
- Prefer the smallest change that works. No new dependencies without a reason.
- Verify by running the real thing: `dotnet build` and `dotnet test` must both be
  clean before calling a change done. For UI changes, render or run it — a
  green build is not proof the pixels are right.
- Do not commit `bin/`, `obj/`, `publish/`, or `dist/` (they are git-ignored).

## Gotchas

- **WPF vs WinForms name clashes.** WinForms is referenced only for `Screen` and
  `NotifyIcon`; its implicit usings are dropped in the csproj. Fully-qualify or
  alias when you need a WinForms type.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky;
  anchor on unique ASCII lines.
- **Line endings are CRLF** (`.gitattributes`). Tools that write LF leave a mixed
  file; normalise before committing.
- **Tests need STA + a WPF Application** for anything touching controls; see the
  `[Apartment(ApartmentState.STA)]` fixtures. `Core/Layout.cs` and `Core/Quota.cs`
  have no WPF, so their tests also run on Linux or macOS by linking those two
  files into a plain `net10.0` NUnit project. The same goes for the agents: link
  `Core/Quota.cs`, `Core/Log.cs`, `Core/Paths.cs`, `Services/KiroRunner.cs`,
  `Services/Agents.cs`, `Services/AcpHost.cs`, `Services/ComputerUse.cs` and `Owl/KiroSession.cs` with
  `KiroRunnerTests.cs` and `TestEnvironment.cs` (its stand-in ACP agent talks over
  in-memory pipes). `TestEnvironment` redirects the data folder to a temp path via
  `HOVER_DATA_DIR`. `OpenCodeHostTests.cs` runs OpenCode against a stand-in
  `opencode serve` (the same routes, auth and event stream, on localhost).
  `OpenCodeLiveTests.cs` drives the real one with a real model and is run by hand:
  `dotnet test .\tests\Hover.Tests -c Release --filter "TestCategory=LiveOpenCode"`
  (`HOVER_LIVE_MODEL` picks the model; the default is OpenCode's free
  `opencode/big-pickle`).
- **There is one `HoverNotch` window per display.** UI Automation lists them in
  z-order, so the first is often another display's. The E2E tests bind the one
  over the primary display's top centre.
- **UI Automation can't see a `Border` or a `Panel`.** Give E2E hooks to a
  control or a `TextBlock`, or give the element an automation peer. The office itself
  is a web page: UI Automation sees only the WebView2, so drive it over DevTools.
- **The Mac's E2E run is `tests/macos/e2e/run.sh`** (see docs/MACOS.md): windowless,
  in the background, against the packaged backend with stand-in agent and gh.
- **The E2E suite (`tests/Hover.E2E`) still drives the 1.x workspace** and fails
  against 2.0 until it is rewritten for the office.
