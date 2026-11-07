# Windows in C#: what is where

The Windows app is a C# (WinUI 3) program over the Go backend, the same split macOS uses. This file
is the map: which C# project took over each Go package, which backend commands the Windows host
uses, the data formats both apps share, and how it is all checked. How to build and run it is
[windows.md](windows.md); the wire format of the backend is
[architecture.md](architecture.md#backend-and-host-boundary). Linux and macOS do not change, and the
Go packages all stay. The Go Windows app is kept as `build.ps1 legacy` until the owner switches over.

## The shape

- `windows/Hover` is the WinUI 3 app (`hoverai.exe`): windows, notch, tray, hotkeys, the hosts
  that tie the libraries to the backend's messages, and every call into Windows that no library owns.
- `windows/Hover.Host` has no UI. It starts `hover-backend`, holds it in a job object, speaks
  the JSON-lines protocol, and keeps the history key (DPAPI).
- `windows/Hover.Notch`, `Hover.Chat`, `Hover.Settings` and `Hover.Voice` are libraries without
  WinUI: the notch's rules, the panels over the office, the Settings pages, voice and sound. They are
  drawn through an `ICanvas` and tested on any system.
- `windows/Hover.Office` draws the 3D office with Direct3D 11 (Vortice) and HLSL.
- `hover-backend` (`cmd/hover-backend`, `internal/backend`) is the Go process that runs the
  agents, the sessions, the history, the settings, the quotas and the voice flow. The Mac app uses it too.

One rule decides most rows below: **the backend is the only writer of `settings.json`, the
history and `secrets.dat`.** The host only gives it the key. Then the byte formats cannot drift,
because the same Go code writes them.

## A. Go packages used on Windows, and where each went

The 21 rows cover every Hover package this prints:

```sh
GOOS=windows CGO_ENABLED=0 go list -deps ./cmd/hover | grep 4regab/Hover
```

**Backend** means the Go code stays and C# reaches it through the protocol (section B). **Not needed**
has its reason.

| # | Go package | What it does on Windows | Where it is now |
|---|---|---|---|
| 1 | `internal/core` | Data folder, `settings.json`, key, sealed history, `secrets.dat`, projects, themes, single instance | **Backend.** DPAPI key and the `%APPDATA%` path: `Hover.Host` (`HistoryKey.cs`, `DataFolder.cs`); single instance: `Hover.Host/InstanceLock.cs`; launch at login: `Hover.Settings/Windows/RunValue.cs`; dark-mode watch: `Hover/Settings/SettingsPane.cs`. |
| 2 | `internal/agents` | ACP host, OpenCode, Claude Code, sessions, permissions, checkpoints, desk panels, editors, terminal, Discord status, job objects | **Backend.** The host adds an outer job object (`Hover.Host/JobObject.cs`) so the backend and its tree die with the host. The Discord status is shown by the host (`initialize {hostDiscord: true}`; `Hover/Discord/DiscordHost.cs`, `Hover.Settings/Discord/DiscordPresence.cs`). |
| 3 | `internal/diagram` | Mermaid flowchart to layout and SVG | **Backend** (`flowchart`). The chat draws the SVG natively (`Hover.Chat/SvgScene.cs`). |
| 4 | `internal/md` | Markdown to HTML and to blocks | **Backend** (`markdown`). |
| 5 | `internal/text` | Text shaping and line breaking for Gio | Not needed: DirectWrite (`Hover/Chat/DwShaper.cs`). |
| 6 | `internal/raster` | CPU painter and SVG renderer for Gio | Not needed: Direct2D (`Hover/Chat/D2DCanvas.cs`). |
| 7 | `internal/chat` | Chat thread layout, selection, scrolling, code colour, diff numbers | `Hover.Chat` (`ChatThread.cs`, `ThreadPainter.cs`, `Selection.cs`, `Scroll.cs`). Code colour and diff numbers come from the backend (`highlight`, `diffLines`). |
| 8 | `internal/gpu` | Binding to wgpu-native | Not needed: Direct3D 11 through Vortice. The installer no longer ships `wgpu_native.dll` (and removes it over a 5.x install). |
| 9 | `internal/office` | 3D scene, bots, desks, wall canvases, camera, picking, pacing, `office.wgsl` | `Hover.Office` (`Scene.cs`, `Bot.cs`, `OfficeController.cs`, `OfficeWalls.cs`, `Render/OfficeRenderer.cs`, the HLSL); `Hover/Office/OfficeHost.cs` puts it in the notch and the app window. Its input is the backend's `state`. |
| 10 | `internal/quota` | Claude, Kiro, Codex, Cursor readings and the 5-minute poll; Kiro credits by day | **Backend** (`quotas`, `credits`). The ring text: `Hover.Notch/Island.cs`. |
| 11 | `internal/app` | Settings pages as data, shared state, resting notch content, key names, desk card data | Split: `hover.go` is `newBackend` in `internal/backend`; `pages.go` and `view.go` are `Hover.Settings/Pages`; `rest.go` is `Hover.Notch/Island.cs`; `keys.go` is `Hover.Notch/Shortcut.cs`; `desk.go` is `Hover.Chat/Desk/DeskRows.cs`. |
| 12 | `internal/notch` | Notch size, placement, outline, openness animation, hover rules, hit region | `Hover.Notch` (`Geometry.cs`, `NotchController.cs`, `PathData.cs`). |
| 13 | `internal/platform/win` | Win32 windows, DirectComposition, tray, `RegisterHotKey`, file pickers | `Hover/Notch` (`NotchWindow.cs`, `NotchSwapChain.cs`, `Native.cs`), `Hover/Settings/Dialogs.cs`, `Hover/Chat/Win32Clipboard.cs`. |
| 14 | `internal/audio` | Sound output (winmm waveOut) | `Hover.Voice/AudioOutput.cs`. |
| 15 | `internal/music` | Office beats (Ogg Vorbis loop, fade 0.32), chime | `Hover.Voice/Beats.cs` (NVorbis decodes `office-beats.ogg`, linked from `internal/music`); `Hover/Voice/BeatsHost.cs`. |
| 16 | `internal/screen` | Voice screenshots | They stay in the backend. The C# app has no Screen panel. |
| 17 | `internal/ui` | The whole Gio interface | `Hover.Chat/Ui` (the panels over the office), `Hover.Settings/Ui` (Settings), `Hover/Chat` and `Hover/Settings` (the hosts that draw them in the notch and the app window). Menus (`OfficeUi.Menus.cs`, the Settings pickers, the reply box's `@` list, the desk's Open file in) are no taller than the window less 16 px and scroll inside with `Hover.Chat/MenuScroll.cs` (`pickMenu` and `modelMenu` in `officelist.go` and `officemenus.go`). The first-task note (`notice` in `officemenus.go`) is `OfficeUi.Notice.cs`: it shows while the settings model says `KiroNoticeSeen` is false, and Got it sends `saveSettings {noticeSeen: true}`. The pointer's shape comes from `OfficeUi.CursorAt` (the hand over a button, the text cursor over a box of words, the resize cursor on the list's edge): the window sets it on the overlay (`UiOverlay`), the notch on its own window (`WM_SETCURSOR`). The list at the side of the chat view groups chats by folder, folds a folder at its heading, deletes a row after a confirm and is resized at its edge (200 to 420). Code and tables wider than the column scroll sideways with the wheel and Shift, a sideways wheel, or the bar under the box. The app window opens at 1200 x 620 (scaled) and cannot be made smaller than 880 x 480 (`windows/Hover/WindowMinimum.cs`, as `internal/shell/env_windows.go`). One wheel notch moves 60 DIPs everywhere (`OfficeUi.WheelStep`, Gio's 120 units at twice the scale); the "Latest" button shows 160 px above the end of the thread; the running step shimmers (`ThreadPainter`, one band every 2 s) and the panels repaint ten times a second while a run is live; a new width keeps the reader at the same turn and the same share of it (`internal/shell/chat.go`). A question taller than the pane scrolls inside its card (`MenuScroll`), the desk card's status box scrolls with the wheel, reply chips wrap onto three rows and the rest is counted in a "+N" chip, and a button under a panel drawn over it neither lights nor answers (a toast is only shown). `EditBox` has undo (Ctrl+Z; Ctrl+Y or Ctrl+Shift+Z redo) in the panels and in Settings: a step is a word, a pause of a second, a run of Backspace or Delete, a paste or a cut. The notch card breaks a long command, path or line of a change at any character and grows with it (up to 150), and dims the path over a change (`NotchRenderer.Card`). |
| 18 | `internal/voice` | Voice state machine, Groq, cleanup, routing, Phonon; mic capture (winmm) | **Backend** (the engine) and `Hover.Voice` (`Microphone.cs`, `VoiceHotkey.cs`, `VoiceService.cs`, the card). |
| 19 | `internal/shell` | Window glue, timers, notch control, new task, chat actions, desk, voice glue, `Env` | `Hover` (`App.cs`, `NotchHost.cs`, `OfficeHost.cs`, `UiCore.cs`, `BackendSession.cs`); what called `agents.*` or `core.*` is a protocol call. |
| 20 | `internal/shots` | `--shots` renderer and its fixtures | `Hover` (`Shots.cs`, `Chat/ChatShots.cs`, `Notch/NotchShots.cs`, `Settings/SettingsShots.cs`, `Voice/VoiceShots.cs`): rendered inside the app, JPEG, at most 1400 px wide, with the Go pictures' names. |
| 21 | `cmd/hover` | `hoverai.exe` entry: `--version`, `--shots DIR`, single instance, quit order | `Hover/Program.cs`, `App.cs`, `LaunchOptions.cs`. The exe is `hoverai.exe` and the icon is the same. |

Packages that are not in the Windows build but matter here:

| Go package | Notes |
|---|---|
| `internal/backend`, `cmd/hover-backend` | The **Backend**. Ships beside `hoverai.exe`. |
| `cmd/hover-data` | Test tool: writes and reads a data folder, and keeps and checks a secret (`secret set`, `secret check`). Not shipped. |
| `cmd/ui-shots`, `cmd/office-shot` | Draw the Go views. Stay as they are. |
| `cmd/notch-spike` | The Windows prototype of the notch. Not needed. |
| `internal/platform/linux`, `internal/platform/wayland` | Linux only. |

Third-party code that left the Windows build when it switched over: Gio (`third_party/gioui.org`),
`go-text/typesetting`, wgpu-native (they still build the Go app: Linux and `build.ps1 legacy`). The
Go backend keeps its own dependencies; the C# app carries the NuGet packages listed in
`THIRD-PARTY-NOTICES.txt`.

Not available on Windows by design (from the code, and the README platform table):
sandbox (`agents.SandboxSupported`), computer use and Cua desktops (`agents.CuaSupported`,
`agents.SpacesSupported`), agent browser (`agents.BrowserSupported`), one-click agent setup
(`agents.SetupSupported`), and helpers/delegation (`agents.OrchMcpSupported` is false on
Windows). The host sends none of `spaces`, `teleport`, `spaceFiles`, `spaceView`, `computerUse*`,
`browser*` or `readClaudeCredentials`, and Settings shows those rows off with the Go notes. One thing
of the Go app is not ported: the greeting on resume or unlock (`Greet`).

## B. The protocol

JSON lines on the backend's stdin and stdout. One writer per line. A line over 48 MiB is skipped
with a toast. The first command must be `initialize`. When stdin closes the backend stops every
tool, writes the history and settings, and exits (`internal/backend/wire.go`). The commands and
events the Mac host already used are in `internal/backend/backend.go`; the Windows host added
the rows below, all additive, so the Swift host keeps working (it reads messages as dictionaries and
ignores what it does not know; new events are sent only in answer to new commands). The C# side is typed in
`Hover.Host/Protocol`. The messages
themselves, with their fields, are in [architecture.md](architecture.md#windows-host-commands).

**Start-up and lifecycle**

| # | Command or field | Go function it reuses |
|---|---|---|
| 1 | `initialize` with no `key` (or `null`): history off, the backend still runs | `core.UseNoHostKey` |
| 2 | start step: drop the old planner files | `core.DropPlanner` |
| 3 | start step: continue a busy Kiro turn | `SetRetryWhenBusy(settings.KiroRetryBusy)` |
| 4 | `initialize` `credits: true`: Kiro credits by day | `quota.NewCredits`, `history.OnChanged(Poke)` |
| 5 | `initialize` `reattach: true`: follow Kiro Web tasks that were cut off | `KiroSessions.ReattachCutOff` |
| 6 | Remove the old "Hover Service" scheduled task, once | host (C#): `Hover.Host/OldService.cs` |

**Settings**

| # | Command or field | Go function it reuses |
|---|---|---|
| 7 | `settings`: the whole model as JSON | `Settings.Model().ToJSON()` |
| 8 | `setSetting` `{key, value}` (ten keys, `Voice` and `ToolAgent` among them) | the `Settings.Set...` of each |
| 9 | `projects` list / add / update / remove / default workspace | `AddProject`, `UpdateProject`, `RemoveProject`, `SetDefaultWorkspace` |
| 10, 11 | `themes`, `themeRead`, `palette` | `InstalledThemes`, `ReadTheme`, `PaletteFromTheme`, `HoverPalette` |
| 12 | `mcp` get / save / enable / remove (Kiro's `mcp.json`) | `LoadMcp`, `SaveMcp`, `SetMcpDisabled`, `RemoveMcp` |
| 13 | `kiroAgents` | `agents.KiroAgents` |
| 14 | `credits` | `quota.Credits.View` |
| 15 | `secret` has / set; the answer is never the value | `core.SystemSecrets()` |
| 16 | Launch at login: the `Run` value `Hover`, written by the host (the backend's `os.Executable()` is `hover-backend.exe`) | host (C#): `Hover.Settings/Windows/RunValue.cs` |

**Notch, tray, office (fields on existing messages)**

| # | Field | Go function it reuses |
|---|---|---|
| 17, 18, 19 | `sessions[].status`, `sessions[].activity {verb, obj}`, `ended.body`, `ended.took` | `agents.Status`, `agents.Activity`, `TextFirstLine(Plain())` |

The office itself needs nothing more: `state.sessions[]` carries `bot`, `seat`, `stage`, `act`, `pose`,
`file`, `ctx`, `ask`, `turns[].steps` and `apps`, which is what `agents.Push` gives the Go office. The
camera, day/night and music choices are kept by the host in `office.json` (section C).

**Tasks and chat**

| # | Command or field | Go function it reuses |
|---|---|---|
| 20 | `new` fields `cloud`, `model`, `effort` (no `chips`: the new-task box has none) | `StartBound`, `setModel` |
| 21 | `reply` field `chips` | `CheckChips`, `ReplyMsg` |
| 22 to 25 | `pause`, `queueSendNow`, `queueRemove`, `queueResume` | `Pause`, `SendNowQueued`, `RemoveQueued`, `ResumeQueue` |
| 26 | `rename` | `Rename` |
| 27, 28 | `switchProvider`, `fork`, `bringBack` | `SwitchProvider`, `Fork`, `BringFindingsBack` |
| 29 | `turns[].uid/.chips/.switchTo`, `sessions[].cloud/.ext/.held/.commands`, `sessions[].kiroId` (Kiro Web tasks only) | the `KiroTurn` and `KiroSession` fields |
| 30 to 32 | `cloudRepos`, `webSessions`, `adoptWeb`, `cliSessions`, `adoptCli` | `Runtime.Repos`, `CloudSessions`, `LocalSessions`, `AdoptCloud`, `AdoptLocal` |
| 33 to 36 | `markdown`, `highlight`, `diffLines`, `flowchart` | `md.Parse`, `chat.Highlight`, `chat.Numbered`, `diagram.Layout` |
| 37 to 40 | `term`, `writeFile`, `editors`, `openIn`, `inspectFolder` | `NewTerm`, `DeskWriteFile`, `AvailableEditors`, `OpenIn`, `Inspect` |

**Voice**

| # | Command or event | Go function it reuses |
|---|---|---|
| 41, 42 | `voicePress`, `voiceAudio {pcm}` or `{error}`, `voiceRelease`, `voiceCancel`, `voiceDismiss`, `voiceStartNow`, `voiceEdit`, `voiceChooseAgent`, `voiceChangeAgent`, `voiceToggleCloud`, `voiceChangeFolder`, `voiceChangeRepo`, `voiceRetry`, `voiceUnattach`; the `voice` event | `voice.NewWith` with a `voice.Remote` microphone the host feeds (16 kHz mono 16-bit PCM, base64) |
| 43 | `phonon` get / download / repair / cancel / remove and its event; `groqCheck` | `voice.NewPhonon`, `voice.CheckGroq` |

"Take a screenshot" said aloud stays in Go (`screen.Whole`, inside the backend). Done in C#, no
protocol: the microphone list and capture (winmm; the name is cut to 31 characters because
`Voice.Microphone` in `settings.json` stores it that way), the hold-to-talk hotkey (`RegisterHotKey` and
`GetAsyncKeyState` release), the chime, pickers, clipboard, notifications, the tray menu, single
instance and window placement.

Failure rule for the host: a command with no answer event of its own fails with a `toast`; a command
with one fails with `ok: false` and `error` in that event.

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
4. The file cannot be read now (locked, transient): **no key this run**. Do not replace it. Start the backend without a key (row 1 below), so no history is written that the next run cannot open.
5. No `note.key`: make 32 random bytes, wrap, write it, then use it. (On Windows there is no earlier key store to inherit.) If `agents\index.dat` already exists, this new key cannot open it, and rule 6 stops the backend before any file is changed.
6. Send the key once, base64, as the first command. The backend then refuses to start if `agents\index.dat` exists and does not open with it, and answers `backendFailure`. The host shows that text and does not touch any file.

The host never writes `settings.json`, `agents\*` or `secrets.dat`.

### The data tool

`cmd/hover-data` writes and reads a Go-style data folder by hand (`write`, `dump`, `check`, `secret set` and
`secret check`). `note.key` is bound to the Windows user, so a folder written on another machine or by another
user cannot be opened by the Windows app.

## D. How this is checked

Windows is GitHub's `windows-2022` runner. CI builds and keeps the app; it runs no tests.

- **`ci.yml`**, on a pull request, a push to `main`, or by hand (`gh workflow run ci.yml --ref <branch>`;
  it cancels an older run on the same ref). Its `windows` job runs `build.ps1 publish` (the C# app and the
  backend), the app's `--shots`, `build.ps1 installer` and `build.ps1 legacy installer` (the Go app still
  builds). `pictures` (by hand) draws the Go views and the C# `--shots`.
- **Local on Linux**: `dotnet build -p:EnableWindowsTargeting=true` of the libraries. The app compiles there up to
  `MakePri.exe`; the runner is the build that counts.
- **Pictures** are compared with the Go app's. The rule: fix visible breakage and clear mismatches, not
  small spacing differences.

### Memory

Measured on the runner (run 41330606525), as the Go app was: the notch at rest two
seconds after everything is up, and with the app window open. The Go app was one process; the C# app is the
app and its backend, so the total is the number to put beside the Go one.

| | Go app (`7d0a1c9`) | C# `hoverai.exe` | C# `hover-backend.exe` | C# total |
|---|---|---|---|---|
| At rest, private MB | 70.8 | 50.8 | 51.8 | 102.6 |
| With the app window, private MB | 483.4 | 118.8 | 51.8 | 170.6 |
| At rest, working set MB | 43.6 | 106.0 | 17.7 | 123.7 |
| With the app window, working set MB | 230.5 | 215.1 | 17.7 | 232.7 |
