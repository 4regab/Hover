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
.\build.ps1 test             # the Go tests (CI does not run them)
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
text widths measured in Linux Chromium with DejaVu Sans; they skip on Windows.

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
