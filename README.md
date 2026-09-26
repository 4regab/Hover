<p align="center">
  <img src="assets/hover.svg" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">Tasks, a focus timer, notes and today's events at the top of your screen — plus sticky notes and screenshots at its edges.</p>

Hover keeps your day a hover away. Rest the pointer at the **top centre** of the
screen, or press `Alt+N`, and a notch drops down with today's tasks, a focus timer,
a daily notepad and your calendar — a Windows take on
[NotchOwl](https://www.notchowl.com). **Sticky notes** wait on the right edge and a
**screenshot tray** on the left. Nothing sits on screen until you need it.

Built with .NET 8 and WPF. A Windows descendant of
[aimen08/noty](https://github.com/aimen08/noty).

## What it does

- **A workspace in the notch.** Hover the top centre or press `Alt+N`:
  - **Today's tasks** — add with Enter, check off, drag to reorder. Each task's `⋯`
    menu can focus it, rename it, give it a time limit, set a reminder (in 30
    minutes, in an hour, this evening, tomorrow morning, or a custom time), move
    it to tomorrow, duplicate or delete it. Unfinished tasks roll over to today.
  - **Focus timer** — pick a task and press play, or start the timer on its own.
    Countdown or stopwatch; pause, resume, add five minutes. Close the notch and
    the time keeps running in it. Timers pause while the PC sleeps.
  - **Daily notepad** — saves as you type. `Ctrl+Enter` turns the line under the
    caret into a task.
  - **Events** — today's events from any calendar's iCal (`.ics`) address or file,
    read-only, with your reminders above them.
  - **Insights** — the last seven days of tasks completed against planned, focus
    time, active days and your streak. Pick a day to see just that day.
  - **Open app** shows the same workspace in an ordinary window.
  - **Export** everything as a JSON backup from the workspace's Settings tab.
- **Notes on the right edge.** Hover the right side and your notes fan out as
  tabs. Click one to open it. Plain-text notes with live Markdown styling,
  checkbox tasks, colours, search, pinning and word-based undo.
- **Name a note yourself.** A note's title normally follows its first line. Click
  the title in an open note, or right-click its tab, to name it something else and
  it stays put however you edit the note. Clear the name and it follows the first
  line again.
- **Screenshots on the left edge.** Every snip you take and every image you copy
  lands in a tray on the left. Hover to see them as thumbnails, each with a Delete
  button. Drag one straight onto a website, a chat box, a folder or a terminal.
- **Out of the way.** Both panels are hidden until you hover the edge, so the
  screen stays clean. If you would rather they stayed put, Settings can switch
  auto-hide off for the notes deck and the screenshot tray separately.
- **Autosave and archive**, a searchable All Notes window, drag-to-reorder, and
  multi-monitor support.
- **Import / export** as Markdown, plain text, or a `.stickies` archive.
- **Local and private** — no account, server or analytics. The only network
  request is the calendar address you add, if you add one.

## Install

Requires Windows 10 or 11.

Download the latest `Hover-Setup-*.exe` from Releases and run it, or build it
yourself (below).

## Shortcuts

Global:

| Shortcut | Action |
|---|---|
| `Alt+N` | Open or close the workspace |
| `Ctrl+Alt+N` | New note |
| `Ctrl+Alt+A` | All Notes |
| `Ctrl+Alt+L` | Archive |

In the workspace:

| Shortcut | Action |
|---|---|
| `Enter` | Add the task you typed |
| `Ctrl+Enter` | In the notepad, turn the caret's line into a task |
| `Esc` | Close the workspace |

`Alt+N` mirrors NotchOwl's `Option+N`. It also means Insert in Office and File name
in file dialogs, so change it in Settings if you use those.

Inside a note:

| Shortcut | Action |
|---|---|
| `Esc` | Close the note |
| `Ctrl+F` | Find |
| `Ctrl+T` | Toggle a task |
| `Ctrl+P` | Pin |
| `Ctrl+.` | Cycle colour |
| `Ctrl+Shift+A` | Archive |
| `Ctrl+Shift+Backspace` | Delete, with ten seconds to undo |
| `Ctrl++` / `Ctrl+-` | Change text size |

Shortcuts can be changed in Settings (right-click the tray icon → Settings).

## Privacy

Notes live in `%APPDATA%\Hover`. Note bodies are encrypted with AES-GCM, and the
key is protected with Windows DPAPI. The workspace — tasks, notepad, focus time —
is kept in `planner.dat` beside them, sealed with the same key.

Screenshots are ordinary picture files in a `Hover Shots` folder under Pictures, so
you can drag one straight into another app. They are not encrypted — a file you can
drop onto any program cannot also be locked.

Nothing leaves your machine, except a request to the calendar address you add.

## Build

Requires the .NET 8 SDK or newer.

```powershell
# Build and run
.\build.ps1 release run

# Self-contained Hover.exe in .\publish
.\build.ps1 publish

# Run the tests
dotnet test .\Hover.slnx -c Release

# Windows installer (needs Inno Setup 6 or 7) in .\dist
.\build.ps1 installer
```

See [AGENTS.md](AGENTS.md) for the layout of the code and how to work in it.

## Credits

Hover began as a Windows rework of **Noty** by
[Aymen Hamza (aimen08/noty)](https://github.com/aimen08/noty) — the original
edge-of-screen sticky-notes idea and design are theirs. This project keeps that
foundation and builds on it: a full Windows/WPF implementation, an auto-hiding
screenshot tray with drag-out to any app, hover-to-reveal panels on both edges,
more font choices, and other changes.

Both the original and this fork are MIT-licensed. Thank you to the original author.

## License

MIT. Original work © Aymen Hamza; Windows rework © Hover contributors.
See [LICENSE](LICENSE).
