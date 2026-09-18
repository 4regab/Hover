# AGENTS.md

Guidance for humans and AI agents working in this repository.

## What Hover is

A Windows desktop app (.NET 8, WPF) that keeps two edge panels a hover away:

- **Notes** on the right edge — an edge deck of sticky notes.
- **A screenshot tray** on the left edge — every snip and copied image, ready to
  drag out.

Neither panel shows until the pointer reaches its edge. There is no main window;
the app lives in the system tray.

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

Only one copy of Hover runs at a time (a named mutex). If a launch seems to do
nothing, an instance is already running — stop `Hover` (and any old `Noty`) first:

```powershell
Get-Process Hover, Noty -ErrorAction SilentlyContinue | Stop-Process -Force
```

Requires the .NET 8 SDK. The app targets `net8.0-windows` and must build on
Windows (or with `EnableWindowsTargeting`).

## Layout

```
src/Hover/
  Core/        Model + storage: Note, NoteStore, Store (SQLite), Crypto (AES-GCM),
               Settings, Paths, Palette, Ink (fonts), TaskSyntax.
  Deck/        The notes edge deck: DeckManager (one per display, polls the
               pointer), DeckController (per-display state machine), DeckWindow
               (the borderless, click-through host window), DeckGeom (metrics),
               Controls/ (custom-drawn tabs, pill, preview card).
  Editor/      The note text view: NoteTextBox (a RichTextBox that treats a note
               as plain text), Styler (renders text -> FlowDocument with inline
               Markdown), DocMap (maps document <-> plain-string offsets).
  Images/      The screenshot tray: ShotStore (watches the folder + clipboard),
               Shot, ShotRow (a thumbnail with a delete button), ShotDrag
               (drag-out payload), ImageStripController / ImageStripManager.
  Interop/     Win32 P/Invoke, monitor enumeration, global hotkeys, spell langs.
  Services/    Actions (menu/shortcut commands), Transfer (import/export),
               TrayIcon.
  Windows/     Ordinary windows: All Notes, Settings, image preview, rename.
  Assets/      hover.ico (the app icon).
tests/Hover.Tests/   NUnit tests.
assets/hover.svg     Icon source (the .ico in src/Hover/Assets was generated from it).
installer/Hover.iss  Inno Setup script (driven by build.ps1).
```

Note: the C# namespace is `Hover.*`. Some environment-variable and folder names
carry a legacy `Noty` reference **only** in `Core/Paths.cs`, which migrates an old
`%APPDATA%\Noty` install to `%APPDATA%\Hover` on first run. Leave that path alone.

## How it works (the parts that surprise people)

- **A note is a plain string.** The RichTextBox document is only a rendering of
  that string, rebuilt after edits. Anything in the document not derived from the
  string is discarded on the next pass. See `Editor/DocMap.cs` and `Styler.cs`.
- **Undo is home-grown.** The document is swapped out on restyle, so WPF's undo
  can't survive it; `NoteTextBox` keeps plain-text snapshots instead.
- **The deck windows are borderless, topmost, and `WS_EX_NOACTIVATE`** so hovering
  them never steals focus. That bit must be cleared for anything that needs
  activation — keyboard focus in an open note, and OLE drag-out from the image
  tray (Chromium refuses drags from a no-activate window). See
  `Deck/DeckWindow.cs` (`SetAcceptsKeys`, `WhileActivatable`).
- **Pointer is polled, not hooked.** `DeckManager` / `ImageStripManager` poll the
  cursor every ~90 ms to notice it reaching an edge.
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

- **WPF vs WinForms name clashes.** WinForms is referenced only for `Screen` and
  `NotifyIcon`; its implicit usings are dropped in the csproj. Fully-qualify or
  alias when you need a WinForms type.
- **`str_replace` on files with `—` (em dash) and non-ASCII** can be finicky;
  anchor on unique ASCII lines.
- **Tests need STA + a WPF Application** for anything touching controls; see the
  `[Apartment(ApartmentState.STA)]` fixtures. `TestEnvironment` redirects the data
  and shots folders to a temp path via `HOVER_DATA_DIR` / `HOVER_SHOTS_DIR`.
- **The single-instance mutex** will silently make a second launch exit.
