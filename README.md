<p align="center">
  <img src="assets/hover.png" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">AI agents at work in a notch at the top of the screen.</p>



https://github.com/user-attachments/assets/024e6b18-0f3e-47b9-99da-2a521644863f



Rest the pointer at the **top centre** of your main screen, or press `Alt+N`, and the
notch opens into the Agent office. At rest it is a slim pill with your AI quotas and
any agent at work, or nothing at all. Built in Rust with Slint and wgpu, for Windows 10 and 11 and Linux (X11, or XWayland on Wayland desktops).

## Features

- **Agent office**: pick a project folder, type a task, and Kiro, Codex or Cursor does it there in the background. Run up to three at once; each gets a bot at a desk in a little 3D office that shows how it's going. Answers show as formatted text with tables, code, images and flowcharts; every session is kept (encrypted) on the office's bookshelf, and a reply picks it back up.
- **One button to start**: the circle at the bottom left shows each agent's logo; pick one and type.
- **Ask before acting**: set an agent to ask first, and the notch shows what it wants to run or change. Run, Trust or Deny it right there, or over the agent's head in the office.
- **AI quotas**: Claude Code, Kiro, Codex and Cursor usage on the notch, each as its own logo in a ring.
- **Settings in the office**: the gear opens it; each agent has its own page for model, effort and tool access.
- **Themes**: Hover light or dark, or any VS Code theme on your PC.

## Install

Windows: download the latest `Hover-Setup-*.exe` from Releases and run it. It installs
over a 2.x install in place. Linux: `make install` (below), or the `.deb`.

## Privacy

Everything lives in `%APPDATA%\Hover` (Linux: `~/.local/share/Hover`). The agents' sessions
are kept in `agents/`, encrypted with AES-GCM under a key protected by Windows DPAPI
(Linux: the Secret Service, else a file only you can read).

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

Requires Rust 1.89 or newer (Windows: with the MSVC build tools). The version is in
`native/Cargo.toml`.

```powershell
# Windows
.\build.ps1 release run      # build and run
.\build.ps1 test             # the tests
.\build.ps1 publish          # Hover.exe in .\publish
.\build.ps1 installer        # Hover-Setup-<version>.exe in .\dist (needs Inno Setup 6 or 7)
```

```sh
# Linux
make                         # release build
make test
sudo make install            # /usr/local (PREFIX=, DESTDIR= as usual); make uninstall
make package                 # .deb and tarball in dist/
```

## License

MIT. See [LICENSE](LICENSE). 
