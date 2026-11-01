# Developing Hover on Windows

Windows 10 or 11, x86-64. All commands run from the repository root in PowerShell 7
(`pwsh`). Windows PowerShell 5.1 works for `build.ps1` too (CI runs `build.ps1 legacy` in it).

The Windows app is C#: a WinUI 3 program in `windows/` (`hoverai.exe`) that starts the same Go
backend the Mac app uses (`cmd/hover-backend`, as `hover-backend.exe`) and speaks JSON lines to it.
Settings, history, secrets, the agents and the voice flow are the backend's; the windows, the notch,
the tray, the shortcuts, the microphone, the sound, the Discord pipe and the Direct3D office are C#.
The Go app that was the Windows build before is kept for now and builds with `.\build.ps1 legacy`
(see [The legacy Go app](#the-legacy-go-app)). The map of what is where is
[windows-csharp.md](windows-csharp.md).

## Prerequisites

To build:

1. **The .NET 10 SDK** (`winget install Microsoft.DotNet.SDK.10`). No Visual Studio: the Windows
   App SDK, the Windows SDK build tools and the test packages come from NuGet.
2. **Go**, the version `go.mod` names (`winget install GoLang.Go`). An older Go fetches the
   named version the first time it builds. Go builds the backend; the Windows build is plain Go: no C
   compiler.
3. **Git.**

Only for the installer (`.\build.ps1 installer`): **Inno Setup 6 or 7**
(`winget install JRSoftware.InnoSetup`). You don't need it to build or run.

The agents themselves (Kiro, Codex, Cursor, OpenCode, Claude Code) are optional. Without them
the office still runs, and the tools show as not installed.

## Build and run

```powershell
.\build.ps1                  # publish\hoverai.exe and publish\hover-backend.exe
.\build.ps1 run              # build, then start it
.\build.ps1 publish          # the same, with LICENSE and the notices
.\build.ps1 installer        # dist\Hover-Setup-<version>.exe (needs Inno Setup)
.\build.ps1 test             # the Go tests, then every windows\*.Tests project (results in out\)
.\build.ps1 test -SkipGo     # only the C# tests
```

`publish\` is the whole program, self-contained: `dotnet publish windows\Hover` (Release, x64) plus
`go build ./cmd/hover-backend` beside it. It is emptied first, so nothing an earlier build left is
installed. The version is the number in `VERSION`; `-Version 5.0.3` overrides it, and
`publish\hoverai.exe --version` prints it. If Go or the .NET 10 SDK is missing, the script says so
before it builds anything.

`build.ps1` stops a running `hoverai.exe` (or an older `Hover.exe`) and `hover-backend.exe` before it
builds, so the files aren't locked.

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

### The installer

`packaging\windows\Hover.iss` packs everything in `publish\` (but the `.pdb` files) into
`Hover-Setup-<version>.exe`, a per-user install in `%LOCALAPPDATA%\Programs\Hover` (no administrator).
It keeps the AppId of the 2.x installer, so it installs over a 2.x or a 5.x (Go) install in place and
Windows lists one product:

- over 2.x it removes `Hover.exe` and moves a Run value that starts it to `hoverai.exe`; over the
  Go app it removes `wgpu_native.dll`, which only the Go office used;
- a running Hover (any build) and its backend are ended before the files are replaced or removed,
  at install and at uninstall; only one Hover runs per user, so one that runs is the one in the way;
- the uninstaller removes every file the setup put there, the Start Menu entry, the desktop
  icon if asked for, and the `Run` value `Hover` (the setup's startup task and Launch at login in
  Settings write the same value), when it starts this `hoverai.exe`. It never touches `%APPDATA%\Hover`.

`.\build.ps1 legacy installer` makes the Go app's setup from the same script (`/DLegacy`), named
`dist\Hover-Legacy-Setup-<version>.exe`.

## Check

CI builds the C# app, runs its tests, drives it (`tools/app-smoke.ps1`) and drives its installer
(`tools/installer-check.ps1`); [testing.md](testing.md) says what each does. Run them the same way
when you change the shell, the notch window or the installer:

```powershell
.\build.ps1 publish
.\tools\app-smoke.ps1 -Exe publish\hoverai.exe -Out $env:TEMP\hover-smoke      # needs a desktop you can leave alone
```

Render every view headless (no desktop session needed; drawn on the GPU or on WARP) as JPEGs no
wider than 1400 px:

```powershell
.\publish\hoverai.exe --shots $env:TEMP\hover-shots
```

## Logs and debugging

- The log is `hover.log` in the data folder (`%APPDATA%\Hover` by default, or
  `HOVER_DATA_DIR`). The app and the backend append to the same file: the app's start and
  quit, the backend's pid and exit code, the notch, the tray icon and the shortcut, the office's
  graphics device, and errors.
- The release build has no console (it is a `WinExe`). `hoverai.exe --version` prints the version
  when its output is piped. Every host log line is also written to standard error, so a console
  or a redirect shows them: `.\publish\hoverai.exe 2> $env:TEMP\hover.err`.
- `HOVER_UI_DUMP=<file>` makes the app write where its buttons are on the screen (see
  [The panels](#the-panels-chat-new-task-and-questions)); the checks of the real app read it.

## What is Windows-specific

- The notch and the app window are Win32 and WinUI windows made by `windows\Hover`; the notch is
  drawn with Direct2D and DirectWrite onto a Direct3D 11 swap chain that DirectComposition shows,
  for per-pixel transparency. The office is `windows\Hover.Office`, Direct3D 11 with HLSL.
- Tray and notifications, the shortcut (`RegisterHotKey`), the notch window's click-through and
  focus rules: `windows\Hover\Notch`.
- Agents are children of the backend, and the backend is in a Windows job object with
  `KILL_ON_JOB_CLOSE`, so the backend and everything it started die with the app, however the app ends.
- `note.key` is wrapped with DPAPI for the Windows user (`CurrentUser`, no entropy), as the Go app
  wrapped it; a folder can be read only where it was written.
- Codex's Read only mode isn't offered: Codex has no sandbox on Windows. The sandbox, computer use,
  the agent browser and its tab, one-click agent setup, Cua agent desktops and Hover's own helpers
  are off on Windows, as they were in the Go app (the sandbox runs on Linux and macOS, the rest on macOS only);
  their Settings rows say so.

## The C# app

The code is in `windows/`; the solution is `windows\Hover.Windows.slnx`.

| Project | What it is |
|---|---|
| `windows\Hover` | `hoverai.exe`: a code-first WinUI 3 app (no `.xaml` files), unpackaged and self-contained. The notch window, the tray, the app window, the hosts that tie the libraries below to the backend's messages, and `--shots`. |
| `windows\Hover.Host` | No WinUI. Starts `hover-backend.exe` in a job object, speaks JSON lines to it (`Protocol\`), keeps `note.key` (DPAPI), data folder paths, the single-instance names, the old-service removal, the log. |
| `windows\Hover.Notch` | No WinUI and no Windows call. The notch's rules: sizes, placement, outline, the open and close animation, the pointer rules, what the resting notch shows (the island), the shortcut and the tray menu. A port of `internal/notch` and `internal/app/rest.go`. |
| `windows\Hover.Office` | The Direct3D 11 office (Vortice): scene, bots, desks, camera, picking, the HLSL, the wall pictures. |
| `windows\Hover.Chat` | No WinUI. The panels over the office: chat thread, new-task box, menus, questions, the desk card and its tabs, the reply box. Drawn through an `ICanvas`; the app gives it Direct2D and DirectWrite. |
| `windows\Hover.Settings` | No WinUI. The Settings pages as data, their controller and painter, the Discord status client, the Run value and the Screen capture. |
| `windows\Hover.Voice` | No WinUI. Voice, sound and music: the hold-to-talk shortcut, the microphone and the sound output (winmm), the voice commands and the `voice` event, the card (its content, behaviour and Direct2D painter), the aura, the chime and the office beats. The winmm and user32 calls are made on Windows only; the rest runs and is tested anywhere. |
| `windows\Hover.TestTools` | Programs the tests start: stand-ins for `codex-acp` and `codex`, a backend that misbehaves on purpose, a host to be killed. |
| `windows\Hover.Tests`, `Hover.Office.Tests`, `Hover.Settings.Tests`, `Hover.Voice.Tests` | `dotnet test` (see [testing.md](testing.md#the-c-windows-app)). `build.ps1 test` runs every `windows\*.Tests` project. |

For the edit-and-run loop, `dotnet build` is enough (the output is runnable as it is, self-contained):

```powershell
$env:CGO_ENABLED = '0'
go build -o out\bin\hover-backend.exe ./cmd/hover-backend
dotnet build windows\Hover.Windows.slnx -c Release
Copy-Item out\bin\hover-backend.exe windows\Hover\bin\Release\net10.0-windows10.0.22621.0\win-x64\
$env:HOVER_DATA_DIR = "$env:TEMP\hover-dev"          # your own data folder, never the real one
windows\Hover\bin\Release\net10.0-windows10.0.22621.0\win-x64\hoverai.exe
```

`hoverai.exe` looks for `hover-backend.exe` in its own folder.

- `hoverai.exe` starts with the notch and the tray icon, no window. `--dashboard`
  opens the app window at start, `--settings` opens it on Settings. `--background` is still accepted (it is what a normal start does
  now). A second launch opens the dashboard of the copy that runs. Closing the dashboard hides it
  to the tray; Quit in the tray menu stops the backend (it writes the history and the settings
  first) and removes the icon. If the notch cannot be made (no graphics), the window opens and
  closing it quits.
- `hoverai.exe --version` prints `Hover <version>`; `hoverai.exe --shots DIR` draws the app's views into JPEG files (at most 1400 px wide) with
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

A click on a desk in the office (`DeskPicked`, with the place of the click) opens the desk card
(`OfficeUi.DeskCard.cs`) there, 6 to the right and 12 above the click and held inside the office, as
`internal/ui/officeview.go` places it; with no click it sits by the bot (`OfficeHost.BotScreenRect`), else at the
top right. The office takes the pointer only where the panels have nothing (`OfficeHost.Covered`): a press the
panels take is not also a click on the office behind them, and the office lets go of the pointer after it has read
the click, not before. Its tiles follow the backend's `desk` probe: Terminal, Files, Diff, Pull request, Linked, Agents. A tile opens the desk
panel (`OfficeUi.DeskPanel.cs`) with the tab's rows, which `Hover.Chat\Desk\DeskRows.cs` makes from the
backend's answers as `internal/app/desk.go` does. The card also holds the last steps, the permission
buttons (Allow, Trust, Deny), the question buttons, a line saying how many helpers the bot has out at its
desk (the office's tag, `OfficeHost.HelpersOf`) and a reply box. Over the office everything the backend
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
  model menu and sends `setModel`; Kiro's menu ends with its compact row ("Auto compact at 80%", from the
  backend's `preferences`), which opens Settings at Kiro's page, and the context ring in the chat's header has a tick
  at that percent; the other agents' menus end with a note. The cloud chip in the header shows for a Kiro Web task
  whose `kiroId` the backend has sent and opens its page on Kiro Web. A reply the backend refuses (it says so in a toast) comes back into the
  box with its words, chips and pictures.
- **The office's board, TV and history panels** (`OfficeUi.Lists.cs`) open over the office from the props
  (`OfficeHost.PropPicked`, `ShowPanel`). History lists the saved chats by day, finds and deletes them, and
  lists Kiro Web and Kiro CLI sessions with a click to adopt one (`webSessions`, `adoptWeb`, `cliSessions`,
  `adoptCli`).
- **Name tags, bubbles and tips** (`OfficeUi.Tags.cs`, `OfficeUi.Tip.cs`): each bot has its tag standing on the point
  the office gives (`OfficeHost.Tags`): the tool's mark and the bot's name, a bubble with what it says, and a
  click on the tag opens its chat. A question stands on the tag in place of the bubble, and not at all while that
  desk's card is open (the card shows it). Beside the pointer, a tip says "Chat with Pip" over a bot,
  "Pip's desk" with what the desk holds over a desk, and a prop's own hint ("Session board"). The bubble's tail
  and Go's pixel face for names and hints are not drawn.
- **Not on Windows, as in Go:** Hover's own helpers (the Helpers switch and the agents' helper rows;
  `agents.OrchMcpSupported()` is false on Windows, `internal/agents/orch.go`; the menu item and the new-task
  helpers are gated in `internal/shell/chatact.go` and `newtask.go`) and the Agent browser tab (`agents.BrowserSupported()` is macOS only). The Screen tab is
  the Screen panel, described with Settings below.

`hoverai.exe --shots DIR` also writes the desk's pictures with the Go pictures' names (`desk-*`, `expand-*`,
`office-panel-*`, and the reply box's `chat-*`), drawn by `Chat\DeskShots.cs` over the real office: the sessions
sit at their desks (`OfficeHost.ShowForShot`), so the card, the tags and the helpers are where the office puts them.
The pictures take none of the backend's answers about their made-up sessions (`UiCore.FreezeState`).

### Settings, the menus, themes, Launch at login, Discord and the Screen panel

Settings is the Go app's `internal/app/pages.go` as data, drawn by the same toolkit as the chat (`Hover.Chat`'s
`ICanvas`, `Pen` and `EditBox`), so the open notch and the app window show the same page from the same code. The library
is `windows\Hover.Settings`; the app's side is `windows\Hover\Settings`.

| What | Where | Notes |
|---|---|---|
| The pages | `Hover.Settings\Pages\*` | `PageBuilder` makes the blocks (groups, switches, segments, pickers, fields, sliders, tiles, links) of each page with the Go words. Windows rules as in Go: Sandbox, Computer use, Agent browser, Agent desktops and the one-click Setup are shown off with the Go notes; there are no Cua rows. Cua, Setup and the `integ.*` actions are Mac only. |
| The controller | `Hover.Settings\Controller\*` | Every control is a typed backend command (`setSetting`, `projects`, `themes`, `themeRead`, `palette`, `mcp`, `kiroAgents`, `secret`, `credits`, `phonon`, `groqCheck`; `Hover.Host\Protocol\SettingsMore.cs`). settings.json is written by the backend alone. A change shows at once and is held until the backend's `settings` has it (3 s at most), so two changes within 400 ms are both kept. |
| The painter | `Hover.Settings\Ui\*` | Draws a page, its menus and its fields on any `ICanvas`; the notch draws it with Direct2D into its window and the app window draws the same page onto a bitmap that fills the place of the office (`SettingsOverlay`). Keys, the wheel, text fields and the shortcut recorder go through `SettingsUi`. |
| In the app | `Hover\Settings\SettingsHost.cs`, `NotchHost.Settings.cs`, `SettingsOverlay.cs` | The open notch has a Settings row in the ≡ menu; the app window has Settings in its menu bar and the tray's Settings… opens it (`hoverai.exe --settings` does too). The voice card's Settings button opens Settings → Voice. While the shortcut records, the notch's and the voice hotkey are let go (`VoiceHost.SuspendKey`). |
| The menu bar | `Hover\Settings\AppMenu.cs` | File / Settings / Help as in `internal/ui/barmenu.go`, the system's own `MenuBar` in the app window's header. |
| Projects | `Hover.Settings\Controller\SettingsActions.cs` | Add (the folder picker), edit a project's name, folder and access, remove the entry only (the folder and its sessions stay), and the default workspace folder and access; a folder that is gone is said on its row and the page still opens. |
| Themes | `Hover.Settings\Pages\Palettes.cs` | The backend lists the installed VS Code, Cursor, Kiro and Windsurf themes and reads a theme file (comments and trailing commas allowed); the palette comes from the backend (`palette`) as Go applies it. Hover's own light and dark are constants. A file with no theme colours says "That file has no VS Code theme colours in it." and changes nothing. Appearance → System follows the system's light or dark, looked at every 2 s. |
| Launch at login | `Hover.Settings\Windows\RunValue.cs` | The C# app writes the value `Hover` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`: the path of `hoverai.exe` in quotes, as the Go app writes it. Off removes it. The backend is not asked. |
| Voice page | `Hover.Settings\Pages\VoicePage.cs` | The shortcut recorder, the microphones (`MicDevices.Names()`), hold or toggle, the Groq key and the cleanup key (sent to `secret` and never shown or logged; the page only knows that one is kept), Phonon install, repair and remove with progress, and Try it (`VoiceHost.PressTrial()`). |
| The Discord status | `Hover.Settings\Discord\DiscordPresence.cs`, `Hover\Discord\DiscordHost.cs` | The C# app talks to Discord on the named pipe `discord-ipc-0` to `9` with the Go rules (a change at most every 5 s, a look every 30 s, "Idle" or "N agents working", the tools' names up to 128 characters). `BackendSession` sends `initialize {hostDiscord: true}`, so the backend starts no client of its own and the status shows once. Not running Discord is not an error: it is tried again at the next look. |
| The Screen tab | `Hover.Settings\Windows\ScreenCapture.cs`, `Hover\Screen\*`, `Hover.Chat\Ui\OfficeUi.DeskScreen.cs` | The desk's Screen tab shows a frame of the main display with only the windows of the processes in `state.sessions[].apps` over the wallpaper (`Cover`ed to the screen, as Go does): each visible window gets its own picture with `PrintWindow` (`PW_RENDERFULLCONTENT`), so what is over it does not show. A minimised or closed window is left out; the frame fits 1280 x 800. At rest it is the desktop once; Watch live, or the agent testing with computer use (`state.sessions[].testing`), takes a picture about four times a second. `ScreenFeed` (app) makes each picture off the UI thread and keeps it in the panels' picture store as `hover://screen/N` (`ImageStore.Put`, `Forget`), and the tab lets the one it replaced go. The line under the picture is Go's. |

`hoverai.exe --shots DIR` writes the Settings pictures with the Go pictures' names (`settings-*` and `notch-open-settings*`)
as JPEG. The Go pictures have a 36 px menu bar strip at the top that these do not. The checks of the real app read the
page with `HOVER_UI_DUMP` (`settings-window` for the app window, `settings-notch` for the open notch): the sections, the
focus and the place of every control.

The data folder is `hover-data`'s to check: `hover-data check <dir>` after a run of the C# app must exit 0, and
`hover-data secret check <dir> voice.groq` must find the key a Voice page saved.

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

The Settings → Voice page is in [Settings](#settings-the-menus-themes-launch-at-login-discord-and-the-screen-panel) above. Screenshots said aloud
are taken by the backend. During dictation the reply box's bar says "Listening…" and then "Writing it down…" at its right
(`OfficeUi.DictationLine`, set from `VoiceService.Changed`); a dictation error is said there for five seconds, not as a balloon.
The screenshots said for go with the reply are read from the data folder's `kiro-images` (`DictationShots`; no other path is read)
and put on the reply as pasted pictures are, four at most.

The key rules for `note.key` (set aside, never overwritten) are in
[windows-csharp.md](windows-csharp.md#c-data-formats); `Hover.Host/HistoryKey.cs` follows them.

### Building on Linux, and two traps

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

## The legacy Go app

Until the owner switches over, the Go Windows app still builds and its setup is made from the same
installer script. Nothing else uses it: CI builds it once to see that it does.

```powershell
.\build.ps1 legacy              # publish-legacy\hoverai.exe and wgpu_native.dll
.\build.ps1 legacy run          # build, then start it
.\build.ps1 legacy publish      # the same, with LICENSE and the notices
.\build.ps1 legacy installer    # dist\Hover-Legacy-Setup-<version>.exe
.\build.ps1 legacy test         # the Go tests (CI runs them on three systems)
```

It needs only Go (no .NET). The Go app draws with Gio (Direct3D 11) and the office with wgpu-native on
DX12, read back and shown as an image; `build.ps1 legacy` downloads `wgpu_native.dll` the first time and
keeps it in `publish-legacy\`. Its notch and app window are Win32 windows (`internal/platform/win`), and the
release build is `-H=windowsgui`; to see a panic on stderr, run it as a console program:
`$env:CGO_ENABLED = '0'; go run ./cmd/hover`. The Go tests (`.\build.ps1 legacy test`) need no setup. Two
layout tests in `internal/chat` compare text widths measured in Linux Chromium with DejaVu Sans; the fonts are
in the repo, so the tests run on Windows too.
