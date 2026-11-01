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

- `hoverai.exe --background` starts without a window. A second launch then opens the dashboard,
  as it will for a Hover started at login.
- `hoverai.exe --shots DIR` draws the app's views into JPEG files (at most 1400 px wide) with
  `RenderTargetBitmap`, from inside the app, and exits. No screen capture. It does not take the
  single-instance claim.
- Single instance: the named mutex `Local\HoverRunningInstance`, and the event
  `Local\HoverShowApp` that a second launch sets. These are the Go app's names, so a Go copy
  and a C# copy keep out of each other's way.
- The log is `hover.log` in the data folder; the C# host and the Go backend append to the same file.

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
