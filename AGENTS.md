# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A Windows desktop app (.NET 8, WPF) with one surface a hover away: **the notch**
at the top centre of the main display (after NotchOwl for Mac). At rest it is a
slim pill showing the items the user picked — a running focus timer, Claude Code /
Kiro / Codex / Cursor quota gauges — or nothing. Hovering it, clicking it
or `Alt+N` opens the workspace: a header (the Hover name, the user's command
buttons, the switched-on quotas, Workspace / Insights / Settings) over
configurable, resizable cards (today's tasks, focus timer, daily notepad, today's
events, screenshots).

The only ordinary window is the dashboard (a click on the Hover name, or a second
launch of the exe), the same workspace in a normal window; the app lives in the
tray. (Sticky notes and the edge tray were removed in 1.1; an old `notes.db` is
left on disk untouched.)

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

Requires the .NET 8 SDK. The app targets `net8.0-windows` and must build on
Windows (or with `EnableWindowsTargeting`).

## Layout

```
src/Hover/
  Core/        Model + storage: Settings, Paths, Crypto (AES-GCM, DPAPI key),
               Shortcut, Log, Layout (the card layout rules and the notch item
               ids), Quota (Claude Code / Kiro / Codex / Cursor usage readers),
               Palette (every colour, Hover's light and dark, and the VS Code theme
               reader and finder). Layout, Quota and Palette have no WPF.
  Images/      Screenshots: ShotStore (watches the folder + clipboard), Shot,
               ShotRow (a thumbnail tile), ShotDrag (drag-out payload).
  Interop/     Win32 P/Invoke, monitor enumeration, global hotkeys, HostWindow
               (the borderless, click-through window the notch is drawn in).
  Services/    Actions (tray menu commands), TrayIcon, Launcher (runs a command
               button in a terminal).
  Windows/     Image preview, rename dialog.
  Owl/         The workspace: Planner (tasks, notepad, focus time, one sealed file),
               FocusTimer, Insights, Calendar (.ics reader), OwlApp (shared state,
               the one-second tick, quota polling, the alarm), Notch (the top-centre
               host, on the main display only, and the dashboard window), WorkspaceView
               (header + the cards), Pages (Insights, Settings), Popover, Theme
               (the palette in use), Ui (brushes, builders, chart, ring gauge,
               segmented control),
               Icons (generated line icons), Corners (pill-shaped corner radii).
  Themes/      Styles.xaml (menus, tooltips), Owl.xaml (the workspace's controls).
  Assets/      hover.ico (the app icon), hover-mark.png (the logo without its dark
               square, for the workspace header), Fonts/ (Inter and Inter Display).
tests/Hover.Tests/   NUnit tests.
tests/Hover.E2E/     UI Automation run against the real app. Not in Hover.slnx.
assets/hover.png     The logo (README picture, source of the app icon).
assets/make-icon.py  Writes src/Hover/Assets/hover.ico (one frame per size) and
                     hover-mark.png from assets/hover.png. Replace hover.png and run it;
                     don't edit the outputs. Needs Pillow.
assets/make-line-icons.py  Writes src/Hover/Owl/Icons.cs from Lucide at a pinned
                     version. Add an icon to NAMES and run it; don't edit Icons.cs.
assets/demo-*        The README's pictures.
installer/Hover.iss  Inno Setup script (driven by build.ps1).
```

Note: the C# namespace is `Hover.*`. Some environment-variable and folder names
carry a legacy `Noty` reference **only** in `Core/Paths.cs`, which migrates an old
`%APPDATA%\Noty` install to `%APPDATA%\Hover` on first run. Leave that path alone.

## How it works (the parts that surprise people)

- **The notch window is borderless, topmost, and `WS_EX_NOACTIVATE`** while
  resting, so brushing it never steals focus. The bit comes off while the
  workspace is open (keyboard), and briefly for an OLE drag-out of a screenshot
  (Chromium refuses drags from a no-activate window). See `Interop/HostWindow.cs`
  (`SetAcceptsKeys`, `WhileActivatable`).
- **Pointer is polled, not hooked.** `NotchManager` reads the cursor every 50 ms.
- **The workspace's timers run at `DispatcherPriority.Normal`.** WPF runs
  Background-priority work only when no input is waiting in the queue, and in
  testing that starved the focus clock and the notch poll for 8–14 s at a time.
- **One full-size, click-through window, on the main display.** The shape grows
  from its resting size (pill, alert, or nothing) to the workspace by animating one
  `Openness` value; the window itself never resizes (that made it blink), except
  when Settings → Notch → Workspace size changes.
- **Cards are star columns with a `GridSplitter` in every gap.** Letting go of a
  splitter turns the laid-out widths back into star shares with the same total
  and saves them (`Settings.Cards`). `CardLayout.Normalize` repairs any saved
  layout: unknown ids dropped, new cards added, widths clamped, never all hidden.
- **Colours come from one `Palette`.** Hover's own light and dark are Apple's
  system colours. A VS Code theme is read from its file (following `include`),
  and only the few colour ids in `Palette.Keys` are kept, in `Settings.Theme`, so
  the theme survives the editor being removed. `Palette.Installed` lists the
  themes VS Code, Cursor, Kiro and Windsurf have on the PC. Code-built elements
  take their brushes when they are made, so a theme change rebuilds the views
  (`Theme.Changed`); the XAML styles read the same colours as dynamic resources
  that `Theme.Publish` rewrites. The resting notch is always black.
- **Command buttons run in a terminal.** `Launcher` opens a Windows Terminal tab
  (`wt -d <folder> pwsh -NoExit -Command <command>`, with `;` escaped for wt), or a
  PowerShell window when wt is missing. The shell stays open, so errors show.
- **Quotas have no official API.** `Core/Quota.cs` reads what each tool exposes,
  read-only: `api.anthropic.com/api/oauth/usage` with Claude Code's own sign-in
  (never refreshed, which would rotate Claude Code's tokens); `kiro-cli chat
  --no-interactive /usage` output; the `rate_limits` of the newest `token_count`
  event in `~/.codex/sessions/**/rollout-*.jsonl`; and `cursor.com/api/usage-summary`
  with the token from Cursor's `state.vscdb`. Each is off until switched on in
  Settings → Notch, and `OwlApp` re-reads it five minutes after the last read
  finished. A format change in any of them shows as a readable failure, not a
  crash. The Kiro read is the heavy one: kiro-cli and the MCP servers it starts
  take a few hundred MB for about eight seconds, then all exit.
- **The alarm is a sound file.** When a countdown ends, `OwlApp.RingAlarm` plays
  `Windows\Media\Alarm01.wav`; the toast sound is held back by Do not disturb.
- **The planner is encrypted** (AES-GCM, DPAPI-wrapped key). Screenshots are plain
  files on purpose, so they can be dragged into other apps.

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
  files into a plain `net8.0` NUnit project. `TestEnvironment` redirects the data
  and shots folders to a temp path via `HOVER_DATA_DIR` / `HOVER_SHOTS_DIR`.
- **There is one `HoverNotch` window per display.** UI Automation lists them in
  z-order, so the first is often another display's. The E2E tests bind the one
  over the primary display's top centre.
- **UI Automation can't see a `Border` or a `Panel`.** Give E2E hooks to a
  control or a `TextBlock`, or give the element an automation peer (`ShotRow`).
