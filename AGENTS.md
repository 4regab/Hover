# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A Windows desktop app (.NET 8, Avalonia) that keeps two edge panels a hover away:

- **Notes** on the right edge — an edge deck of sticky notes.
- **A screenshot tray** on the left edge — every snip and copied image, ready to
  drag out.

Neither panel shows until the pointer reaches its edge. There is no main window;
the app lives in the system tray.

The app was rewritten from WPF to Avalonia. The screenshots half is done; the
notes half is being rebuilt. See "What is not built yet" below before planning
work.

## Build, test, run

All commands run from the repo root in PowerShell. Never use `cd`; the paths are
relative to root.

```powershell
# Build everything
dotnet build .\Hover.slnx -c Release

# Run the tests
dotnet test .\Hover.slnx -c Release

# Build and launch
.\build.ps1 release run

# Self-contained single-file Hover.exe in .\publish
.\build.ps1 publish

# Installer in .\dist (needs Inno Setup 6 or 7)
.\build.ps1 installer
```

If a launch seems to do nothing, a copy is probably already running — the app has
no window, so the only sign of life is the tray icon:

```powershell
Get-Process Hover -ErrorAction SilentlyContinue | Stop-Process -Force
```

Requires the .NET 8 SDK. The app targets `net8.0-windows` and must build on
Windows (or with `EnableWindowsTargeting`).

## Layout

```
src/Hover/
  Core/        Model + storage: Note, NoteStore, Store (SQLite), Crypto (AES-GCM),
               Settings, Paths, Palette, Ink (fonts), TaskSyntax, DeckStyle.
  Deck/        HoverWindow — the borderless, topmost, no-focus host window the
               edge panels sit in, including the Win32 region shaping that makes
               its blank areas click-through.
  Images/      The screenshot half: ShotStore (watches the folder + clipboard),
               Shot/ShotItem, ShotTray + ShotRowView (the panel and its rows),
               ShotGroups (day headings), Snip + SnipOverlay (drag a box),
               Markup + MarkupCanvas + AnnotateWindow (draw on a picture),
               TrayPreviewWindow (temporary host, see below).
  Interop/     Win32 P/Invoke, monitor enumeration, edge-wake timing, global
               hotkeys, the hidden message window, clipboard pictures, screen
               capture.
  Services/    Tray (tray icon, menu, the screenshot shortcut).
  Themes/      Canvas.axaml — the panel colours, hover reveals and press feedback.
  Assets/      hover.ico (the app icon, also loaded at run time for the tray).
tests/Hover.Tests/   NUnit tests, headless Avalonia.
assets/hover.svg     Icon source (the .ico in src/Hover/Assets was generated from it).
installer/Hover.iss  Inno Setup script (driven by build.ps1).
```

Note: the C# namespace is `Hover.*`. Some environment-variable and folder names
carry a legacy `Noty` reference **only** in `Core/Paths.cs`, which migrates an old
`%APPDATA%\Noty` install to `%APPDATA%\Hover` on first run. Leave that path alone.

## What is not built yet

The notes half has no UI: no note editor, no right-edge deck, no All Notes or
Settings window. `Core/` already has the model, storage and encryption for it.
The screenshot panel is shown by `TrayPreviewWindow`, an ordinary window standing
in until the panel gets its real left-edge `HoverWindow`.

Spell check is gone and is not coming back soon: neither Avalonia nor AvaloniaEdit
has it.

## How it works (the parts that surprise people)

- **A note is a plain string.** Anything the editor shows is a rendering of that
  string, rebuilt after edits.
- **The edge windows are borderless, topmost, and `WS_EX_NOACTIVATE`** so hovering
  them never steals focus. The flag must be set before the window is first shown,
  and cleared for anything that needs activation — keyboard focus in an open note,
  and drag-out from the screenshot panel. See `Deck/HoverWindow.cs`
  (`SetAcceptsKeys`, `WhileActivatable`).
- **Clicks do not fall through blank areas on their own.** WPF got that free from
  layered-window hit testing; Avalonia does not. `HoverWindow` gives Windows an
  explicit shape with `SetWindowRgn`, and anything painted rather than built from
  controls needs `ICustomHitTest` to be reachable at all — see `MarkupCanvas`.
- **Pointer is polled, not hooked.** `EdgeWake` is asked once per tick whether the
  pointer has sat in an edge band long enough.
- **Marks on a picture are stored in the picture's own pixels**, never in window
  units, so the saved file matches what was on screen whatever size the editor was.
- **Note bodies are encrypted** (AES-GCM, DPAPI-wrapped key). Screenshots are
  plain files on purpose, so they can be dragged into other apps.

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

- **Avalonia type names shadow their own namespaces.** `WindowDecorations`,
  `HorizontalAlignment`, `VerticalAlignment` and `Screens` all collide with
  members in scope; qualify them (`Avalonia.Controls.WindowDecorations.None`,
  `Avalonia.Layout.HorizontalAlignment`, `Interop.Screens.Cursor`).
- **A message-window handler must name the messages it wants.** Answering every
  message also answers `WM_NCCREATE`, and Windows then abandons the half-built
  window. See `Interop/MessageWindow.cs`.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky;
  anchor on unique ASCII lines.
- **Tests that touch controls need `[AvaloniaTest]`**, not `[Test]`, or there is no
  render interface and no UI thread. `TestEnvironment` redirects the data and
  shots folders to a temp path via `HOVER_DATA_DIR` / `HOVER_SHOTS_DIR`. Headless
  UI can be rendered to a PNG with `window.GetLastRenderedFrame()`.
- **Nothing stops a second copy launching.** The old app had a named mutex; this
  one does not yet.
