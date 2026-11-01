# Windows in C#: what is where

The Windows app is a C# (WinUI 3) program over the Go backend, the same split macOS uses. This file
is the map: which C# project took over each Go package, which backend commands the Windows host
uses, the data formats both apps share, and how it is all checked. How to build and run it is
[windows.md](windows.md); what CI runs is [testing.md](testing.md); the wire format of the backend is
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
- `windows/Hover.Tests`, `Hover.Office.Tests`, `Hover.Settings.Tests` and `Hover.Voice.Tests` are the
  `dotnet test` projects; `windows/Hover.TestTools` holds the programs they start.
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
| 1 | `internal/core` | Data folder, `settings.json`, key, sealed history, `secrets.dat`, projects, themes, single instance | **Backend.** DPAPI key and the `%APPDATA%` path: `Hover.Host` (`HistoryKey.cs`, `DataFolder.cs`); single instance: `Hover.Host/SingleInstance.cs`; launch at login: `Hover.Settings/Windows/RunValue.cs`; dark-mode watch: `Hover/Settings/SettingsHost.cs`. |
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
| 12 | `internal/notch` | Notch size, placement, outline, openness animation, hover rules, hit region | `Hover.Notch` (`Geometry.cs`, `NotchController.cs`, `SvgPath.cs`). `Hover.Tests` compares it with numbers the Go code prints (`golden/notch.json`). |
| 13 | `internal/platform/win` | Win32 windows, DirectComposition, tray, `RegisterHotKey`, file pickers | `Hover/Notch` (`NotchWindow.cs`, `NotchSurface.cs`, `Native.cs`), `Hover/Settings/Dialogs.cs`, `Hover/Chat/Win32Clipboard.cs`. |
| 14 | `internal/audio` | Sound output (winmm waveOut) | `Hover.Voice/AudioOutput.cs`. |
| 15 | `internal/music` | Office beats (Ogg Vorbis loop, fade 0.32), chime | `Hover.Voice/Beats.cs` (NVorbis decodes `office-beats.ogg`, linked from `internal/music`); `Hover/Voice/BeatsHost.cs`. |
| 16 | `internal/screen` | Screen panel capture (`PrintWindow` per agent window over the wallpaper), voice screenshots | Panel: `Hover.Settings/Windows/ScreenCapture.cs`, `Hover/Screen/*`, `Hover.Chat/Ui/OfficeUi.DeskScreen.cs`. Voice screenshots stay in the backend. |
| 17 | `internal/ui` | The whole Gio interface | `Hover.Chat/Ui` (the panels over the office), `Hover.Settings/Ui` (Settings), `Hover/Chat` and `Hover/Settings` (the hosts that draw them in the notch and the app window). |
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
ignores what it does not know; new events are sent only in answer to new commands). Every row has a
Go test in `internal/backend` that sends the JSON line to a backend process (`host_test.go` starts the
test binary as the backend over real pipes with a stand-in agent), and the Mac smoke test
(`tests/macos/backend-smoke.py`) has additive asserts for them. The C# side is typed in
`Hover.Host/Protocol` and runs against the real `hover-backend` in `windows/Hover.Tests`. The messages
themselves, with their fields, are in [architecture.md](architecture.md#windows-host-commands).

**Start-up and lifecycle**

| # | Command or field | Go function it reuses | Go test |
|---|---|---|---|
| 1 | `initialize` with no `key` (or `null`): history off, the backend still runs | `core.UseNoHostKey` | `TestInitializeWithoutAKeyRunsWithTheHistoryOff` |
| 2 | start step: drop the old planner files | `core.DropPlanner` | `TestStartDropsTheOldPlannerFilesOnly` |
| 3 | start step: continue a busy Kiro turn | `SetRetryWhenBusy(settings.KiroRetryBusy)` | `TestABusyKiroTurnIsContinuedByTheLiveSetting` |
| 4 | `initialize` `credits: true`: Kiro credits by day | `quota.NewCredits`, `history.OnChanged(Poke)` | `TestCreditsByDay` |
| 5 | `initialize` `reattach: true`: follow Kiro Web tasks that were cut off | `KiroSessions.ReattachCutOff` | `TestACutOffKiroWebTaskIsFollowedOnWhenAsked` |
| 6 | Remove the old "Hover Service" scheduled task, once | host (C#): `Hover.Host/OldService.cs` | `Hover.Tests` |

**Settings**

| # | Command or field | Go function it reuses | Go test |
|---|---|---|---|
| 7 | `settings`: the whole model as JSON | `Settings.Model().ToJSON()` | `TestSettingsAnswersWithTheWholeModel` |
| 8 | `setSetting` `{key, value}` (ten keys, `Voice` and `ToolAgent` among them) | the `Settings.Set...` of each | `TestSetSettingChangesOneSettingAndKeepsTheFileFormat` |
| 9 | `projects` list / add / update / remove / default workspace | `AddProject`, `UpdateProject`, `RemoveProject`, `SetDefaultWorkspace` | `TestProjectsAddUpdateRemoveAndTheDefaultWorkspace` |
| 10, 11 | `themes`, `themeRead`, `palette` | `InstalledThemes`, `ReadTheme`, `PaletteFromTheme`, `HoverPalette` | `TestThemeFilesAreReadAndPaletteIsHovers` |
| 12 | `mcp` get / save / enable / remove (Kiro's `mcp.json`) | `LoadMcp`, `SaveMcp`, `SetMcpDisabled`, `RemoveMcp` | `TestKiroMcpListIsReadAndChangedInKirosFile` |
| 13 | `kiroAgents` | `agents.KiroAgents` | `TestKiroAgentsAreListedFromTheFolder` |
| 14 | `credits` | `quota.Credits.View` | `TestCreditsByDay` |
| 15 | `secret` has / set; the answer is never the value | `core.SystemSecrets()` | `TestSecretsAreSealedAndNeverSentBack` |
| 16 | Launch at login: the `Run` value `Hover`, written by the host (the backend's `os.Executable()` is `hover-backend.exe`) | host (C#): `Hover.Settings/Windows/RunValue.cs` | `Hover.Settings.Tests` |

**Notch, tray, office (fields on existing messages)**

| # | Field | Go function it reuses | Go test |
|---|---|---|---|
| 17, 18, 19 | `sessions[].status`, `sessions[].activity {verb, obj}`, `ended.body`, `ended.took` | `agents.Status`, `agents.Activity`, `TextFirstLine(Plain())` | `TestSessionsCarryStatusAndActivityAndEndedCarriesBodyAndTook` |

The office itself needs nothing more: `state.sessions[]` carries `bot`, `seat`, `stage`, `act`, `pose`,
`file`, `ctx`, `ask`, `turns[].steps` and `apps`, which is what `agents.Push` gives the Go office. The
camera, day/night and music choices are kept by the host in `office.json` (section C).

**Tasks and chat**

| # | Command or field | Go function it reuses | Go test |
|---|---|---|---|
| 20 | `new` fields `cloud`, `model`, `effort` (no `chips`: the new-task box has none) | `StartBound`, `setModel` | `TestNewTaskTakesModelEffortAndCloud` |
| 21 | `reply` field `chips` | `CheckChips`, `ReplyMsg` | `TestReplyTakesChipsAndChecksThem` |
| 22 to 25 | `pause`, `queueSendNow`, `queueRemove`, `queueResume` | `Pause`, `SendNowQueued`, `RemoveQueued`, `ResumeQueue` | `TestPauseAndTheQueue` |
| 26 | `rename` | `Rename` | `TestRenameKeepsTheNameWithTheChat` |
| 27, 28 | `switchProvider`, `fork`, `bringBack` | `SwitchProvider`, `Fork`, `BringFindingsBack` | `TestSwitchForkAndBringBack` |
| 29 | `turns[].uid/.chips/.switchTo`, `sessions[].cloud/.ext/.held/.commands` | the `KiroTurn` and `KiroSession` fields | `TestSessionsCarryTheChatsFields` |
| 30 to 32 | `cloudRepos`, `webSessions`, `adoptWeb`, `cliSessions`, `adoptCli` | `Runtime.Repos`, `CloudSessions`, `LocalSessions`, `AdoptCloud`, `AdoptLocal` | `TestKiroWebAndCliListingsAnswerAndEnd`, `TestAdoptTakesOnlyWhatWasListed` |
| 33 to 36 | `markdown`, `highlight`, `diffLines`, `flowchart` | `md.Parse`, `chat.Highlight`, `chat.Numbered`, `diagram.Layout` | `render_test.go` |
| 37 to 40 | `term`, `writeFile`, `editors`, `openIn`, `inspectFolder` | `NewTerm`, `DeskWriteFile`, `AvailableEditors`, `OpenIn`, `Inspect` | `deskmore_test.go` |

**Voice**

| # | Command or event | Go function it reuses | Go test |
|---|---|---|---|
| 41, 42 | `voicePress`, `voiceAudio {pcm}` or `{error}`, `voiceRelease`, `voiceCancel`, `voiceDismiss`, `voiceStartNow`, `voiceEdit`, `voiceChooseAgent`, `voiceChangeAgent`, `voiceToggleCloud`, `voiceChangeFolder`, `voiceChangeRepo`, `voiceRetry`, `voiceUnattach`; the `voice` event | `voice.NewWith` with a `voice.Remote` microphone the host feeds (16 kHz mono 16-bit PCM, base64) | `TestVoiceTakesTheHostsAudioAndStartsATask` |
| 43 | `phonon` get / download / repair / cancel / remove and its event; `groqCheck` | `voice.NewPhonon`, `voice.CheckGroq` | `TestPhononAndTheGroqCheck` |

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

### Fixture and the cross-read

`note.key` is bound to the Windows user, so a fixture written on Linux (where `note.key` is the
raw 32 bytes in a private file) cannot be opened by the Windows app. The Go-written fixture is made and
read on one Windows machine, in one run: `.\build.ps1 test` does it (the same steps run by hand):

```powershell
$fx = Join-Path $env:TEMP 'fixture-go'
New-Item -ItemType Directory -Force $fx, "$fx\project" | Out-Null
Set-Content -Path "$fx\answer.md" -Value 'Sample **answer**.'
$env:HOVER_DATA_DIR = $fx      # Go's log goes here, not to the %APPDATA% of whoever runs this
$env:CGO_ENABLED = '0'
go build -o "$fx\hover-data.exe" ./cmd/hover-data
& "$fx\hover-data.exe" write $fx "$fx\project" 2 "$fx\answer.md"   # prints the session key
& "$fx\hover-data.exe" dump  $fx                                   # expect: history: 1 sessions
& "$fx\hover-data.exe" check $fx                                   # exit 0: Go would save the same bytes
$env:HOVER_FIXTURE_GO = $fx    # the C# tests read this folder
```

What `write` makes: `settings.json`, `note.key` (DPAPI), `agents\index.dat` and one sealed session of two
turns ("A long rich conversation"). `hover-data secret set <dir> voice.groq` (the key on standard input)
adds a `secrets.dat`, and `secret check` opens one. `windows\Hover.Tests` (`GoCompatibilityTests`) does
two things in the same run:

1. **Go to C#.** `Hover.Host` reads `note.key` through DPAPI and starts `hover-backend` with that folder; the
   `state` lists the session key `write` printed. The test also opens `agents\<key>.dat` with its own
   sealing code (an independent check of the byte format), reads a secret the Go side kept with
   `hover-data secret set`, and checks that a run that is asked for nothing leaves `settings.json` as the
   same bytes.
2. **C# to Go.** In a second folder the host makes `note.key`, starts the backend, runs a task of the
   stand-in agent, and stops it; then `hover-data check` (exit 0) and `dump` (the session is listed), and
   `hover-data secret check` opens a `secrets.dat` the test sealed with its own code.

On Linux the same tests run when the key guard is the plain file (`note.key` is the raw key there): a
convenience, not a proof for DPAPI.

## D. How this is checked

Windows is GitHub's `windows-2022` runner. [testing.md](testing.md) has the detail.

- **`ci.yml`**, on a pull request, a push to `main`, or by hand (`gh workflow run ci.yml --ref <branch>`;
  it cancels an older run on the same ref). Its `windows` job runs `build.ps1 publish` (the C# app and the
  backend), `build.ps1 test -SkipGo` (every `windows\*.Tests` project and the Go-written fixture),
  `tools/app-smoke.ps1`, the app's `--shots`, `build.ps1 installer`, `tools/installer-check.ps1` (over the
  published 5.x Go setup while it runs, into a clean folder, into a folder with spaces twice, over 2.x; each
  uninstalls and leaves nothing) and `build.ps1 legacy installer` (the Go app still builds). Its `test` job
  runs the Go tests on three systems; `pictures` (by hand) draws the Go views and the C# `--shots`.
- **Local on Linux**: `dotnet build -p:EnableWindowsTargeting=true` of the libraries and the headless tests
  that need no Windows API. The app compiles there up to `MakePri.exe`; the runner is the build that counts.
- **Pictures** are compared with the Go app's. The rule: fix visible breakage and clear mismatches, not
  small spacing differences.

### Memory

Measured by `tools/app-smoke.ps1` on the runner (private bytes; the Go app was one process, the C# app is
the app and its backend), the notch at rest two seconds after everything is up, and with the app window
open:

| | Go app (`7d0a1c9`) | C# `hoverai.exe` | C# `hover-backend.exe` | C# total |
|---|---|---|---|---|
| At rest, private MB | 70.8 | 51.0 | 52.0 | 103.0 |
| With the app window, private MB | 483.4 | 120.2 | 52.0 | 172.2 |
