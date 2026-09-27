<p align="center">
  <img src="assets/hover.png" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">Your day in a notch at the top of the screen.</p>



https://github.com/user-attachments/assets/024e6b18-0f3e-47b9-99da-2a521644863f



Rest the pointer at the **top centre** of your main screen, or press `Alt+N`, and the
notch opens into your workspace. At rest it is a slim pill with a running timer and
your AI quotas, or nothing at all. Built with .NET 8 and WPF, for Windows 10 and 11.

## Features

- **Tasks**: add with Enter, drag to reorder, give them time limits and reminders.
- **Focus timer**: a countdown or stopwatch that keeps running in the notch.
- **Notepad**: one note a day; `Ctrl+Enter` turns a line into a task.
- **Events**: today's events from any iCal (`.ics`) calendar.
- **Screenshots**: every snip and copied image, ready to drag into any app.
- **Kiro**: pick a project folder, type a task, and Kiro CLI does it there in the background while a ghost shows how it's going.
- **Insights**: seven days of finished tasks and focus time, in Settings.
- **AI quotas**: Claude Code, Kiro, Codex and Cursor usage, in the header or on the notch.
- **Command buttons**: one click opens a terminal that runs `claude`, `kiro-cli` or any command.
- **Themes**: Hover light or dark, or any VS Code theme on your PC.
- **Your layout**: show, hide, resize and reorder cards, and pick the workspace size.


## Install

Download the latest `Hover-Setup-*.exe` from Releases and run it, or build it
yourself (below).

## Privacy

Everything lives in `%APPDATA%\Hover`. Tasks, the notepad and focus time are kept in
`planner.dat`, encrypted with AES-GCM under a key protected by Windows DPAPI.
Screenshots are ordinary files in `Pictures\Hover Shots`, so they can be dragged
into other apps.

There is no account, server or analytics. Hover only goes online for the calendar
address you add and, if you switch them on, the Cursor and Claude Code quotas (one
request every five minutes each, with the sign-in that tool already keeps). The
Kiro quota runs `kiro-cli /usage` on your PC; the Codex quota reads Codex's own logs.

The Kiro page runs `kiro-cli chat --no-interactive --trust-all-tools` in the folder
you choose. With full tool access Kiro can edit files and run commands there
without asking, so choose a folder under version control. Hover explains this the
first time you open the page.

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

## License

MIT. See [LICENSE](LICENSE). 
