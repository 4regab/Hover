<p align="center">
  <img src="assets/logo.svg" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">Tasks, a focus timer, a notepad, today's events and your screenshots, in a notch at the top of your screen.</p>

Hover keeps your day a hover away. Rest the pointer at the **top centre** of the
screen, or press `Alt+N`, and a notch drops down with today's tasks, a focus timer,
a daily notepad, your calendar and your screenshots. At rest it is a slim pill that
can show the time, a running timer, and how much of your Claude Code, Kiro, Codex
or Cursor plan is used.

Built with .NET 8 and WPF.

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
  - **Screenshots** — every snip you take and every image you copy, as
    thumbnails. The scissors button in the card opens the Windows snipping tool.
    Drag a thumbnail straight onto a website, a chat box, a folder or a
    terminal; click to open it; right-click to copy, rename or delete.
  - **Insights** — the last seven days of tasks completed against planned, focus
    time, active days and your streak. Pick a day to see just that day.
  - **Open app** shows the same workspace in an ordinary window.
- **Cards your way.** Show or hide any card from the layout button in the header,
  drag the gap between two cards to resize them, and reorder them in
  Settings → Cards. The layout is saved.
- **A notch you choose.** Settings → Notch: keep the notch always visible, and pick
  what its resting pill shows — the time, the running focus timer, and quota
  gauges for **Claude Code**, **Kiro CLI**, **Codex** and **Cursor**. Switched-on
  quotas always show in the workspace header; "Keep quotas on the notch" also keeps
  them on the resting pill. Each quota is off until you switch it on:
  - Claude Code — asked of api.anthropic.com with the sign-in Claude Code keeps in
    `%USERPROFILE%\.claude\.credentials.json` (a Pro or Max plan).
  - Kiro CLI — read from `kiro-cli chat --no-interactive /usage` (needs `kiro-cli`
    on PATH and signed in).
  - Codex — read from the rate limits Codex records in its own session logs
    (`%USERPROFILE%\.codex\sessions`), no network.
  - Cursor — asked of cursor.com with the sign-in Cursor already keeps on this PC.
  Quotas refresh every five minutes.
- **Light or dark.** Settings → General → Appearance: follow Windows, or pick one.
- **Local and private** — no account, server or analytics. The only network
  requests are the calendar address you add and, if switched on, the Cursor and
  Claude Code usage checks.

## Install

Requires Windows 10 or 11.

Download the latest `Hover-Setup-*.exe` from Releases and run it, or build it
yourself (below).

### Upgrading from 1.0

Sticky notes and the edge tray are gone; the notch's Notepad and Screenshots cards
take their place. Your old notes are not deleted — `notes.db` stays in
`%APPDATA%\Hover` untouched — but they are encrypted and 1.1 can't open them. To
keep them as files, export them from 1.0 (tray icon → Export) before upgrading, or
reinstall 1.0 to do it later. Screenshots stay where they were, in `Pictures\Hover Shots`.

## Shortcuts

| Shortcut | Action |
|---|---|
| `Alt+N` | Open or close the workspace (global) |
| `Enter` | Add the task you typed |
| `Ctrl+Enter` | In the notepad, turn the caret's line into a task |
| `Esc` | Close the workspace |

`Alt+N` also means Insert in Office and File name in file dialogs, so change it in
Settings → General if you use those.

## Privacy

Everything lives in `%APPDATA%\Hover`. The workspace — tasks, notepad, focus
time — is kept in `planner.dat`, encrypted with AES-GCM under a key protected by
Windows DPAPI.

Screenshots are ordinary picture files in a `Hover Shots` folder under Pictures, so
you can drag one straight into another app. They are not encrypted — a file you can
drop onto any program cannot also be locked.

Nothing leaves your machine, except a request to the calendar address you add and,
if you switch those quotas on, one request every five minutes each to cursor.com
and api.anthropic.com, sent with the sign-in each tool already keeps.

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

## License

MIT. See [LICENSE](LICENSE). Hover includes the Inter font and Lucide icons; their
licences are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).
