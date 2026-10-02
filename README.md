<p align="center">
  <img src="assets/hover.png" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">
  AI coding agents at work in a notch at the top of your screen.<br>
  Hand a task to Kiro, Codex, Cursor, OpenCode or Claude Code by typing or speaking, and watch it get done.
</p>

<p align="center">
  <a href="https://github.com/4regab/Hover/actions/workflows/ci.yml"><img src="https://github.com/4regab/Hover/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/4regab/Hover/releases"><img src="https://img.shields.io/github/v/release/4regab/Hover?include_prereleases&label=release" alt="Latest release"></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-informational" alt="Windows, Linux and macOS">
  <img src="https://img.shields.io/badge/built%20with-Rust%20%2B%20Slint-orange" alt="Rust and Slint">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#voice">Voice</a> ·
  <a href="#privacy-and-security">Privacy</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

https://github.com/user-attachments/assets/8c7b498f-4b31-4d88-8861-74a28c711347

## Overview

Hover is a desktop app for Windows, Linux and macOS. It lives in a slim black island at the top
centre of your main display (on a Mac, around the camera notch). At rest, the island shows the AI quotas you switched on, the
agents at work, or a question an agent is waiting on. Hover over it, click it or press
`Alt+N` (Option-N on a Mac) to open the **Agent office**. There, each task is a bot at a desk in a small 3D
office. On a Mac the quotas sit in the menu bar instead, because the notch leaves no room.

The agents run in the background in the folder you choose, up to three at once. Hover
never opens a terminal for them. Every session is kept, encrypted, so you can pick any of
them up later.

## Features

| | |
|---|---|
| **Agent office** | Pick a folder, type a task, and the agent works there in the background. Each session gets a bot at a desk. |
| **Readable answers** | Headings, tables, checklists, images and flowcharts. Code is colour-coded, and diffs have line numbers. Command output shows the real exit code. |
| **Honest activity** | The chat shows only the thinking, subagents and changes the agent actually reports. Nothing is made up. |
| **Ask before acting** | Set a tool or a task to ask first. Approve, trust or deny each request in the notch, over the bot, or in the chat. |
| **Checkpoints** | Each turn keeps the project folder as it was. **Restore** puts the files and the chat back to an earlier answer; **Try again** goes back to before a message and sends it again. Needs Git installed. |
| **Desk card** | Click a desk to see what its agent is doing, reply to it, or open its terminal, files, diff, agents, linked pull requests and the branch's pull request. The pull request tab can set up the GitHub CLI and open a pull request for you. |
| **Helpers** | A subagent at work shows as a small bot beside its parent's desk. |
| **Sandbox** | On a Mac or Linux, agents run inside Anthropic's sandbox-runtime: they write only to their folders, can't read your keys or other apps' data, and reach only the hosts they need. Needs `srt`; Settings → Integrations says what is missing. |
| **Computer use** | Off until you switch it on. Agents drive other apps in the background through Cua Driver, without moving your pointer or taking focus. macOS first; Settings installs it and asks for its permissions. |
| **Agent browser** | macOS. Agents get a browser they can open pages in, read, click and type in; you see it in the desk card's Browser tab. |
| **Agent setup** | macOS. One click installs an agent with its maker's own installer and opens its sign-in. |
| **Reply, queue, pause** | Replies sent during a run wait their turn. Pause stops the current answer, and the next queued reply starts once the agent confirms. |
| **Voice** | Hold `Ctrl+Alt+Space`, say a task, let go. Speech is turned into text on your computer (Phonon) or by Groq. |
| **Projects** | Register the folders voice may work in, the other names you call them by, and each folder's own tool access. |
| **AI quotas** | Claude Code, Kiro, Codex and Cursor usage on the notch (in the menu bar on a Mac), each as its own logo in a ring. |
| **Themes** | Hover's light or dark, the system's, or any VS Code theme installed on your PC. |

| | |
|:---:|:---:|
| <img src="assets/readme/new-task.jpg" alt="Starting a task: prompt, folder, access and model in one box"> | <img src="assets/readme/approval.jpg" alt="An agent asks to change a file, with Deny, Trust and Allow buttons"> |
| Start a task | Approve a change |
| <img src="assets/readme/answer.jpg" alt="A finished answer with a table, code and a flowchart"> | <img src="assets/readme/history.jpg" alt="Session history listing past tasks"> |
| Read the answer | Pick up a past session |

<sub>Screenshots use sample tasks and answers.</sub>

## Install

Download the latest build from [Releases](https://github.com/4regab/Hover/releases).

| Platform | Package |
|---|---|
| Windows 10/11 (x64) | `Hover-Setup-<version>.exe`. It installs over a 2.x install in place and keeps your data. |
| Ubuntu 22.04+ and Debian-based (x64) | `hover_<version>_amd64.deb` |
| Other Linux (x64) | `hover-<version>-linux-x86_64.tar.gz`, or [build from source](#build-from-source) |
| macOS 14+ (Apple Silicon or Intel) | [Build from source](#build-from-source) for now; there is no release package yet. See [docs/MACOS.md](docs/MACOS.md). |

Hover then lives in the tray (the menu bar on a Mac). Only one copy runs at a time: starting it again opens the
dashboard of the copy that is already running.

### Agents

Hover drives the agents you already have. Install and sign in to at least one:

| Agent | Program Hover runs |
|---|---|
| Kiro | `kiro-cli` |
| Codex | `codex-acp` |
| Cursor | `cursor-agent` |
| OpenCode | `opencode` |
| Claude Code | `claude` (signed in, or an API key, Bedrock or Vertex) |

The office shows which agents it found, and how to install or sign in to the rest. On a
Mac, each agent's page in Settings has a **Set up** row that installs what is missing and
opens the tool's own sign-in; Hover never sees the credentials.

## Getting started

1. Open the office: hover over the notch, click it, or press `Alt+N` (Option-N on a Mac).
2. Press the circle at the bottom left, pick an agent, and choose a folder.
3. Choose its tool access: **Full**, **Ask first**, **Ask always** or **Read only**.
4. Type the task and press Enter.

Each agent has its own page in Settings (the office menu → Settings) for its model,
effort and tool access. The effort choice shows once the agent has reported its effort
levels, after its first run.

> [!WARNING]
> With **Full** access an agent can change files and run commands in its folder without
> asking. Use a folder under version control.

## Voice

Hold a shortcut, say a task, let go. Hover shows what it heard, the folder, the agent and
its access, then starts a **new chat** after a five-second countdown (Settings → Voice → Start on
its own: Off, 3, 5 or 10 s). With a chat open and its reply box open, hold the shortcut with
the pointer over the chat to dictate into the reply instead. Voice is off until
you switch it on in Settings → Voice.

1. **Add projects** (Settings → Projects). Pick a folder, and add the other names you might
   say for it. Each project has its own tool access; a new one starts at Ask first, never
   Full. When a task names no project, or the name isn't clear, it goes to the default
   workspace: `Hover` in your home folder, made the first time it is needed. You can change
   that folder and its access.
2. **The agent.** Voice uses the default agent (the one last picked in the new-task circle)
   with that agent's own model and settings.
3. **Choose speech recognition** (Settings → Voice):
   - **Local (Phonon)** runs on your computer and understands **English only**. Press
     Download on the Phonon card. Nothing is downloaded before you do.
   - **Cloud (Groq)** sends the recording to Groq with your own key from
     [console.groq.com](https://console.groq.com) and detects the language. Check key
     tests it.

   Hover never switches between the two on its own.
4. **Cleanup (optional).** Gemini, OpenAI or any OpenAI-compatible service, with your key
   and model, fixes punctuation and filler words. If it fails, the original text is used.
5. **Talk.** Hold `Ctrl+Alt+Space` (on a Mac, Control-Option-Space; you can change it), speak, and let go. Edit the task to
   stop the countdown, then press Start. Esc cancels. Settings → Voice → Try it shows what
   would start, without starting anything.

On a Mac, **Local (Phonon)** is off for now (the card says so): use **Cloud (Groq)**. macOS
asks for the Microphone the first time Hover listens.

### Local speech requirements

| Platform | Download | On disk | Free space during setup |
|---|---|---|---|
| Windows x64 | 420 MB | 1.5 GB | 1.9 GB |
| Linux x64 | 523 MB | 1.8 GB | 2.3 GB |
| Linux arm64 | 410 MB | about 1.8 GB (not tested yet) | about 2.2 GB |

The model itself is 164 MB. The rest is the private Python and PyTorch runtime that
Phonon's engine needs; Hover keeps it in its own folder and never touches a system Python.
Windows also needs the Microsoft Visual C++ Redistributable (x64); the card says so before
anything is downloaded. Remove deletes the whole install.

## Computer use

Switch it on in Settings → Integrations. Hover then gives every agent
[Cua Driver](https://github.com/trycua/cua), which lets it see and operate other apps.
Settings installs it and asks for its two permissions (Accessibility and Screen Recording),
which go to CuaDriver, not to Hover.

- It works in the background: input goes to the app the agent names, without moving your
  pointer or taking focus. Hover's guard turns foreground input into background input and
  refuses what would get in your way: input to the whole desktop, raising a window, moving
  windows, the clipboard and quitting apps.
- Under **Ask first**, **Ask always** and **Read only**, its calls are treated like any other
  tool call: asked about, or refused.
- The desk card's **Screen** tab shows the agent's apps live (on a Mac this needs Hover's own
  Screen Recording permission; the tab has an **Allow…** button).
- It is built for macOS. On Linux Cua is a pre-release, and the switch says so. On Windows it
  works if Cua Driver is installed, but without the guard.

## Privacy and security

Everything lives in `%APPDATA%\Hover` on Windows, `~/.local/share/Hover` on Linux and
`~/Library/Application Support/Hover` on a Mac.
There is no account, server or analytics.

- **Sessions** are encrypted with AES-GCM under a key protected by Windows DPAPI, on a Mac
  by the login Keychain, or on
  Linux by the Secret Service (else a file only you can read).
- **API keys** (Groq, cleanup) are sealed in `secrets.dat` with that same key, never in
  plain text. If the key isn't available, a key you enter is kept only until Hover quits.
- **Network.** Hover itself goes online only for what you switch on:
  - the Cursor and Claude Code quotas, once every five minutes each;
  - the Phonon download;
  - Groq, in Cloud mode;
  - the cleanup service.

  The Kiro quota runs `kiro-cli /usage` on your PC, and the Codex quota reads Codex's own
  logs.
- **Voice data.** Cloud sends the audio to Groq; Local keeps recognition on your computer.
  Cleanup gets the text, never the audio. The recording is deleted after it is turned into
  text.
- **Checkpoints** are copies of the files in your project folder (what its `.gitignore` leaves out is not copied), kept in a Git store in the data folder (`checkpoints`), unencrypted like the files themselves. Deleting a session deletes its copies.
- **Agents** run as hidden child processes in the folder you choose, and stop with Hover.
  OpenCode's server listens on 127.0.0.1 only, with a password made for each start. Claude
  Code runs as one process per conversation, in its folder, with your own Claude Code settings. Each
  agent stops after 5 or 15 idle minutes (Settings) and starts again when you reply.
- **Sandbox.** On a Mac and on Linux the agents are started under `srt` (on by default; off
  with Settings → Integrations → Sandbox). They can write only to their folders, their own
  state and temp, can't read keychains, mail or other apps' data, and reach the network
  only through srt's proxy, to the hosts their tool needs. If `srt` isn't installed the
  agents start unsandboxed and Settings says so.
- **Agent browser.** Hover talks to each agent's browser tool over a Unix socket only you
  can use, with a token made for each start.
- **Other tools you switch on.** Computer use hands agents the right to see and operate
  your apps; GitHub CLI setup and Create pull request run `gh` and `git` on your PC and
  push to the remote you pick.

## Platform support

| | Windows 10/11 x64 | Linux x64 (X11) | Linux on Wayland | macOS 14+ |
|---|---|---|---|---|
| Notch, office, Settings | ✓ | ✓ | ✓ through XWayland | ✓ around the camera notch (unverified on a Mac) |
| Quotas | on the notch | on the notch | on the notch | in the menu bar |
| Global shortcuts (`Alt+N`, voice) | ✓ | ✓ | Only while an XWayland window has focus | ✓ (Option-N, Control-Option-Space) |
| Local speech (Phonon) | ✓ | ✓ (arm64 not tested yet) | ✓ | off, use Cloud (Groq) |
| Sandbox | off | ✓ | ✓ | ✓ |
| Agent browser and Browser tab | off | off | off | ✓ |
| One-click agent setup | off | off | off | ✓ |
| Computer use | ✓ (without the guard) | pre-release Cua | pre-release Cua | ✓ |
| Desk card, helpers, Pull request tab | ✓ | ✓ | ✓ | ✓ |
| Release package | installer | .deb, tarball | .deb, tarball | none yet |

Where a feature is off, its switch says why. The Rust port hasn't been run on a Mac yet, so the macOS
column is what the code is written to do and what builds, not something tested: see
[docs/MACOS.md](docs/MACOS.md).

## Build from source

Requires Rust 1.89 or newer. On Windows, install the MSVC build tools as well. The
toolchain Hover is tested with is pinned in `rust-toolchain.toml`.

On Ubuntu or Debian, the build needs these packages (the tests also use
`fonts-dejavu-core dbus gnome-keyring python3-gi`):

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libfreetype-dev \
  libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev
```

```powershell
# Windows
.\build.ps1 release run      # build and run
.\build.ps1 test             # all tests
.\build.ps1 publish          # hoverai.exe in .\publish
.\build.ps1 installer        # Hover-Setup-<version>.exe in .\dist (needs Inno Setup 6 or 7)
```

```sh
# Linux
make                         # release build
make test
sudo make install            # /usr/local (PREFIX=, DESTDIR= as usual); make uninstall
make package                 # .deb and tarball in dist/
```

```sh
# macOS (needs Xcode's command line tools: xcode-select --install)
cargo build --release -p hover     # target/release/hoverai
sh packaging/macos/bundle.sh       # dist/Hover.app, not signed
```

The Mac build is for now a local one: CI only checks that it compiles, and makes no
package. [docs/MACOS.md](docs/MACOS.md) has the permissions it asks for.

Developer documentation is in [`docs/development`](docs/development): architecture,
testing, profiling and Windows notes. [`AGENTS.md`](AGENTS.md) summarises how the app
works for people and AI agents changing it.

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first. Every
pull request runs the tests on Windows and Linux, and a compile check on macOS.

The macOS support builds on Arz's ([@Entourage397](https://github.com/Entourage397)) macOS
v1.0 for Hover 2.x, which was Swift and C#.

## License

[MIT](LICENSE)
