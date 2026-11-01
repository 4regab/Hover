# Windows in C#: what maps to what

This file is the plan for replacing the Go Windows app with a C# (WinUI 3) shell on top of the
existing Go backend. It was written from the code at commit `7d0a1c9` and from a real run of
the Go Windows app on GitHub's `windows-2022` runner (see [Baseline](#baseline)). Where a
statement comes from a file, the file is named.

Linux and macOS do not change. The Go packages all stay.

## The shape

- `windows/Hover` is the WinUI 3 app (`hoverai.exe`): windows, notch, tray, hotkeys, pages,
  chat, settings, and every call into Windows.
- `windows/Hover.Host` has no UI. It starts `hover-backend`, holds it in a job object, speaks
  the JSON-lines protocol, and keeps the history key (DPAPI).
- `windows/Hover.Office` draws the 3D office with Direct3D 11 (Vortice) and HLSL.
- `windows/Hover.Tests` is the `dotnet test` project.
- `hover-backend` (`cmd/hover-backend`, `internal/backend`) is the Go process that runs the
  agents, the sessions, the history, the settings and the quotas. The Mac app already uses it.

One rule decides most rows below: **the backend is the only writer of `settings.json`, the
history and `secrets.dat`.** The host only gives it the key. Then the byte formats cannot drift,
because the same Go code writes them.

## A. Go packages used on Windows

The 21 rows below cover every Hover package this prints:

```sh
GOOS=windows CGO_ENABLED=0 go list -deps ./cmd/hover | grep 4regab/Hover
```

Destination words: **Hover**, **Hover.Host**, **Hover.Office**, **Hover.Tests** are the C#
projects. **Backend** means the Go code stays and C# reaches it through the protocol (section B).
**Not needed** has its reason.

| # | Go package | What it does on Windows | Goes to | Notes |
|---|---|---|---|---|
| 1 | `internal/core` | Data folder, `settings.json`, key, sealed history, `secrets.dat`, projects, themes, single instance | **Backend** | Already used by `hover-backend`. The Windows adapters change owner: DPAPI key and `%APPDATA%` path go to **Hover.Host**; dark-mode watch, launch at login and single instance (`platform_windows.go`, `single_windows.go`) go to **Hover**. `readfile_windows.go` (rename with retries) stays in the backend. |
| 2 | `internal/agents` | ACP host, OpenCode, Claude Code, sessions, permissions, checkpoints, desk panels, editors, terminal, Discord status, job objects (`proc_windows.go`) | **Backend** | Already used. The host adds an outer job object so the backend and its tree die with the host. Discord status already runs in the backend (`DiscordStart`), so no pipe code is needed in C#. |
| 3 | `internal/diagram` | Mermaid flowchart to layout and SVG | **Backend** (`flowchart`, row 36) | Only if the native chat cannot draw the graph. WebView2 with `web/office/diagram.js` is the fallback named in the owner's decisions. |
| 4 | `internal/md` | Markdown to HTML and to blocks | **Backend** (`markdown`, row 33) | The Go code is tested against `md.js` output, so the chat shows the same blocks. |
| 5 | `internal/text` | Text shaping and line breaking for Gio | Not needed | WinUI and DirectWrite shape text. |
| 6 | `internal/raster` | CPU painter and SVG renderer for Gio | Not needed | WinUI draws. The tool logos (`internal/ui/marks.go`, `assets/kiro.svg`) become C# vector assets. |
| 7 | `internal/chat` | Chat thread layout, selection, scrolling, code colour, diff numbers | **Hover** (native chat) | Layout, selection, scroll and paint are not needed. Two functions are reused through the backend: `chat.Highlight` (row 34) and `chat.Numbered` (row 35). |
| 8 | `internal/gpu` | Binding to wgpu-native | Not needed | Direct3D 11 through Vortice replaces it. The installer stops shipping `wgpu_native.dll`. |
| 9 | `internal/office` | 3D scene, bots, desks, wall canvases, camera, picking, pacing, `office.wgsl` | **Hover.Office** | Port `scene.go`, `bot.go`, `office.go`, `canvas.go`, `page.go`, `live.go` and `office.wgsl` (to HLSL). Its input is the backend's `state` message. Fonts: `assets/PixelifySans.ttf`, `assets/Inter-Regular.ttf`. |
| 10 | `internal/quota` | Claude, Kiro, Codex, Cursor readings and the 5-minute poll; Kiro credits by day | **Backend** | Readings already flow as `quotas`. Credits need row 14. `quota.ItemShort` and `quota.Custom` (ring text) are small and are ported to **Hover**. |
| 11 | `internal/app` | Settings pages as data (`pages.go`, `view.go`), shared state (`hover.go`), resting notch content (`rest.go`), key names (`keys.go`), desk card data (`desk.go`) | Split | `hover.go` is `newBackend` in `internal/backend`. `pages.go`, `view.go`, `rest.go`, `keys.go`, `desk.go` become view models in **Hover**. What they read or write goes through section B. |
| 12 | `internal/notch` | Notch size, placement, outline, openness animation, hover rules, hit region | **Hover** | Port with the same numbers. **Hover.Tests** compares results with values printed from the Go tests. |
| 13 | `internal/platform/win` | Win32 windows, DirectComposition, tray, `RegisterHotKey`, file pickers | **Hover** (`windows/Hover/Win32/`) | `gfx.go` presents Gio frames; the C# app uses DirectComposition for the notch and the office swap chain. |
| 14 | `internal/audio` | Sound output (winmm waveOut) | **Hover** | Music and chime. |
| 15 | `internal/music` | Office beats (Ogg Vorbis loop, fade 0.32), chime | **Hover** | Reuse `office-beats.ogg` (CC0, in the repo). An Ogg decoder from NuGet is needed. Port `RampStep`. |
| 16 | `internal/screen` | Screen panel capture (`PrintWindow` per agent window over the wallpaper), voice screenshots | **Hover** | The agent's window ids come from `state.sessions[].apps`. Capture stays in C#. |
| 17 | `internal/ui` | The whole Gio interface (35 files, 12,622 lines) | **Hover** | WinUI 3 pages and controls. |
| 18 | `internal/voice` | Voice state machine, Groq, cleanup, routing, Phonon; mic capture (winmm) | **Backend** + **Hover** | Engine stays in Go (rows 41 to 43). Mic capture, the hold-to-talk hotkey and the chime are C#: `audio_windows.go` is replaced by a C# capture that streams 16 kHz mono PCM to the backend. |
| 19 | `internal/shell` | Window glue, timers, notch control, new task, chat actions, desk, voice glue, `Env` (the OS boundary) | **Hover** | `env.go` lists the Windows calls C# must provide: windows, hotkeys, tray, notifications, folder/image pickers, clipboard, open URL. Logic that calls `agents.*` or `core.*` becomes protocol calls. |
| 20 | `internal/shots` | `--shots` renderer and its fixtures | **Hover** (`--shots`) | The C# `--shots` renders inside the app (RenderTargetBitmap, D3D readback) and mirrors the names below. |
| 21 | `cmd/hover` | `hoverai.exe` entry: `--version`, `--shots DIR`, single instance, quit order | **Hover** | Keep the exe name `hoverai.exe`. Keep the icon (`internal/platform/win/hover.ico`). |

Packages that are not in the Windows build but matter here:

| Go package | Goes to | Notes |
|---|---|---|
| `internal/backend`, `cmd/hover-backend` | **Backend** (extended in section B) | Ships beside `hoverai.exe`. Builds for Windows (checked: `GOOS=windows CGO_ENABLED=0 go build ./cmd/hover-backend`). |
| `cmd/hover-data` | Test tool | Writes and reads a data folder. Used for the fixture (section C). Builds for Windows (checked by cross-compile). |
| `cmd/ui-shots`, `cmd/office-shot` | Baseline tools | Draw the Go views. Stay as they are. |
| `cmd/notch-spike` | Not needed | Windows prototype of the notch. |
| `internal/platform/linux`, `internal/platform/wayland` | Not needed | Linux only. |

Third-party code that leaves the Windows build when it switches over: Gio (`third_party/gioui.org`),
`go-text/typesetting`, wgpu-native. The Go backend keeps its own dependencies.

Not available on Windows by design (from the code, and the README platform table):
sandbox (`agents.SandboxSupported`), computer use and Cua desktops (`agents.CuaSupported`,
`agents.SpacesSupported`), agent browser (`agents.BrowserSupported`), one-click agent setup
(`agents.SetupSupported`), and helpers/delegation (`agents.OrchMcpSupported` is false on
Windows). The host does not need `spaces`, `teleport`, `spaceFiles`, `spaceView`, `computerUse*`,
`browser*` or `readClaudeCredentials` messages.

## B. The protocol

JSON lines on the backend's stdin and stdout. One writer per line. A line over 48 MiB is skipped
with a toast. The first command must be `initialize`. When stdin closes the backend stops every
tool, writes the history and settings, and exits (`internal/backend/wire.go`).

### B1. What the backend has today

Host to backend (`Backend.handle` in `internal/backend/backend.go`):

| Command | Fields | Does |
|---|---|---|
| `initialize` | `key` (base64, 32 bytes) | First command. `core.UseHostKey`. Refuses a key that cannot open an existing `agents/index.dat`. |
| `ready` | | Sends `state`, checks the tools, reads the quotas. |
| `refresh` | | Checks the tools and reads the quotas again. |
| `new` | `folder`, `tool`, `prompt`, `access`, `images` | `KiroSessions.StartAs`. Needs "agent access reviewed" (`noticeSeen`). |
| `reply` | `id` or `key`, `text`, `images` | `KiroSessions.Reply`; wakes a saved session by `key`. |
| `stop`, `answer`, `delete`, `remove`, `open`, `history` | `id` / `key` / `ask` / `answer` / `answers` | Stop a turn, answer a permission or question, delete a session, dismiss a desk, select a desk, load a saved session as `transcript`. |
| `restore`, `again` | `id`, `turn` | `KiroSessions.Rewind` (checkpoints). |
| `desk` | `id`, `what`, `arg` | Panels: `terminal`, `agents`, `browser`, `probe`, `files`, `file`, `diff`, `pr`, `linked`. Answered as `desk`. |
| `deskAction` | `id`, `what: prCreate`, `args` | Create a pull request. Answered as `deskAction`. |
| `gh` | `step`: `setup`, `cancel`, or none | GitHub CLI check and setup. Answered as `gh`. |
| `setModel` | `tool`, `model`, `effort` | Sets the tool's model and effort. |
| `getSettings`, `saveSettings` | `maxRunning`, `hover`, `noticeSeen`, `quotaItems`, `sandbox`, `discordPresence`, `kiroAutoCompact`, `kiroCompactAt`, `tools[{id, access, idle, hideSteps}]` (and macOS toggles) | A small subset of `settings.json`. Answered as `preferences`. |
| `setup`, `computerUse`, `computerUseSetup`, `spaces`, `spaceView`, `teleport`, `spaceFiles`, `browserResult`, `claudeCredentials` | | macOS only. Not used on Windows. |
| `shutdown` | | Stops everything and exits. |

Backend to host:

| Event | Carries |
|---|---|
| `initialized` (`version: 1`), `backendFailure` (`text`), `toast` (`text`) | Start, a fatal start error, a message to show. |
| `state` | `canStart`, `maxRunning`, `spaces`, `folder`, `tool`, `tools[]` (ready, hint, models, efforts, access), `sessions[]` (id, key, tool, bot, seat, title, folder, ctx, access, stage, act, pose, file, ask, turns[], apps, testing, space, files), `history[]`. |
| `transcript` | One saved session, in the same shape as `sessions[]`. |
| `ended` | `title`, `text`, `tool`, `task`, `ok`. |
| `quotas` | `values{ id: {ok, used, detail} }`. Every 5 minutes and on `refresh`. |
| `preferences`, `gh`, `desk`, `deskAction` | Answers. |
| `computerUse`, `spaces`, `space`, `teleport`, `browser`, `readClaudeCredentials` | macOS only. |

Compatibility with the Mac host: it reads messages as dictionaries and sends any type it does
not know to the office page (`macos/Sources/Hover.swift`, the `default:` branches). So **new
fields on existing messages are safe**, and **new events are safe when they are sent only after a
new command that only the Windows host sends**. Keep every change additive and cover it with a Go test.

### B2. What is missing for Windows

"Phase" is the C# phase from the owner's plan (1 skeleton, 2 notch, 3 office, 4 tasks and chat,
5 settings and the rest). "Field" rows add to an existing message. The Go function named is the
one the Go Windows app calls today; the backend command should call the same function.

**Start-up and lifecycle**

| # | Name | Direction | Go function it reuses | Phase |
|---|---|---|---|---|
| 1 | `initialize` with no `key`: history off, backend still runs | host to backend | The no-key branch of `app.Start` (`History` stays nil when `core.GlobalCrypto()` is nil). Today `newBackend` returns "The history key is not available." | 1 |
| 2 | Start step: drop the old planner files | backend start | `core.DropPlanner(core.Support())` | 1 |
| 3 | Start step: continue a busy Kiro turn | backend start | `KiroSessions.SetRetryWhenBusy(settings.KiroRetryBusy)` (set in `app.With`, missing in `newBackend`) | 1 |
| 4 | Start step: Kiro credits by day, redone when history changes | backend start | `quota.NewCredits(history, quota.DailyPath(), changed)` and `history.OnChanged(Credits.Poke)` | 5 |
| 5 | Start step: follow Kiro Web tasks that were cut off | backend start, after `initialized` | `KiroSessions.ReattachCutOff` (called in `cmd/hover/main_windows.go`) | 4 |
| 6 | Remove the old "Hover Service" scheduled task, once | host (C#) | `app.oldServiceRemoval` (`schtasks /Query`, `/End`, `/Delete`; marker `old-service-removed`). OS call, so it lives in **Hover.Host**. | 1 |

**Settings**

| # | Name | Direction | Go function it reuses | Phase |
|---|---|---|---|---|
| 7 | `settings` (request) and `settings` (event): the whole model as JSON | both | `core.Settings.Model()` then `Model.ToJSON()`. The Mac `preferences` message has only a subset. | 2 |
| 8 | `setSetting` `{key, value}` for: `Appearance`, `Theme`, `WorkspaceSize`, `ScWorkspace`, `ChatView`, `LastEditor`, `KiroFolder`, `KiroRetryBusy` | host to backend | `Settings.SetAppearance`, `SetTheme`, `SetWorkspaceSize`, `SetScWorkspace`, `SetChatView`, `SetLastEditor`, `SetKiroFolder`, `SetKiroRetryBusy` | 2 (shortcut, appearance), 5 (rest) |
| 9 | `projects` add / update / remove, default workspace | host to backend | `Settings.AddProject`, `UpdateProject`, `RemoveProject`, `SetDefaultWorkspace`, `core.ResolveFolder`, `core.EnsureFolder` | 5 |
| 10 | `themes` (list) | both | `core.InstalledThemes` | 5 |
| 11 | `themeRead` `{path}` and the palette | both | `core.ReadTheme`, `core.PaletteFromTheme`, `core.HoverPalette`, `core.HexColor` | 5 |
| 12 | `mcp` get / save / enable / remove (Kiro's `mcp.json`) | both | `agents.LoadMcp`, `SaveMcp`, `SetMcpDisabled`, `RemoveMcp`, `DraftOf`, `agents.McpFile(agents.McpHome())` | 5 |
| 13 | `kiroAgents` `{folder}` | both | `agents.KiroAgents` | 5 |
| 14 | `credits` (Kiro credits by day) | both | `quota.Credits.View` | 5 |
| 15 | `secret` has / set `{name, value}`; answer is `{has, persistent}` and never the value | both | `core.SystemSecrets()` then `Has`, `Set`, `Persistent` | 5 |
| 16 | Launch at login | host (C#) | `core.SystemAutostart` writes the `Run` value `"<exe path>"`. The backend must not do it: `os.Executable()` there is `hover-backend.exe`. C# writes the same value name (`Hover`) with the path of `hoverai.exe`. | 5 |

**Notch, tray, office (fields on existing messages)**

| # | Name | Direction | Go function it reuses | Phase |
|---|---|---|---|---|
| 17 | Field `state.sessions[].status` (e.g. "Reading the code") | backend to host | `agents.Status` (used by `app.Hover.WorkingText`) | 2 |
| 18 | Field `state.sessions[].activity` `{verb, obj}` | backend to host | `agents.Activity` (used by `app/rest.go`) | 2 |
| 19 | Fields `ended.body` and `ended.took` (seconds) | backend to host | `agents.TextFirstLine(agents.Plain(r.Text))` and `EndedAt.SecsSince(StartedAt)` (from `app.Hover.ended`) | 2 |

The office itself (phase 3) needs nothing more: `state.sessions[]` already carries `bot`,
`seat`, `stage`, `act`, `pose`, `file`, `ctx`, `ask`, `turns[].steps` and `apps`, which is what
`agents.Push` gives the Go office. The camera, day/night and music choices are kept by the host
in `office.json` (section C).

**Tasks and chat**

| # | Name | Direction | Go function it reuses | Phase |
|---|---|---|---|---|
| 20 | `new` fields `cloud` (repos), `chips`, `model`, `effort` | host to backend | `KiroSessions.StartBound(tool, folder, text, images, &access, cloud, ext)` (`shell/newtask.go`) | 4 |
| 21 | `reply` field `chips` | host to backend | `KiroSessions.ReplyMsg(id, agents.Msg{Text, Images, Chips})` and `agents.CheckChips` | 4 |
| 22 | `pause` `{id}` | host to backend | `KiroSessions.Pause` | 4 |
| 23 | `queueSendNow` `{id, uid}` | host to backend | `KiroSessions.SendNowQueued` | 4 |
| 24 | `queueRemove` `{id, uid}`; answer returns the text for the composer | both | `KiroSessions.RemoveQueued` | 4 |
| 25 | `queueResume` `{id}` | host to backend | `KiroSessions.ResumeQueue` | 4 |
| 26 | `rename` `{id, name}` | host to backend | `KiroSessions.Rename` | 4 |
| 27 | `switchProvider` `{id, to}` | host to backend | `KiroSessions.SwitchProvider`, `agents.ParseTarget` | 4 |
| 28 | `fork` `{id, turn, to}` and `bringBack` `{id}` | host to backend | `KiroSessions.Fork`, `KiroSessions.BringFindingsBack` | 4 |
| 29 | Fields `sessions[].turns[].uid`, `.chips`, `.switchTo`; `sessions[].cloud`, `.ext`, `.held`, `.commands` | backend to host | `KiroTurn.UID`, `Chips`, `SwitchTo`; `KiroSession.Cloud`, `Ext`, `Held`, `Commands` | 4 |
| 30 | `cloudRepos` (Kiro Web repo list) | both | `Runtime.Repos` (Kiro) | 4 |
| 31 | `webSessions` list and `adoptWeb` `{id}` | both | `Runtime.CloudSessions`, `Runtime.CloudTranscript`, `KiroSessions.AdoptCloud` | 4 |
| 32 | `cliSessions` list and `adoptCli` `{id}` | both | `Runtime.LocalSessions`, `Runtime.LocalTranscript`, `KiroSessions.AdoptLocal` | 4 |
| 33 | `markdown` `{src, folder, files}`: blocks, or HTML | both | `md.Parse` (blocks) and `md.Markdown` (HTML); local images through `md.ImageFor` | 4 |
| 34 | `highlight` `{info, text}`: coloured byte ranges | both | `chat.Highlight` | 4 |
| 35 | `diffLines` `{diff}`: lines with old/new numbers | both | `chat.Numbered` | 4 |
| 36 | `flowchart` `{src}`: graph layout, or "cannot read" | both | `diagram.Layout` (and `diagram.Flowchart` for SVG) | 4 |
| 37 | `term` run / interrupt / clear `{id, cmd}` and a `term` event with entries | both | `agents.NewTerm`, `Term.Run`, `Interrupt`, `Clear`, `View` (PowerShell in the session's folder) | 4 |
| 38 | `writeFile` `{id, path, text}` (Files tab) | host to backend | `agents.DeskWriteFile` | 4 |
| 39 | `editors` (list) and `openIn` `{id, editor, file, line}` | both | `agents.AvailableEditors`, `agents.EditorChoices`, `agents.OpenEditorOrFirst`, `agents.OpenFileManager` | 4 |
| 40 | `inspectFolder` `{folder}`: branch, worktree | both | `agents.Inspect` (used by `shell.branchOf`) | 4 |

**Voice** (phase 5)

| # | Name | Direction | Go function it reuses |
|---|---|---|---|
| 41 | `voicePress` `{trial, dictate, activeProject}`, `voiceAudio` `{pcm}` (base64, 16 kHz mono, 16-bit) or `{error}` (the text `voice.Source.Failed` returns), `voiceRelease`, `voiceCancel`, `voiceDismiss`, `voiceStartNow`, `voiceEdit` `{task}`, `voiceChooseAgent`, `voiceChangeAgent`, `voiceToggleCloud`, `voiceChangeFolder`, `voiceChangeRepo`, `voiceRetry`, `voiceUnattach` | host to backend | `voice.Voice.Press`, `Dictate`, `Release`, `Cancel`, `Dismiss`, `StartNow`, `Edit`, `ChooseAgent`, `ChangeAgent`, `ToggleCloud`, `ChangeFolder`, `ChangeRepo`, `Retry`, `Unattach`. The microphone is a remote `voice.Source` fed by `voiceAudio`; `voice.New` takes it as its `Open` argument. The hooks (`Router`, `Start`, `Available`, `ActiveProject`) are `app.Hover.Runner`, `KiroSessions.StartIn`, `agents.Known`, and the field sent with `voicePress`. "Take a screenshot" said aloud stays in Go: `voice.go` calls `screen.Whole` and `core.SaveImages` inside the backend. |
| 42 | `voice` event: stage, level, seconds, preview (heard, task, folder, tool, model, access, countdown, cloud, repo), pending, error, started session | backend to host | `voice.Voice.Stage`, `OnChange`, `CountdownTotal`, `CanStart`, `FolderChoices`, `ReadyTools`, `Shots` |
| 43 | `phonon` get / download / repair / cancel / remove, `phonon` event (install state); `groqCheck` | both | `voice.NewPhonon`, `Phonon.State`, `Facts`, `Download`, `Repair`, `Cancel`, `Remove`, `OnChange`; `voice.CheckGroq` |

Done in C#, no protocol: microphone list and capture (winmm; use the `waveInGetDevCaps` name, cut to 31 characters, because `Voice.Microphone` in `settings.json` stores that name and `OpenMic` matches it that way), the hold-to-talk hotkey
(`RegisterHotKey` and `GetAsyncKeyState` release), the chime (`music.Chime`), pickers, clipboard, notifications, tray menu, single instance, window
placement.

Order of work for Phase 1: rows 1 to 3 (the skeleton can run without them, but the smoke test
should match the Go app's start-up), and row 6.

## C. Data formats

The C# app opens the same data folder as the Go app. Nothing is converted. Read these from the
Go code: `internal/core/{paths,crypto,json,settings,history,store,platform_windows}.go`.

### Where

`%APPDATA%\Hover` (the roaming known folder), or `HOVER_DATA_DIR` when set (made if missing).
If `Hover` is missing and an old `Noty` folder exists, `Noty` is renamed to `Hover`
(`core.ResolveDataDir`). The host sets `HOVER_DATA_DIR` for the backend it starts.

### Files

| File | Written by | Format |
|---|---|---|
| `note.key` | **Hover.Host** | The 32-byte history key, wrapped with DPAPI. `ProtectedData.Protect(key, null, DataProtectionScope.CurrentUser)` in .NET; Go calls `CryptProtectData` with no entropy and `CRYPTPROTECT_UI_FORBIDDEN`. Both open each other's file. It is 262 bytes on the CI runner. Plain `File.WriteAllBytes`. |
| `settings.json` | **Backend** | UTF-8, no BOM, indented two spaces, `\r\n` on Windows (`\n` on Linux and Mac), no newline at the end. Every property in the order of `Model.ToJSON`, nulls included. Optional toggles (`ComputerUse`, `ChatView`, `Sandbox`, `AgentBrowser`, `AgentSpaces`, `SpaceImage`, `LastEditor`, `KiroAutoCompact`, `KiroCompactAt`, `KiroRetryBusy`, `DiscordPresence`) are written only once set. Strings use .NET's default escaping: `& ' + < > \`` and every non-ASCII character become `\uXXXX` with upper-case hex. Written 400 ms after the last change, to `settings.json.tmp`, then renamed. |
| `agents\index.dat` | **Backend** | Sealed (below). Plain text is a compact JSON array of `{Key, Tool, Title, Folder, Updated, State, Turns, Credits}`. A bad index is renamed `index-<guid>.bad` and rebuilt from the session files. |
| `agents\<32 hex>.dat` | **Backend** | Sealed. Plain text is one compact JSON session: `Key, Tool, Folder, Title, AcpId, Context, Turns, Updated, Access`, then `Cloud` and `Ext` only when used. Times are ISO 8601 strings. |
| `secrets.dat` | **Backend** | Sealed. Plain text is a compact JSON object `{ "<name>": "<value>" }`. Names: `voice.groq`, `cleanup.gemini`, `cleanup.openai`, `cleanup.custom`. Written to `secrets.dat.tmp`, then renamed. Never in `settings.json` or the log. Without a key the value is kept in memory for this run only. |
| `hover.log` | Both | One line per event: `HH:mm:ss.fff hover: message`, `\r\n`, appended. The host may append to it. |
| `backend.json` | Backend | `{"MaxRunning":3}`, compact. |
| `office.json` | **Hover** | A flat JSON object of strings: `view` (`"x,y,zoom"`, e.g. `"0,0,1"`), `time` (`"day"` or `"night"`), `beats` (`"on"` or `"off"`). Not sealed. Both builds read it. |
| `kiro-images\`, `checkpoints\`, `phonon\`, `kiro-usage.json`, `repos.json`, `orch\runs.dat`, `old-service-removed` | Backend or Go app | Backend owns the first four. `repos.json` (Kiro Web repo list cache) is written by the Go shell today. `orch\runs.dat` and the marker are made by the Go app on first start; the C# app needs neither. |

### Sealing

AES-256-GCM, key = the 32 bytes from `note.key`. A sealed file is `nonce (12 bytes) ‖ ciphertext ‖ tag (16 bytes)`, no associated data, a new random nonce for each write. The plain text is UTF-8 JSON. Open failures read as "no data" and are logged; they never delete a file. This is the same as the old .NET `Crypto.cs` (`git show 0ecbc07:src/Hover/Core/Crypto.cs`).

### The key rules for the host

These are `core.LoadOrCreateCrypto` with the DPAPI guard. Match them exactly:

1. `note.key` exists and unwraps to 32 bytes: use it.
2. It unwraps to another length: rename it to `note.key.unreadable-<yyyyMMddHHmmss>` (never overwrite), then make a new key.
3. DPAPI refuses (another user, reset profile): the same rename, then a new key.
4. The file cannot be read now (locked, transient): **no key this run**. Do not replace it. Start the backend without a key (row 1), so no history is written that the next run cannot open.
5. No `note.key`: make 32 random bytes, wrap, write it, then use it. (On Windows there is no earlier key store to inherit.) If `agents\index.dat` already exists, this new key cannot open it, and rule 6 stops the backend before any file is changed.
6. Send the key once, base64, as the first command. The backend then refuses to start if `agents\index.dat` exists and does not open with it, and answers `backendFailure`. The host shows that text and does not touch any file.

The host never writes `settings.json`, `agents\*` or `secrets.dat`.

### Fixture and the cross-read

`note.key` is bound to the Windows user, so a fixture written on Linux (where `note.key` is the
raw 32 bytes in a private file) cannot be opened by the Windows app. The Go-written fixture must
be made and read inside **one Windows job**. Put this in the Phase 1 workflow, in `pwsh`, after
`actions/setup-go`:

```powershell
$fx = Join-Path $env:RUNNER_TEMP 'fixture-go'
New-Item -ItemType Directory -Force $fx, "$fx\project" | Out-Null
Set-Content -Path "$fx\answer.md" -Value 'Sample **answer**.'
$env:HOVER_DATA_DIR = $fx      # Go's log goes here, not to the runner's %APPDATA%
$env:CGO_ENABLED = '0'
go build -o "$fx\hover-data.exe" ./cmd/hover-data
& "$fx\hover-data.exe" write $fx "$fx\project" 2 "$fx\answer.md"   # prints the session key
& "$fx\hover-data.exe" dump  $fx                                   # expect: history: 1 sessions
& "$fx\hover-data.exe" check $fx                                   # exit 0: Go would save the same bytes
$env:HOVER_FIXTURE_GO = $fx    # the C# tests read this folder
dotnet test windows/Hover.Tests
```

What `write` makes: `settings.json`, `note.key` (DPAPI), `agents\index.dat` and one sealed
session of two turns ("A long rich conversation"). It makes **no `secrets.dat`**: `hover-data`
has no secrets command. Because the backend writes and reads `secrets.dat` itself with
`core.Secrets`, the C# side needs no secrets code; to test it, call row 15 against a backend and
check the file with a small `hover-data secret` command. That command does not exist yet and
`cmd/hover-data` is outside the Phase 1 paths, so someone must be given that file.

The C# test then does two things in the same job:

1. **Go to C#.** In `Hover.Host`, read `$fx\note.key` through DPAPI, start `hover-backend` with
   `HOVER_DATA_DIR=$fx` and `initialize`, and expect a `state` whose `history` lists the session
   key that `write` printed. Also open `agents\<key>.dat` in the test with the sealing rules
   above (an independent check of the byte format) and compare the plain text with the Go
   output.
2. **C# to Go.** In a second folder `$fx2`, let the host make `note.key` (DPAPI), start the
   backend, run one stand-in agent task (as `tests/macos/backend-smoke.py` does), then `shutdown`.
   Then run `hover-data.exe check $fx2` (exit 0) and `hover-data.exe dump $fx2` (the session is
   listed). Also run `hover-data.exe check` on a folder where the C# side only set a setting
   with row 8, to cover `settings.json`.

On a Linux machine the same tests can run against `evidence/phase0/fixture-linux` if the key
guard is injectable (on Linux `note.key` is the raw key). That is a convenience, not a proof for
DPAPI.

## D. How this branch is verified

Windows is GitHub's `windows-2022` runner. Nothing here runs on the owner's PC.

- **`ci.yml`**, run by hand on the branch (`gh workflow run ci.yml --ref windows-csharp`). It
  checks and tests the Go code on three systems, builds the Go Windows app and installer and
  drives them (`tools/app-smoke.ps1`, `tools/installer-check.ps1`), and its `pictures` job draws
  every Go view on WARP (artifact `pictures-windows`; app and installer reports in
  `windows-reports`). It cancels an older run on the same ref, so only one item dispatches it at
  a time.
- **`windows-csharp-dev.yml`** (made in Phase 1), on every push to `windows-csharp`, with one
  concurrency group per commit. It builds the C# solution, runs `dotnet test`, runs the Go
  fixture steps from section C, and runs the C# `--shots`. Phase 6 folds it into `ci.yml`.
- **Local on Linux**: `dotnet build -p:EnableWindowsTargeting=true` and the headless tests that
  need no Windows API. Useful for a quick compile; the runner is the build that counts.
- **Compare pictures** with the baseline (below). The owner's rule applies: fix visible
  breakage and clear mismatches, not small spacing differences.

### Baseline

Run: <https://github.com/4regab/Hover/actions/runs/41283431721> (`ci.yml`, `workflow_dispatch`
on `windows-csharp` at `7d0a1c9`). Every job succeeded: `version`, `check`, `test` on Ubuntu,
Windows and macOS, `linux`, `windows`, `macos`, `pictures` (`release` is skipped on a dispatch).

- `pictures-windows`: 387 PNGs of every Go view on WARP, office views included. By name
  prefix: chat 91, settings 88, voice 82, desk 71, office 33, expand 13, notch 8, new 1.
- `windows-reports`: four pictures from the real app (`1-rest.png`, `2-open.png`,
  `3-folded.png`, `4-app-window.png`), `report.json` for the app drive and for the installer
  check, and the data folder the app made.
- The Go app at rest used 70.8 MB private memory and 43.6 MB working set. With the app window
  open: 483.4 MB and 230.5 MB. The C# app should be measured the same way
  (`tools/app-smoke.ps1`).

Findings:

- `docs/development/architecture.md` says `ci.yml` "runs no Go tests". That is out of date:
  `ci.yml` has a `test` job on three systems since `7d0a1c9`. Not changed here.
- The installer check first installed the published `Hover-Setup-5.0.8.exe` (the oldest Go
  release it finds that is 5.0.3 or later and not equal to `VERSION`), then this branch's setup,
  built as 5.0.4 because `VERSION` says 5.0.4. So it installed a lower number over a higher one
  and Inno Setup accepted it. `run_key_before` and `run_key_after` both point to
  `...\Programs\Hover\hoverai.exe`, so a C# installer must keep that path.
- The data folder after the app drive holds `hover.log`, `note.key` (262 bytes), `office.json`
  (`{"view":"0,0,1"}`), `orch\runs.dat` and `old-service-removed`.
