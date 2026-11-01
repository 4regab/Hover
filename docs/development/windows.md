# Developing Hover on Windows

Windows 10 or 11, x86-64. All commands run from the repository root in PowerShell 7
(`pwsh`). Windows PowerShell 5.1 works for `build.ps1` too.

## Prerequisites

To build:

1. **Go**, the version `go.mod` names (`winget install GoLang.Go`). An older Go fetches the
   named version the first time it builds. Hover's Windows build is plain Go: no C compiler,
   no Visual Studio.
2. **Git.**

Only for the installer (`.\build.ps1 installer`): **Inno Setup 6 or 7**
(`winget install JRSoftware.InnoSetup`). You don't need it to build or run.

The agents themselves (Kiro, Codex, Cursor, OpenCode, Claude Code) are optional. Without them
the office still runs, and the tools show as not installed.

## Build and run

```powershell
.\build.ps1                  # publish\hoverai.exe and publish\wgpu_native.dll
.\build.ps1 run              # build, then start it
.\build.ps1 publish          # the same, with LICENSE and the notices
.\build.ps1 installer        # dist\Hover-Setup-<version>.exe (needs Inno Setup)
.\build.ps1 test             # the Go tests (CI runs the same)
```

The version is the number in `VERSION`; `-Version 5.0.3` overrides it.

The office is drawn with wgpu-native. `build.ps1` downloads `wgpu_native.dll` the first time
and keeps it in `publish\`; the exe looks for it beside itself.

`build.ps1` stops a running `hoverai.exe` (or an older `Hover.exe`) before it builds, so the file isn't locked.

The exe is `hoverai.exe`, not 2.x's `Hover.exe`: Discord's game list matches any path
ending in `hover/hover.exe` (Hover: Revolt of Gamers), and the install folder is
`Programs\Hover`. The installer deletes the old exe and moves a Run value that points at it.

Only one Hover runs at a time (the named mutex `Local\HoverRunningInstance`). A second
launch opens the first one's window and exits. Quit an installed Hover from its tray
menu before you run your build.

Use a data folder of your own, so your build never touches your real settings and
history:

```powershell
$env:HOVER_DATA_DIR = "$env:TEMP\hover-dev"
.\publish\hoverai.exe
```

## Check

CI builds and drives the Windows app (`tools/app-smoke.ps1`) and its installer
(`tools/installer-check.ps1`); [testing.md](testing.md) says what each does. Run them the
same way when you change the shell, the notch window or the installer.

Render every view headless (no desktop needed; the office pictures need
`HOVER_SHOTS_OFFICE=1`, and draw on the GPU or on WARP):

```powershell
.\publish\hoverai.exe --shots $env:TEMP\hover-shots
```

The Go tests (`.\build.ps1 test`) need no setup. Two layout tests in `internal/chat` compare
text widths measured in Linux Chromium with DejaVu Sans; the fonts are in the repo, so the
tests run on Windows too.

## Logs and debugging

- The log is `hover.log` in the data folder (`%APPDATA%\Hover` by default, or
  `HOVER_DATA_DIR`). Tool starts, stops, permission questions, GPU choice and errors
  go there. `HOVER_TRACE=1` adds what the shell is doing.
- The release build has no console (it is built with `-H=windowsgui`). To see a panic on
  stderr, run it as a console program: `$env:CGO_ENABLED = '0'; go run ./cmd/hover`.

## What is Windows-specific

- Windows: the notch and the app window are Hover's own Win32 windows (`internal/platform/win`),
  presented through DirectComposition for per-pixel transparency. Gio draws the interface into
  them (Direct3D 11). The office is drawn by wgpu-native on DX12, read back and shown as an
  image, on its own thread.
- Tray and notifications, the shortcut (`RegisterHotKey`), the notch window's click-through and
  focus rules: `internal/platform/win` and `internal/shell/env_windows.go`.
- Codex's Read only mode isn't offered: Codex has no sandbox on Windows.

## The C# app (branch `windows-csharp`)

The Windows app is being rewritten in C# with WinUI 3, on top of the same Go backend the
Mac app uses. The plan is [windows-csharp.md](windows-csharp.md). The Go app above stays the
Windows build until the owner switches over (`build.ps1 legacy`). The C# code is in
`windows/`; the solution is `windows\Hover.Windows.slnx`.

| Project | What it is |
|---|---|
| `windows\Hover` | `hoverai.exe`: a code-first WinUI 3 app (no `.xaml` files), unpackaged and self-contained. |
| `windows\Hover.Notch` | No WinUI and no Windows call. The notch's rules: sizes, placement, outline, the open and close animation, the pointer rules, what the resting notch shows (the island), the shortcut and the tray menu. A port of `internal/notch` and `internal/app/rest.go`. |
| `windows\Hover.Voice` | No WinUI. Voice, sound and music: the hold-to-talk shortcut, the microphone and the sound output (winmm), the voice commands and the `voice` event, the card (its content, behaviour and Direct2D painter), the aura, the chime and the office beats. The winmm and user32 calls are made on Windows only; the rest runs and is tested anywhere. |
| `windows\Hover.Host` | No WinUI. Starts `hover-backend.exe` in a job object, speaks JSON lines to it, keeps `note.key` (DPAPI), data folder paths, the single-instance names, the old-service removal. |
| `windows\Hover.Office` | The Direct3D 11 office. Empty until its phase. |
| `windows\Hover.TestTools` | Programs the tests start: stand-ins for `codex-acp` and `codex`, a backend that misbehaves on purpose, a host to be killed. |
| `windows\Hover.Tests` | `dotnet test`. |
| `windows\Hover.Voice.Tests` | `dotnet test` for voice, sound and music (see [testing.md](testing.md#the-c-windows-app)). |
| `windows\tools\gofixture` | A small Go program for the tests: writes and reads a secret with the Go code. |

Prerequisites: the .NET 10 SDK (`winget install Microsoft.DotNet.SDK.10`) and Go as above. No
Visual Studio. The Windows App SDK, Windows SDK build tools and test packages come from NuGet.

```powershell
$env:CGO_ENABLED = '0'
go build -o out\bin\hover-backend.exe ./cmd/hover-backend
dotnet build windows\Hover.Windows.slnx -c Release
Copy-Item out\bin\hover-backend.exe windows\Hover\bin\Release\net10.0-windows10.0.22621.0\win-x64\
$env:HOVER_DATA_DIR = "$env:TEMP\hover-dev"          # your own data folder, never the real one
windows\Hover\bin\Release\net10.0-windows10.0.22621.0\win-x64\hoverai.exe
```

`hoverai.exe` looks for `hover-backend.exe` in its own folder. The `dotnet build` output is
runnable as it is (self-contained). Packaging it is a later phase.

- `hoverai.exe` starts as the Go app does: the notch and the tray icon, no window. `--dashboard`
  opens the app window at start. `--background` is still accepted (it is what a normal start does
  now). A second launch opens the dashboard of the copy that runs. Closing the dashboard hides it
  to the tray; Quit in the tray menu stops the backend (it writes the history and the settings
  first) and removes the icon. If the notch cannot be made (no graphics), the window opens and
  closing it quits.
- `hoverai.exe --shots DIR` draws the app's views into JPEG files (at most 1400 px wide) with
  `RenderTargetBitmap`, from inside the app, and exits. No screen capture. It does not take the
  single-instance claim.
- Single instance: the named mutex `Local\HoverRunningInstance`, and the event
  `Local\HoverShowApp` that a second launch sets. These are the Go app's names, so a Go copy
  and a C# copy keep out of each other's way.
- The log is `hover.log` in the data folder; the C# host and the Go backend append to the same file.

### The notch

The notch is a Win32 window of its own (`windows\Hover\Notch\NotchWindow.cs`), not a WinUI
window: borderless, `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, at the top centre of the
main display's work area. It never takes the keyboard unless it is opened with the shortcut or a
question's card is opened, and it gives the keyboard back when it folds. Its pixels come from a
swap chain made for composition with premultiplied alpha, shown by a DirectComposition visual
(`NotchSurface.cs`), so what is not drawn is see-through. Direct2D and DirectWrite draw the
frame (`NotchRenderer.cs`); the text is set in the Inter files in `Assets\Fonts`. The window is
click-through (`WS_EX_TRANSPARENT`) except over the shape and its shadow; the pointer is polled
every 50 ms, as the Go app does.

The rules are in `windows\Hover.Notch` and use the Go numbers. `windows\Hover.Tests\golden\notch.json`
is written by `internal/app/notch_golden_test.go` from the Go code; the C# tests feed the same
inputs to the C# code and compare. Change either side and a test fails until the file is written
again:

```powershell
$env:HOVER_UPDATE_GOLDEN = '1'; go test ./internal/app -run TestNotchGolden
```

What the resting notch shows comes from the backend's `state` (the sessions' `status`,
`activity`, `ask`), `ended` (`body`, `took`), `quotas` and `settings` messages. Alt+N (or the
`ScWorkspace` setting) is `RegisterHotKey` on the notch window; a refusal is logged with
Windows' error code (1409: already taken) and said once in a tray balloon. The tray menu is the Go
app's: Open Agent Office, Open App Window, Launch at Login (the `Run` value `Hover`, as the Go app
writes it), Settings…, Quit Hover.

**Where the office goes.** The open notch is the shape at its full size, `Open.W` by `Open.H`
DIPs (1120 x 440 for Office size Default, less the 24 DIPs kept free on a small screen), at
`x = Pad (40)`, `y = 0` inside the notch window (`NotchScene.OfficeArea`). `NotchSurface.Composition`
gives a later view the DirectComposition device and root visual to add its own swap chain at that
rectangle. Until then the area is a dashed frame with its size written in it. The app window has the
same place: `DashboardWindow.OfficeArea`, a bordered area under the header.

### The panels: chat, new task and questions

The office's panels (`internal/ui` in the Go app) are `Hover.Chat.Ui.OfficeUi`: the switch between the
office and the chat view, the session list, the chat (header, thread, reply box), the new-task box and
its menus (agent, where it runs, folder, access, model), the question over a bot and at the end of a
chat, the rewind confirmation and the toast. `OfficeUi` knows nothing of Windows: it follows the
backend's messages (`Accept`), draws a frame on an `ICanvas` (`Paint`), reads the pointer and the keys
against the places the last frame drew (`PointerDown`, `Key`, `Char`) and sends the backend commands the
Go shell sends (`new`, `reply`, `answer`, `pause`, `queueSendNow`, `queueRemove`, `queueResume`, `rename`,
`switchProvider`, `fork`, `bringBack`, `restore`, `again`, `delete`). Because it is plain C# the tests
run all of it on any system with a fixed-width shaper and a canvas that writes down what was drawn.

The chat thread (`Hover.Chat`: Markdown, code colour, numbered changes, tables, flowcharts, pictures,
selection and copy) is laid out by `ChatThread` and painted by `ThreadPainter`. Markdown, colour,
changes and flowcharts are read by the backend (`markdown`, `highlight`, `diffLines`, `flowchart`); the
thread shows plain text for an answer until the backend's reply arrives. The chat is drawn natively with
DirectWrite and Direct2D, not in a WebView2: DirectWrite sets the paragraphs (`DwShaper`, with Inter
beside the app and Consolas for code), `D2DCanvas` draws shapes, text, the flowcharts (`SvgScene` reads
the SVG the backend writes and the icons), the tools' marks and pictures (`ImageStore` reads a file the
session serves or the web, decodes it with the system's imaging and shows the broken-image mark when it
cannot). Selection runs over every text box in document order, so it crosses paragraphs, lists, tables
and code; copying writes the text the page would copy.

Two hosts draw the same `OfficeUi` (one shows at a time, so a chat opened in one is open in the other):

- **The open notch** (`Notch\NotchHost.cs`, `Chat\UiLayer.cs`): a second composition swap chain, its own
  visual kept above the other visuals of the notch window (the 3D office is one), so the panels draw over
  the office. Pointer, wheel, keys and typed text (`WM_CHAR`) go to the panels first; what they do not use
  goes to the office. While one of their boxes has the keyboard the notch takes it and gives it back.
- **The app window** (`Chat\UiOverlay.cs`): the panels drawn onto a bitmap shown over the office area. The
  overlay takes the pointer only where the panels have something (it is hit-testable or not as the
  pointer moves), so the office behind it still gets the rest.

A question reaches the user in three places, all answering with `answer`: the notch's island and card
(`Allow`, `Trust`, `Deny`, Esc, Enter, Shift+Enter), over the bot in the office (`OfficeHost.BotScreenRect`;
the cards stack at the top when the office does not show the bot), and at the end of the chat. A question
with choices shows them as buttons that toggle, a box for one's own answer where the agent takes one,
and Skip and Answer.

For the checks of the real app, `HOVER_UI_DUMP=<file>` makes the app write where each place that
answers a click is on the screen, the words of the thread and the lines the panels drew
(`Chat\UiDump.cs`), and, in a `voice` part, whether the voice card shows, what it says and where its
buttons are; a test reads it to click and type as a person would. The `window` part also says where the
bots are (`bots`), which desk card and panel are open (`deskCard`, `deskPanel`, `deskTab`, `deskRows`),
which board, TV or history panel shows (`officePanel`), how many pictures and chips the reply holds
(`shots`, `chips`), how many rows the `@` or `/` list offers (`popRows`) and what the GitHub CLI card
says (`ghTitle`, `ghBusy`).

### The desk, the reply box and the office's panels

A click on a desk in the office (`DeskPicked`) opens the desk card (`OfficeUi.DeskCard.cs`) beside the bot,
where the office says the bot is (`OfficeHost.BotScreenRect`), else at the top right. Its tiles follow
the backend's `desk` probe: Terminal, Files, Diff, Pull request, Linked, Agents. A tile opens the desk
panel (`OfficeUi.DeskPanel.cs`) with the tab's rows, which `Hover.Chat\Desk\DeskRows.cs` makes from the
backend's answers as `internal/app/desk.go` does. The card also holds the last steps, the permission
buttons (Allow, Trust, Deny), the question buttons and a reply box. Over the office everything the backend
sends comes through `desk`, `term`, `writeFile`, `editors`, `openIn`, `deskAction` and `gh` (`Hover.Host\Protocol\Desk.cs`).

- **Terminal:** PowerShell in the session's folder (the backend runs it): run, interrupt, clear. The output
  keeps its last lines in view and a command that never ends can be interrupted.
- **Files:** the folder's list and changes, a file opened with its line numbers, edited and saved with
  `writeFile` (the backend refuses a path outside the folder and the editor stays open with the reason),
  opened in an editor with `openIn`. A binary file and a file that went while open say so and cannot be
  edited. A Markdown file and a pull request's text are shown formatted by a second `ChatThread`
  (`ChatThread.SetDocument`).
- **Diff:** both sides numbered, a binary file folded, cut at 4,000 rows as the Go diff is.
- **Pull request:** the form (title, body, branch, base; commit or draft), the open pull request with its
  checks, and the GitHub CLI setup: install (winget), sign in with GitHub's code, Cancel.
- **Reply box** (`OfficeUi.Composer.cs`, `OfficeUi.Reply.cs`): the stop square is Pause, as in Go (Go has no
  Stop command; `internal/shell/chatact.go` `sendReply`). Up to four pictures of at most 8 MiB each from
  Ctrl+V of a clipboard bitmap or the attach button; `@` lists the folder's files and a pick is a chip sent
  with the reply (`ReplyCommand.Chips`); `/` lists the agent's commands and Hover's; the model pill opens the
  model menu and sends `setModel`. A reply the backend refuses (it says so in a toast) comes back into the
  box with its words, chips and pictures.
- **The office's board, TV and history panels** (`OfficeUi.Lists.cs`) open over the office from the props
  (`OfficeHost.PropPicked`, `ShowPanel`). History lists the saved chats by day, finds and deletes them, and
  lists Kiro Web and Kiro CLI sessions with a click to adopt one (`webSessions`, `adoptWeb`, `cliSessions`,
  `adoptCli`).
- **Not on Windows, as in Go:** the helpers (`agents.OrchMcpSupported()` is false on Windows,
  `internal/agents/orch.go`; the menu item and the new-task helpers are gated in `internal/shell/chatact.go`
  and `newtask.go`) and the Agent browser tab (`agents.BrowserSupported()` is macOS only). The Screen tab is
  the Screen panel of Phase 5.

`hoverai.exe --shots DIR` also writes the desk's pictures with the Go pictures' names (`desk-*`, `expand-*`,
`office-panel-*`, and the reply box's `chat-*`), drawn by `Chat\DeskShots.cs`.

### Voice, sound and music

Voice is the backend's flow (Groq or Phonon for the words, cleanup, the choice of folder and agent,
the preview, starting the task) with the parts that belong to Windows done in C#. The protocol is
in [architecture.md](architecture.md#windows-host-commands). The code is `windows\Hover.Voice` and,
for the notch's side, `windows\Hover\Voice`.

| What | Where | Notes |
|---|---|---|
| The shortcut | `Hover.Voice\VoiceHotkey.cs` | `RegisterHotKey` with id 2 on a message-only window of its own (`MOD_NOREPEAT`); letting go is found by asking `GetAsyncKeyState` about the key and each modifier every 30 ms. Ctrl+Alt+Space unless `Voice.Shortcut` says otherwise, and only while `Voice.Enabled`. A chord Windows refuses is said on the card in the Go app's words (error 1409: "already in use by another app, by Windows or by Hover's own shortcut"). |
| Hold or toggle | `Hover.Voice\VoiceService.cs` | `Voice.HoldToTalk`: hold the chord and let go to finish; off (the default) a second press ends the recording. A press while an interaction is in progress is turned away and the card glows amber for 0.6 s. Dictation (an open reply box in a chat in view) writes the words into the box and shows no card. |
| The microphone | `Hover.Voice\Microphone.cs` | winmm `waveIn`, 16 kHz mono 16-bit, four buffers of 50 ms. The device is the one named in `Voice.Microphone`; winmm cuts names at 31 characters, so a longer stored name is matched by its start (the Go rule). A missing or busy device is sent as `voiceAudio {error}` with the Go app's words: "No microphone is connected.", "The microphone “X” isn’t connected. Pick another in Settings → Voice.", "Another app is using the microphone." |
| The audio | `Hover.Voice\VoiceProtocol.cs` | Each 50 ms buffer is a `voiceAudio {pcm}` line (base64 of 16-bit little-endian samples); a block over 128 KiB is cut into several. The last partial buffer is kept at the release. |
| The card | `Hover.Voice\Card\*`, `Hover\Voice\*` | `CardModel` makes the card from the `voice` event; `CardController` has the menus (agent, model and effort, folder, Kiro Web repository), the picks and the keys; `CardPainter` draws it with Direct2D and DirectWrite in the notch's style; `Aura` is the orb on the listening and working cards. The notch shows the card in place of the island, takes the keyboard for Esc, Enter and the task when an interaction begins, and gives it back when the card has nothing to type into. |
| Sound output | `Hover.Voice\AudioOutput.cs` | winmm `waveOut`, 16-bit stereo 44.1 kHz, four buffers of 100 ms. With no output device the open fails with "There is no sound output." and nothing else happens. |
| The chime | `Hover.Voice\Beats.cs` (`Chime`) | Two quiet notes, 880 and 1320 Hz, 90 ms each; played when voice takes a screenshot. |
| The beats | `Hover.Voice\Beats.cs`, `Hover\Voice\BeatsHost.cs` | `internal\music\office-beats.ogg` is linked into `Hover.Voice` (not copied) and decoded with NVorbis as it plays, started again at its end. The volume ramps 0.02 up and 0.03 down a frame to 0.32 (the Go `RampStep`), and the device is let go when it reaches 0. The switch is `OfficeHost.Beats` (`office.json`'s `beats`); the loop plays while it is on and the open notch or a shown window is in view. If no sound output can be opened the switch goes back off. |

`hoverai.exe --shots DIR` writes the card's 82 pictures with the Go pictures' names as JPEG
(`voice-default-preview-2x.jpg` and so on). A picture is the notch's whole window, so one wider than
1400 pixels is cut to the middle 1400. The Go 2x pictures draw the aura at half size (the Go shots
draw its 66 pixels in a 2x window); the C# pictures draw it at the size the app has.

What is not done: the Settings → Voice page itself (shortcut, microphone, Phonon, Groq key, Try it)
belongs to the Settings pages; the commands it needs (`setSetting` `Voice`, `phonon`, `groqCheck`,
`secret`, `VoiceService.PressTrial`) are in place. Screenshots said aloud are taken by the backend; the
dictation line ("Listening…", "Writing it down…") under the reply box is not drawn.

The key rules for `note.key` (set aside, never overwritten) are in
[windows-csharp.md](windows-csharp.md#c-data-formats); `Hover.Host/HistoryKey.cs` follows them.

On Linux, `dotnet build` of `Hover.Host`, `Hover.TestTools` and `Hover.Tests` works, and
`dotnet test` runs everything in the tests that is not marked Windows only. For `windows/Hover`
there is a compile check that needs no Windows:

```sh
dotnet build windows/Hover/Hover.csproj -p:EnableWindowsTargeting=true \
  -p:WindowsAppSDKSelfContained=false -p:SelfContained=false
```

It compiles the C# and then stops with `MSB6003 ... MakePri.exe could not be run`: the Windows
App SDK build runs `mt.exe` and `MakePri.exe`, which are Windows programs, so a full build, and
running the app, need Windows. The runner is the build that counts.

Two things learned the hard way about a code-first WinUI 3 app: `Application.Resources` cannot
be read in the `App` constructor (it fails with E_UNEXPECTED), so the control styles
(`XamlControlsResources`) are merged in `OnLaunched`; and an exception that leaves the
`Application.Start` callback ends the process with only the exit code `-1073741189`, so the
callback catches and logs it.
