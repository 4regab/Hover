<p align="center">
  <img src="assets/hover.png" width="96" height="96" alt="Hover">
</p>

<h1 align="center">Hover</h1>

<p align="center">AI agents at work in a notch at the top of the screen.</p>



https://github.com/user-attachments/assets/8c7b498f-4b31-4d88-8861-74a28c711347



## Features

- **Agent office**: pick a project folder, type a task, and Kiro, Codex, Cursor or OpenCode does it there in the background. Run up to three at once; each gets a bot at a desk in a little 3D office that shows how it's going. Answers show as formatted text with tables, code, images and flowcharts; every session is kept (encrypted) on the office's bookshelf, and a reply picks it back up.
- **One button to start**: the circle at the bottom left shows each agent's logo; pick one and type.
- **Ask before acting**: set an agent to ask first, and the notch shows what it wants to run or change. Run, Trust or Deny it right there, or over the agent's head in the office.
- **AI quotas**: Claude Code, Kiro, Codex and Cursor usage on the notch, each as its own logo in a ring.
- **Settings in the office**: the gear opens it; each agent has its own page for model, effort and tool access.
- **Themes**: Hover light or dark, or any VS Code theme on your PC.

| | |
|:---:|:---:|
| <img src="assets/readme/new-task.jpg" alt="Starting a task: prompt, folder, access and model in one box"> | <img src="assets/readme/approval.jpg" alt="An agent asks to change a file, with Deny, Trust and Allow buttons"> |
| Start a task | Approve a change |
| <img src="assets/readme/answer.jpg" alt="A finished answer with a table, code and a flowchart"> | <img src="assets/readme/history.jpg" alt="Session history listing past tasks"> |
| Read the answer | Pick up a past session |

<sub>Screenshots use sample tasks and answers.</sub>

## Voice and projects

Hold a shortcut, say a task, let go. Hover shows what it heard and where it will run it,
then starts a new chat after three seconds. Voice is off until you switch it on.

1. **Projects** (Settings → Projects): add the folders voice may work in, with Add a
   project. Give each the other names you might say ("also called"). Each project has its
   own tool access; a new one starts at Ask first, never Full. When what you say names no
   project, or isn't clear, the task goes to the default workspace (`Hover` in your home
   folder, made when first needed; you can change it and its access).
2. **The agent**: voice always uses the default agent, the one picked in the new-task
   circle, with that agent's model and settings.
3. **Speech** (Settings → Voice): choose Local (Phonon) or Cloud (Groq).
   - Local runs on your computer and understands English only. Press Download in the
     Phonon card. Nothing is downloaded until you do. It needs, per platform:

     | | Download | On disk | During setup |
     |---|---|---|---|
     | Windows x64 | 420 MB | 1.5 GB | 1.9 GB |
     | Linux x64 | 523 MB | 1.8 GB | 2.3 GB |
     | Linux arm64 | 410 MB | about 1.8 GB (not tested yet) | about 2.2 GB |

     Remove deletes it again. Windows needs the Microsoft Visual C++ Redistributable (x64);
     the card says so before downloading.
   - Cloud sends the recording to Groq with your own key (from console.groq.com) and detects
     the language. Paste the key in Settings → Voice; Check the key tests it.
   - Hover never switches between the two on its own.
4. **Cleanup** (optional): Gemini, OpenAI or your own OpenAI-compatible service, with your
   key and model, fixes punctuation and filler words. If it fails, the original text is used.
5. **Talk**: hold Ctrl+Alt+Space (you can change it), speak, let go. Edit the task to stop
   the countdown, then press Start. Esc cancels. Try it in Settings shows what would start,
   without starting anything.

What goes where: Cloud sends the audio to Groq. Local keeps recognition on your computer.
Cleanup, when on, sends the text (never the audio) to its service. The agent gets the task.
The recording is deleted after transcription. Keys are sealed in `secrets.dat` with the same
protected key as the history, never in plain text; if that key isn't available, a key is
kept only until Hover quits.

Phonon is by Fermion Research. Its model weights are under CC-BY-4.0 and its code under
Apache-2.0. The notice and both licences are kept with the install, in
`%APPDATA%\Hover\phonon\installs\<id>\licenses` (Linux:
`~/.local/share/Hover/phonon/installs/<id>/licenses`).

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
Voice goes online only as you set it up: Phonon's download, Groq in Cloud mode, and the
cleanup service if you turn it on (see Voice and projects).

The Agent office runs Kiro (`kiro-cli acp`), Codex (`codex-acp`), Cursor
(`cursor-agent acp`) or OpenCode (`opencode serve`, on 127.0.0.1 only, with a password
made for each start) in the folder you choose. With full tool access they can edit
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

## License

MIT. See [LICENSE](LICENSE). 
