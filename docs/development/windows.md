# Developing Hover on Windows

Windows 10 or 11, x86-64. All commands run from the repository root in PowerShell 7
(`pwsh`). Windows PowerShell 5.1 works for `build.ps1` too.

## Prerequisites

To build and test:

1. **Visual Studio Build Tools** with the "Desktop development with C++" workload
   (MSVC and the Windows SDK). Rust links with them.

   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

2. **Rust** through rustup. `rust-toolchain.toml` in the repository picks the version
   (the one CI uses); rustup installs it the first time you build.

   ```powershell
   winget install Rustlang.Rustup
   ```

3. **Git.**

Only for the installer (`.\build.ps1 installer`): **Inno Setup 6 or 7**
(`winget install JRSoftware.InnoSetup`). You don't need it to build, run or test.

The agents themselves (Kiro, Codex, Cursor, OpenCode) are optional. Without them the
office still runs, and the tools show as not installed. The tests use a stand-in
(`fake-agent`) instead.

## Build and run

```powershell
.\build.ps1                  # debug build: native\target\debug\hover.exe
.\build.ps1 release          # release build: native\target\release\hover.exe
.\build.ps1 release run      # build, then start it
.\build.ps1 publish          # publish\Hover.exe, with LICENSE and the notices
.\build.ps1 installer        # dist\Hover-Setup-<version>.exe (needs Inno Setup)
```

`build.ps1` stops a running `Hover.exe` before it builds, so the file isn't locked.

Only one Hover runs at a time (the named mutex `Local\HoverRunningInstance`). A second
launch opens the first one's window and exits. Quit an installed Hover from its tray
menu before you run your build.

Use a data folder of your own, so your build never touches your real settings and
history:

```powershell
$env:HOVER_DATA_DIR = "$env:TEMP\hover-dev"
.\native\target\release\hover.exe
```

## Test

```powershell
.\build.ps1 test
# the same as:
cargo test --manifest-path native/Cargo.toml --release --workspace
```

Three layout tests in `hover-chat` compare text widths measured in Linux Chromium
with DejaVu Sans; they skip on Windows. The D-Bus tests (`look.rs`,
`secret_service.rs`, `tray.rs`) are Linux-only.

Render every view headless with the software renderer (no GPU, no desktop needed):

```powershell
.\native\target\release\hover.exe --shots $env:TEMP\hover-shots
```

Real input and memory checks run against the release app with
`tools/hover-measure`; see [testing.md](testing.md) and [profiling.md](profiling.md).

## Logs and debugging

- The log is `hover.log` in the data folder (`%APPDATA%\Hover` by default, or
  `HOVER_DATA_DIR`). Tool starts, stops, permission questions, GPU choice and errors
  go there.
- A release build has no console (it is a Windows GUI app). Start it from a terminal
  with its output piped (for example `| Out-Host`) to see panics on stderr.
- Debug builds (`.\build.ps1 run`) keep symbols. For a debugger, open
  `native\target\debug\hover.exe` in Visual Studio or WinDbg.
- `RUST_BACKTRACE=1` prints a backtrace on a panic. Release builds use
  `panic = "abort"` and strip symbols, so use a debug build for backtraces.

## What is Windows-specific

- Renderer: femtovg on wgpu (DX12), presented through DirectComposition for per-pixel
  transparency. The windows and the office share one GPU device (`shared_gpu` in
  `main.rs`), on the adapter that drives the main display.
- Allocator: mimalloc, so freed memory goes back to Windows.
- Tray and notifications, the shortcut (`RegisterHotKey`), the notch window's
  click-through and focus rules: `apps/hover/src/win.rs`.
- Codex's Read only mode isn't offered: Codex has no sandbox on Windows.
