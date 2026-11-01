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
| `windows\Hover.Host` | No WinUI. Starts `hover-backend.exe` in a job object, speaks JSON lines to it, keeps `note.key` (DPAPI), data folder paths, the single-instance names, the old-service removal. |
| `windows\Hover.Office` | The Direct3D 11 office. Empty until its phase. |
| `windows\Hover.TestTools` | Programs the tests start: stand-ins for `codex-acp` and `codex`, a backend that misbehaves on purpose, a host to be killed. |
| `windows\Hover.Tests` | `dotnet test`. |
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
(`Chat\UiDump.cs`); a test reads it to click and type as a person would.

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
