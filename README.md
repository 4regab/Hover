<p align="center">
  <img src="assets/hover.png" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">AI agents at work in a notch at the top of the screen.</p>



https://github.com/user-attachments/assets/024e6b18-0f3e-47b9-99da-2a521644863f



Rest the pointer at the **top centre** of your main screen, or press `Alt+N`, and the
notch opens into the Agent office. At rest it is a slim pill with your AI quotas and
any agent at work, or nothing at all. Built with .NET 10 and WPF, for Windows 10 and 11.

## Features

- **Agent office**: pick a project folder, type a task, and Kiro, Codex or Cursor does it there in the background. Run up to three at once; each gets a bot at a desk in a little 3D office that shows how it's going. Answers show as formatted text with tables, code, images and flowcharts; every session is kept (encrypted) on the office's bookshelf, and a reply picks it back up.
- **One button to start**: the circle at the bottom left shows each agent's logo; pick one and type.
- **AI quotas**: Claude Code, Kiro, Codex and Cursor usage on the notch.
- **Settings in the office**: the gear opens it; each agent has its own page for model, effort and tool access.
- **Themes**: Hover light or dark, or any VS Code theme on your PC.

## Install

Download the latest `Hover-Setup-*.exe` from Releases and run it, or build it
yourself (below).

## Privacy

Everything lives in `%APPDATA%\Hover`. The agents' sessions are kept in `agents\`,
encrypted with AES-GCM under a key protected by Windows DPAPI.

There is no account, server or analytics. Hover itself only goes online for the
Cursor and Claude Code quotas, if you switch them on (one
request every five minutes each, with the sign-in that tool already keeps). The
Kiro quota runs `kiro-cli /usage` on your PC; the Codex quota reads Codex's own logs.

The Agent office runs Kiro (`kiro-cli acp`), Codex (`codex-acp`) or Cursor
(`cursor-agent acp`) in the folder you choose. With full tool access they can edit
files and run commands there without asking, so choose a folder under version
control. Hover explains this the first time you open the page. Each tool stays
running for 5 or 15 idle minutes (Settings), then stops until you reply.

## Build

Requires the .NET 10 SDK or newer.

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
