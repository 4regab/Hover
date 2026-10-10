# Architecture

Hover is one Go module (`github.com/4regab/Hover`) at the repository root. One program
(`cmd/hover`: `hoverai` on Windows, `hover` on Linux) holds the whole product. The packages in
`internal/` keep the parts that have no window apart from the parts that do, so most of the
logic builds and runs anywhere. (The Rust version this replaced is at the git tag `rust-final`.)

## The big picture

The same code builds for Windows and Linux. Nearly all of it is shared: the backend packages
have no window and no OS calls of their own, and the interface is one set of Go files drawn
with Gio. What differs per OS is a thin layer of adapters, picked at compile time by file name
(`_windows.go`, `_linux.go`) or a `//go:build` line.

```mermaid
flowchart TB
    subgraph UI["Frontend: shared"]
        ui["internal/ui<br/>(Gio: notch, office, Settings, chat view, desk card)"]
        glue["internal/shell + internal/app<br/>windows, timers, pages, voice and desk glue"]
    end

    subgraph Backend["Backend: shared, no window"]
        core["core<br/>paths, settings, crypto, history, secrets, projects"]
        agents["agents<br/>ACP host, OpenCode server, Claude Code, sessions, routing"]
        quota["quota<br/>Claude Code, Kiro, Codex, Cursor"]
        chat["chat + raster + text<br/>thread layout and CPU painter"]
        md["md + diagram<br/>Markdown, Mermaid"]
        notch["notch<br/>geometry, animation, hover rules"]
        office["office + gpu<br/>3D scene on wgpu-native, its own goroutine"]
        voice["voice<br/>capture, Groq, Phonon, cleanup"]
    end

    subgraph Win["Windows adapters"]
        w1["platform/win: notch window, tray, RegisterHotKey, hold-to-talk"]
        w2["core/platform_windows.go: DPAPI, autostart, dark mode"]
        w3["agents/proc_windows.go: Job objects"]
        w4["platform/win/gfx.go: Direct3D 11, DirectComposition"]
    end

    subgraph Lin["Linux adapters"]
        l1["platform/wayland: Hover's own Wayland client, layer-shell notch"]
        l2["platform/linux: tray and notifications over D-Bus, portals, shortcuts"]
        l3["core/dbus_linux.go: Secret Service, XDG"]
        l4["agents/proc_linux.go: process groups, PDEATHSIG"]
        l5["Gio on EGL"]
    end

    ui --> glue
    glue --> Backend
    glue -- "_windows.go" --> Win
    glue -- "_linux.go" --> Lin
    core -. "per OS" .-> w2
    core -. "per OS" .-> l3
    agents -. "per OS" .-> w3
    agents -. "per OS" .-> l4
```

The interface is Go code (`internal/ui`), so there is no UI file to ship and no interpreter at
run time. The same code draws on both OSes; only the window and surface under it differ
(see [Frames](#the-offices-frames-and-their-lifetime)).

## How the packages depend on each other

Arrows point at what a package uses (the main ones). `core` imports nothing else from the
module, and no backend package imports Gio.

```mermaid
flowchart LR
    hover["cmd/hover<br/>(hoverai)"]
    shell
    app
    ui
    agents
    core
    quota
    chat
    md
    diagram
    notch
    office
    gpu
    raster
    text
    voice
    screen
    music
    audio
    plat["platform/<br/>win, linux, wayland"]
    backend
    hb["cmd/hover-backend"]

    hover --> shell & app & core & ui & plat
    shell --> agents & app & chat & core & md & music & notch & office & plat & screen & text & ui & voice
    ui --> app & core & raster
    app --> agents & chat & core & office & quota
    voice --> agents & core & screen
    quota --> agents & core
    agents --> core
    backend --> agents & core & quota
    hb --> backend
    office --> core & gpu & raster & text
    chat --> md & raster & text
    md --> diagram
    raster --> text
    music --> audio & core
    screen --> core & plat
    plat --> core & notch
```

`core` is the floor: everything that stores or reads the user's data goes through it.
`diagram`, `notch`, `gpu`, `text` and `audio` depend on nothing in the module.

## How the builds work

Both platforms run the same `go build` of the same module. They differ in the wrapper script
and in how the result is packaged. Windows needs no C compiler; Linux needs one for EGL.

```mermaid
flowchart TB
    src["module at the root<br/>cmd + internal + VERSION"]

    subgraph WinB["Windows: build.ps1"]
        wc["go build -ldflags -H=windowsgui<br/>CGO_ENABLED=0"]
        wexe["publish/hoverai.exe + wgpu_native.dll<br/>(downloaded, wgpu-native v29.0.0.0)"]
        wpub["publish/<br/>+ LICENSE, THIRD-PARTY-NOTICES.txt"]
        wiss["Inno Setup (ISCC) + packaging/windows/Hover.iss"]
        wout["dist/Hover-Setup-version.exe"]
        wc --> wexe --> wpub -->|"build.ps1 installer"| wiss --> wout
    end

    subgraph LinB["Linux: Makefile"]
        lc["go build -tags nowayland,nox11,novulkan<br/>(cgo: gcc and the EGL headers)"]
        lexe["hover-linux + lib/libwgpu_native.so (make wgpu)"]
        lpkg["packaging/linux/package-linux.sh"]
        ldeb["dist/hover_version_amd64.deb"]
        ltar["dist/hover-version-linux-x86_64.tar.gz"]
        linst["make install<br/>PREFIX/bin/hover + icon + .desktop"]
        lc --> lexe -->|"make package"| lpkg --> ldeb & ltar
        lexe -->|"make install"| linst
    end

    src --> wc
    src --> lc
```

- The program carries its fonts, icons, music and the Phonon locks and check sample
  (`go:embed`), so the installers ship one executable, the wgpu-native library and the
  licence files.
- The version is the number in `VERSION`. `build.ps1`, the Makefile and the installers read
  it, and put it in the program with `-ldflags "-X .../internal/shell.Version=..."`;
  `hoverai --version` prints it.
- The Go version is the one `go.mod` names. An older Go fetches it the first time.
- Gio is `third_party/gioui.org` (patched; see its README), taken in by a `replace` in `go.mod`.

### CI and releases

`.github/workflows/ci.yml` runs on GitHub's own runners. It runs no Go tests. One job per OS
builds and checks what it ships, and on a release the same jobs build the installers from the
build they just checked.

```mermaid
flowchart LR
    push["push to main or go-port<br/>or a pull request"] --> cj & wj & lj & mj
    tag["push of tag vX.Y.Z, or of a new<br/>VERSION to main"] --> cj & wj & lj & mj
    hand["Run workflow, by hand"] --> pj

    subgraph cj["check (ubuntu-22.04)"]
        c1["VERSION matches version.go, gofmt,<br/>go vet for Linux, Windows and macOS,<br/>the Mac backend builds"]
    end

    subgraph wj["windows job (windows-2022)"]
        wb["build.ps1 publish"] --> wd["start the app, Alt+N, Esc,<br/>second launch"] --> wi["installer over the 5.x setup,<br/>checks, uninstall"]
    end

    subgraph lj["linux job (ubuntu-22.04)"]
        lp["make package"]
    end

    subgraph mj["macos job (macos-15, Apple Silicon)"]
        mc["build-macos.sh, the packaged backend<br/>against stand-ins; release: Intel too,<br/>disk images"]
    end

    subgraph pj["pictures (windows-2022)"]
        pp["hoverai --shots: every view"]
    end

    cj & wj & lj & mj --> rel
    rel["release job (release only)<br/>tags the commit; Latest GitHub release with the .exe, .deb, .tar.gz, .dmg"]
```

A tag whose version doesn't match `VERSION` fails before anything builds. A failing job means
nothing is published. The release waits for all four jobs, the macOS one included: it builds
the disk images (Apple silicon and Intel), runs the packaged backend against stand-in tools
(nothing has been run on a Mac yet; see `docs/MACOS.md`), and fails a pull request whose Mac code
doesn't compile. Pushes that only touch Markdown, `docs/` or the README's pictures don't run CI.

## What runs at run time

One process. The UI goroutine owns every window; the heavy work runs on goroutines of its own
and reaches the UI only through `UIDo`. The agents are child processes.

```mermaid
flowchart LR
    subgraph P["hoverai process"]
        ui["UI goroutine<br/>window loop, notch, office UI, Settings"]
        rt["app.Hover (hover.go)<br/>settings, history, sessions, quota poller"]
        turns["one goroutine per running turn"]
        off["office goroutine<br/>wgpu scene"]
        vw["voice goroutines<br/>capture, transcribe, route, countdown"]
        ui <-->|"UIDo / hooks"| rt
        rt --> turns
        ui <--> off
        ui <--> vw
    end

    turns -->|"JSON-RPC over stdio"| acp["kiro-cli acp, codex-acp, cursor-agent acp"]
    turns -->|"HTTP on 127.0.0.1"| oc["opencode serve"]
    turns -->|"stream-json over stdio, one per conversation"| cc["claude (Agent SDK mode)"]
    vw -->|"one run per recording"| ph["Phonon helper<br/>(managed Python + fermion)"]
    vw -->|"HTTPS"| groq["Groq, cleanup service"]
    rt --> disk[("data folder<br/>settings.json, agents/, secrets.dat")]
```

The child processes sit in a Windows job object or a Linux process group, so they stop
when Hover does.

## Packages

| Package | What it owns | Window? |
|---|---|---|
| `internal/core` | The data folder (`paths.go`, the old `Noty` move), `settings.json` (`settings.go`, `json.go`: the exact bytes 2.x wrote), the key and encryption (`crypto.go`: AES-GCM, key kept by DPAPI or the Secret Service), the sealed session history (`history.go`), API keys sealed in `secrets.dat` and images (`store.go`), projects, the default workspace and the voice settings (`projects.go`), the single-instance lock (`single*.go`), colours and VS Code themes (`palette.go`), and the OS adapters (`platform_*.go`, `dbus_linux.go`, `macos.go`). | No |
| `internal/agents` | Running the agents: the ACP host (`acp.go`, JSON-RPC over stdio for Kiro, Codex and Cursor), OpenCode's local server (`opencode.go` over `http.go`), Claude Code in its Agent SDK mode (`claude.go`), the `Runtime` they sit behind (`runtime.go`), the sessions and their limits (`session.go`, `sessions.go`), permission questions (`ask.go`), voice's project routing (`route.go`), the office's state message (`state.go`), process groups and Windows jobs (`proc*.go`). | No |
| `internal/quota` | The four quota readers (Claude Code, Kiro, Codex, Cursor) and their five-minute schedule. | No |
| `internal/md`, `internal/diagram` | Markdown and Mermaid flowcharts, the same output as 2.x's `md.js` and `diagram.js`. | No |
| `internal/chat`, `internal/raster`, `internal/text` | The chat thread: layout per message (cached), selection, copy, images; the CPU painter (`raster`) and text shaping and line breaking (`text`). | No |
| `internal/notch` | The notch's geometry, animation and hover rules. | No |
| `internal/office`, `internal/gpu` | The office: scene, bots, wall canvases, camera, picking and pacing (`office.go`, `scene.go`, `bot.go`), the wgpu renderer (`render.go`, `office.wgsl`), the page's background and vignette (`page.go`), and its own goroutine (`live.go`). `gpu` is Hover's small binding to wgpu-native. | No (renders offscreen) |
| `internal/ui` | The interface on Gio: palette, icons, the tools' marks, the controls, Settings, the notch, the office views, the chat view, the desk card and panel, the voice card, the app window's title bar. | Yes |
| `internal/app` | The app without a window: Settings as data (`pages.go`, `view.go`), the shared state `Hover` (`hover.go`), the resting notch (`rest.go`), key names (`keys.go`). | No |
| `internal/shell` | The glue: windows, timers, the notch (`notchctl.go`), the office (`office.go`), chat, desk, new task, voice, and the system bindings (`env_windows.go`, `env_linux.go`). | Yes |
| `internal/platform` | `win` (Win32 windows, Direct3D 11, DirectComposition, tray), `linux` (D-Bus tray and notifications, portals, pickers, clipboard), `wayland` (the notch as a layer-shell surface, the app window, keyboard, input method). | Yes |
| `internal/voice`, `audio`, `music`, `screen` | Voice (capture, Groq, cleanup, Phonon), the system's audio output, the office's beats, the Screen panel's capture. | No |
| `internal/backend`, `cmd/hover-backend` | The Mac app's backend: core, agents and quota behind JSON lines on stdin and stdout. | No |
| `internal/shots`, `cmd/ui-shots`, `cmd/office-shot` | The pictures (`--shots`), and the office alone. Not part of the product's work. | Yes |
| `cmd/notch-spike`, `cmd/hover-data` | The first notch prototype (Windows), and a tool that writes and reads a data folder. Not shipped. | Yes / No |

## Boundaries

- **UI goroutine.** The window loop runs everything in `internal/shell` and `internal/ui`.
  Other goroutines reach it only through `Env.UIDo` (`internal/shell/env.go`), which posts a
  function to the loop.
- **Runtime.** `app.Hover` (`internal/app/hover.go`) is the shared state: settings, history,
  one `Runtime` per tool, the sessions, the quota poller. It has no UI; views register hooks
  (`OnSessions`, `OnQuotas`, `OnNotify`) that fire off the UI goroutine.
- **Sessions.** `KiroSessions` keeps every session behind one lock. Each turn runs on its own
  goroutine. `changed` and `ended` fire with the lock released. At most three turns run at
  once (`MaxRunning`); six sessions keep desks (`MaxKept`).
- **Storage.** `AgentHistory` writes the index and one sealed file per session off the UI
  goroutine, in order. `Hover.Shutdown` flushes the history and settings on quit.
- **Views.** The notch and the app window show the same state; `internal/shell` pushes it into
  both (`officePush` in `office.go`).

## An agent task, end to end

1. The new-task box (`internal/ui/officenew.go`) is clicked; `internal/shell/newtask.go`
   handles it.
2. It calls `KiroSessions.StartBound` (`StartAs` is the short form) with the tool, folder,
   prompt, images and the access picked.
3. `sessions.go` gives the session a desk, saves it, and starts a turn goroutine that calls
   the tool's `RunTask`.
4. For Kiro, Codex and Cursor the runner is `AcpHost.RunAs` (`acp.go`): it starts the tool
   once (`kiro-cli acp …`, `codex-acp`, `cursor-agent acp`), sends `session/new` or
   `session/load`, sets model, effort and access, then `session/prompt`. For OpenCode it is
   `opencode.go`: one `opencode serve` on 127.0.0.1 with a password made for that start,
   `prompt_async`, and its event stream. For Claude Code it is `claude.go`: a `claude` process
   per conversation, started in its folder in the Agent SDK's stream-json mode, `initialize`,
   then the prompt as a user message on its stdin.
5. Updates (`session/update`, OpenCode events, Claude Code's messages) become `KiroEvent`s and
   phases. The session changes, `changed` fires, and the UI marks the office dirty.
6. A permission request (`session/request_permission`) is answered off the read loop:
   `ask.go` decides what the access setting allows. The rest goes to `KiroSessions.Ask`, which
   shows it in the notch, over the bot and in the chat until the user answers or the run stops.
7. The turn ends: the result is saved, `ended` fires, and the notch shows the end (and
   a system notification when no office is in view).

## Projects and the default workspace

`internal/core/projects.go`, kept in `settings.json` beside the 2.x keys (`Projects`,
`DefaultWorkspace`, `Voice`; 2.x ignores keys it doesn't know). A project has a stable id,
a name, a folder, aliases, a voice switch and its own access (`full`, `risky`, `always`,
`read`). A new project or workspace starts at `risky` (Ask first): being registered
never grants Full. `ResolveFolder` checks a folder before use (absolute, followed links,
readable; Windows' `\\?\` prefix dropped), and `SameFolder` keeps one registration per
real folder. The default workspace is home + `Hover` (`C:\Users\<name>\Hover`,
`~/Hover`) unless the user picks another. It is made when a task first needs it
(`EnsureFolder`; no git init). Settings → Projects edits all of this (`internal/app/pages.go`,
`view.go`). Removing a project deletes no files, history or runs.

## Keys (`secrets.dat`)

`internal/core/store.go`. The Groq key (`voice.groq`) and the cleanup keys
(`cleanup.gemini`, `cleanup.openai`, `cleanup.custom`) are sealed with Hover's own key
(`note.key`, kept by DPAPI or the Secret Service) in `secrets.dat` beside
`settings.json`, written to a temp file and renamed. Settings holds no keys. When Hover's
key isn't there this run, a key is kept in memory until Hover quits (`ThisRunOnly`)
and Settings says so. It is never written in the clear. Errors never contain a key.

## Voice

Hold the shortcut (Ctrl+Alt+Space at first), speak, let go. The recording becomes
text, is cleaned up if that is on, is routed to a project or the default workspace,
and shows in the notch as a preview. After three seconds it starts a new chat. Voice is
off until switched on in Settings → Voice. Only new tasks start by voice.

- **Capture** (`internal/voice/audio.go`, with `audio_windows.go` and `audio_linux.go`). The
  chosen microphone or the system's default is opened (winmm's waveIn on Windows, PipeWire's
  `pw-record` on Linux) and asked for 16 kHz mono 16-bit audio, which the system converts to.
  It goes into one buffer that stops at ten minutes (9.6 M samples, 19.2 MB). The notch's level is the
  real RMS. The stream is dropped as soon as the key comes up, the cap is reached, the
  device fails or the user cancels. A recording with no 30 ms window above −50 dBFS, or
  under a quarter second, is "Nothing was heard" (`audible`), with Retry.
- **One file** (`wav.go`). Just before transcription the samples are written as one
  WAV in the system temp folder's `hover-voice`. It is deleted when closed, on every
  path. Files a crash left are swept when Hover starts.
- **Speech** (`speech.go`). One interface for both modes: `Speech.Transcribe(wav, cancel)`
  returns a `Transcript` or a `*SpeechError`. It blocks, on voice's worker thread. The
  mode (`VoiceSettings.Speech`) is read once, at the press. Hover never switches modes on
  its own and never uploads a local recording: Local not Ready is an error that says to
  set it up. A transcript the engine says it cut short is shown for review, never
  counted down.
  - Cloud (`groq.go`): the WAV to Groq's OpenAI-compatible
    `/audio/transcriptions` with the user's key and model (`whisper-large-v3-turbo` or
    `whisper-large-v3`). No language is sent, so Groq detects it. Over 25 MB is refused
    before upload. The key goes only in the Authorization header and is scrubbed from
    errors. `HOVER_GROQ_BASE` points it at a fake Groq for measuring; only a
    `http://127.0.0.1:PORT` address is taken.
  - Local (`phonon.go`): below.
- **Cleanup** (`cleanup.go`), optional and separate from the speech mode. The
  transcript (never the audio) goes to the user's OpenAI-compatible service: the Gemini
  or OpenAI preset, or a custom base URL, with the user's key and model. The instruction
  is fixed and the transcript is the user message. Any failure keeps the original and
  the preview says "Cleanup failed; using the original.": no key or model, the 15 s
  timeout, an error, an empty answer, an answer that lost a negation or changed length a
  lot (`suspect`). A transcript over 12,000 characters isn't sent (it is never cut);
  the note then says it was too long.
- **Routing** (`internal/agents/route.go`). Only voice-enabled projects are candidates.
  `Decide` goes by the words first. A name or alias said in full settles it. Among
  several, the active project wins (the chat open in the office, or the new-task box's
  folder, at the press). No project's words at all means the default workspace. Only
  what the words leave open goes to the default agent, in a turn with access `"none"`:
  `AcpHost` and `OpenCodeHost` turn down every request it makes, reads too. It runs in
  an empty temp folder of its own, with a 60 s limit, and gets no desk or chat. Its
  answer is checked (`ReadAnswer`): a project only from the candidates, and its task
  only when it took words off one end and dropped no negation (`TaskOk`). The app, not
  the model, turns the pick into a folder, agent, model and access. An agent that
  doesn't answer is an error with Retry, not a default-workspace pick.
- **Preview and dispatch.** The card shows what was heard, the task (editable), the
  folder, the agent and model, the access, and a note (`Routed.Note`, for example
  "Using default workspace: no clear project match."). The countdown is 3 s on a
  monotonic clock. Every interaction has an id; Cancel moves it on, so late results are
  dropped. The countdown's end, Start and Enter all go through one locked
  `beginStart`, so exactly one of them dispatches. An edit stops the countdown for
  good; the text is routed again once typing pauses (300 ms), and Start is needed. Start
  checks the target again: still registered and voice-enabled, folder usable, access
  unchanged (the default workspace is made here). A change shows the updated card for
  another Start. Then the shell's `voiceStart` (`internal/shell/voice.go`) starts a session
  through `KiroSessions.StartBound(tool, folder, prompt, nil, access, …)`: a new chat every time, even beside another chat in the same
  folder. Started shows only once the tool named the conversation, or the turn ended
  well. A failure keeps the prompt; Retry goes back to the card and nothing is resent on
  its own. Read only on a tool with no read only mode here (Codex on Windows) is refused.
- **The agent.** Always the global default: `Settings.AgentTool()`, the tool picked
  in the new-task circle, with its `AgentOptions` (model and the rest). Project
  settings don't change it. When it isn't available (`agents.Check`), the card asks for
  another for this task only (`ChooseAgent`), and routing and the run both use that pick.
- **Try it** (Settings → Voice) is `Press(true)`: the same mode, cleanup and routing (by
  the words alone when no agent is available), and a preview with Start off. It starts
  no session and makes no folder.

### The voice lifecycle

`Voice` (`internal/voice/voice.go`) owns the state, behind one mutex, outside the renderer.
Its work runs on goroutines of its own (recording, routing, editing, the countdown, the
start). `internal/shell/voice.go` and `internal/ui/voicecard.go` only draw it (the notch's
card, or Try it's card in Settings) and pass keys and clicks on; changes reach the UI
goroutine one hop at a time. One interaction runs at a time: a press while one is in progress keeps it and
flashes busy. Escape cancels the voice interaction only; from Starting on, the task is
the chat's. Nothing resumes after a restart.

| Stage | Means | Set by |
|---|---|---|
| `StageIdle` | Nothing in progress | `Dismiss`; `Retry` after a recording or transcription error |
| `StageRecording` (level, secs) | The shortcut is held | `Press`, then the recording goroutine |
| `StageLoading` | Local: Phonon starts and transcribes (one call) | recording goroutine |
| `StageTranscribing` | Cloud: Groq | recording goroutine |
| `StageCleaning` | The cleanup service | recording goroutine |
| `StageResolving` | Routing | recording goroutine, `ChooseAgent`, `Retry` |
| `StageChooseAgent` | The default agent isn't available | routing, or Start's check |
| `StagePreview` | Counting down; Try it's preview has no countdown | routing |
| `StageEditing` | Stopped for good: an edit, a cut-short transcript, a retry, a changed target | `Edit`, routing, `Retry`, Start's check |
| `StageStarting` | Being dispatched | `beginStart` (countdown, Start, Enter) |
| `StageStarted` (session, folder) | The tool took it | the start goroutine |
| `StageDictated` | Dictation: the words for the chat's reply box (the UI writes them in, then dismisses) | `Dictate` |
| `StageCancelled` | Escape or Cancel | `Cancel` |
| `StageError` (message, retry, transcript) | A stage failed; the transcript is kept when there is one | any worker |

### Local speech (Phonon)

`internal/voice/phonon.go`. Phonon-2 runs in the official `fermion` CLI (Python and CPU PyTorch), so
Hover keeps a Python of its own for it. A system Python is never used or changed. It is
downloaded only when the user presses Download in Settings → Voice, never for Cloud.

- **Pins.** Model `FermionResearch/Phonon-2` at `9c7fef3584499a88fe8d394427f45851bbb8b446`,
  `fermion-research` 0.2.5, CPython 3.12.14 (python-build-standalone 20260929), and the
  wheels per platform in `internal/voice/assets/wheels-<triple>.txt` (made by `make-lock.py`
  beside them, at build time only). Every file has a pinned URL, size and SHA-256.
- **Before any download.** Windows x64 or Linux x64/arm64, SSE4.1 on x64, not Windows on
  Arm, the VC++ 2015–2022 runtime on Windows, glibc 2.28 or newer on Linux. A device that
  fails is `Unsupported` with the reason. Setup also needs the peak disk plus 300 MB free.
- **Install states** (`Install`, with its `InstallKind`): `NotInstalled`, `Unsupported` (with why),
  `Downloading` (done of total), `Verifying` (every file hashed again), `Installing` (unpack the runtime, pip
  from the local hash-checked wheels only, fermion's own verifying unpacker for the
  model, the model files checked against their pins), `Ready`, `Cancelled`,
  `Failed` (with why). `Ready` means the new install turned the bundled sample
  (`internal/voice/assets/check.wav`) into the expected words. A partial download is never
  resumed; a finished one is kept for Retry and hashed again.
- **Where files live.** `<data>/phonon/` (`%APPDATA%\Hover\phonon`,
  `~/.local/share/Hover/phonon`): `downloads/` during setup, `installs/<id>/` (the Python,
  `model/`, `licenses/`, `ready.json`), and `current`, a small file naming the live
  install. Each install is built where it lives and `current` is swapped by an atomic
  rename; folders are never moved (Windows refuses while a virus scanner reads them). So
  a failed or cancelled repair leaves the working install as it was. Folders `current`
  doesn't name are swept at the next setup. `VoiceSettings.local` records the model, its
  version and its folder once Ready.
- **Licences.** `installs/<id>/licenses/` keeps Phonon's `NOTICE`,
  `LICENSE-WEIGHTS-CC-BY-4.0.txt` and `LICENSE-CODE-Apache-2.0.txt`, fetched from the
  model repo at the pinned revision.
- **Each recording** runs `python -I -c <fermion's main> transcribe --json <model> <wav>`
  with structured arguments, offline (`HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`), in a
  process group or job. No server, nothing at login. It exits when done; Cancel kills it,
  and `Shutdown` stops it on exit, when voice is switched off or when Local is left.
  Phonon-2 is English only. Recordings over ten minutes are refused.
- **Remove** is refused while setup runs or a transcription reads the files. It forgets
  the install at once and deletes Hover's own folders on a worker. Local then reads Not
  installed; it never switches to Cloud.

Measured sizes and times are in `evidence/voice-chat/phonon-proto.md`.

## The chat's additions

- **Thoughts.** Reasoning the tool exposes becomes a `KiroStep` of kind `thought`, its text
  in `output`, in order among the tool calls. ACP's `agent_thought_chunk` and OpenCode's
  `reasoning` part feed it (`stream.go`, `opencode.go`). An ACP thought lasts until the
  agent does something else and keeps 256 KB; an OpenCode thought is one reasoning part,
  done when the part ends. Each closes with its measured time and is saved with the
  session. Nothing is made up from the answer.
- **Subagents.** OpenCode's `task` tool becomes a step of kind `agent` (what it was asked,
  its kind, what it found). The chat lists them, four before Show more. ACP tools send no
  child events, so theirs stay ordinary tool steps.
- **Changes and output.** A diff carries `@@ -N +N @@` only when the tool says where it is
  (an ACP call's location line, a new file, OpenCode's own patch). 400 lines of a change or
  of a command's output are kept, and a cut is said. The chat folds them after eight lines.
  An exit code shows only when the tool gave one.
- **Pause, queue and stop.** A reply sent while a turn runs is queued, in order, and each
  has Cancel (`KiroSessions.CancelQueued`). An empty composer during a turn shows Pause
  (`KiroSessions.Pause`): the turn is cancelled through the tool, the conversation stays,
  and the next queued reply goes once, after the tool says the turn has ended. A tool that
  doesn't confirm within 8 s is shut down when no other turn uses it. Otherwise the result
  is `unconfirmed`: the chat says it may still be working, and nothing queued is sent. A
  reply never answers a permission request; it is queued behind it. (An OpenCode question
  that takes free text is answered by a reply.)

## What each provider exposes

What Hover reads from each tool. Kiro, Codex and Cursor speak ACP (`acp.go`, `stream.go`);
OpenCode is its own server (`opencode.go`); Claude Code runs in its SDK mode (`claude.go`).

| | Kiro, Codex, Cursor (ACP) | OpenCode |
|---|---|---|
| Reasoning | `agent_thought_chunk` text as a thought | `reasoning` parts as a thought |
| Child agents | None in ACP: ordinary tool steps | The `task` tool as an `agent` step |
| Cancellation | `session/cancel`; 8 s, then shut down or `unconfirmed` | `POST /session/{id}/abort`, open questions withdrawn; 8 s, then the same |
| Permissions | `session/request_permission`. Kiro: autopilot off to ask. Codex: its mode (`agent-full-access`, `workspace-write` or `read-only`). Cursor: asks unless `--force`; Ask mode for read only | Per-session rules on its server; `once` answers |
| Read only | Kiro, Cursor; Codex on Linux only | Yes: every change asks, and Hover refuses it |
| Usage | Context %: ACP `usage_update` (used / size), Kiro's `_meta.kiro.contextUsage`. Credits per turn: Kiro's `turn_completion` summary | Context %: the last request's tokens over the model's window |
| Diffs | `diff` content (old and new text); line numbers only from the call's location or a new file | Its edit's unified diff, with the file's line numbers |
| Tool output | `rawOutput`, the last 400 lines; exit code when given (`exitCode`, `exit_code`) | The command's output; exit code when given |

Claude Code (`claude.go`) in the same terms: reasoning is its thinking blocks (streamed
as `thinking_delta`) as a thought; a subagent's `Task` call is an `agent` step, and the
subagent's own messages (`parent_tool_use_id` set) stay out of the answer; cancellation is
the `interrupt` control request, then its process ended after 8 s; permissions are its
`can_use_tool` requests (Full is `bypassPermissions`); read only switches off its edit and
command tools; context % is the last answer's tokens over the model's window (the result's
`modelUsage`); diffs are its `structuredPatch` hunks with their line numbers; tool output is
`tool_use_result.stdout`/`stderr`. Checked against Claude Code 2.1.287 through
`fake-anthropic` (a stand-in Anthropic API; it was in the Rust tools, tag `rust-final`); with a real model only by the maintainer, not in CI.

Observed: written down in the code as seen from the real tool. Kiro sends an empty diff
while an edit is pending. Kiro's context and credit payloads are the shapes quoted in
`stream.go`. Codex may send a warning before its answer. codex-acp 1.13 dropped
`workspace-write`. Codex's read-only mode wrote files on Windows (no sandbox there), so
Hover doesn't offer it there.

Not verified: whether each real tool sends reasoning text, which tools send
`usage_update`, OpenCode's `task` and `reasoning` events, the diff line numbers, and an
unconfirmed stop. These are handled in code and tested against fakes only
(`internal/agents/*_test.go`). The voice live smoke used `fake-agent` (in the Rust tools, tag
`rust-final`), not the real tools.

## The office's frames and their lifetime

- The office is made the first time an office is in view (`officeFollow` in
  `internal/shell/office.go`). It runs on its own goroutine (`office.StartLive`, `live.go`).
  The UI sends it the state (at most every 120 ms, only when something changed), the pointer
  and resizes.
- Each frame: the scene is rendered on wgpu-native, read back, and composed on the CPU over
  the page's background and vignette (`page.go`). The UI makes a blurred quarter-size
  copy for the glass panels (`ui.Blur`).
- On both systems the frame is read back and shown as an image in the Gio view. Gio has a
  device of its own, so the office does not share it. The office has its own wgpu device
  (DX12 on Windows, Vulkan or GL on Linux).
- Hidden for 30 s, the office is dropped: its goroutine, renderer and textures go
  (`officeDrop`). Shown again, it is made again at once, with the camera and open chat
  restored.
- Pacing: 30 fps while a bot walks or works, 10 fps idle, 1 fps with animations off,
  nothing while hidden.

## OS adapters

| Concern | Windows | Linux |
|---|---|---|
| Notch window | `platform/win`: borderless, topmost, `WS_EX_NOACTIVATE`; click-through by layered hit mode; DirectComposition for see-through | `platform/wayland`: a layer-shell surface at the top of the display, see-through. No X11 and no XWayland; a compositor with no layer-shell (GNOME's own) can't place it |
| Renderer | Gio on Direct3D 11, one device shared by the windows; the office on wgpu-native (DX12) | Gio on EGL; the office on wgpu-native (Vulkan or GL) |
| Tray, notifications | `platform/win` (Shell_NotifyIcon) | `platform/linux/sni.go` (StatusNotifierItem over D-Bus; notifications over org.freedesktop.Notifications) |
| Shortcut | `RegisterHotKey` | The desktop's GlobalShortcuts portal (KDE, GNOME 48+, Hyprland); elsewhere bind a key to `hover --toggle` |
| Voice hold-to-talk | `RegisterHotKey` for the press, `GetAsyncKeyState` polled for the release | The portal's key down and key up |
| Microphone | winmm waveIn | PipeWire (`pw-record`) |
| Sound out | winmm waveOut | PipeWire (`pw-cat`) |
| Key storage | DPAPI | Secret Service, else a file only the user can read |
| Child processes | Job object (tools die with Hover) | Process group with `Pdeathsig` |
| Single instance | Named mutex `Local\HoverRunningInstance` | Lock file in `$XDG_RUNTIME_DIR` |

Keep OS code in files named for the system (`_windows.go`, `_linux.go`, `_darwin.go`) or behind
a `//go:build` line. What can be worked out without the OS (the Keychain and LaunchAgent logic in
`internal/core/macos.go`, the sandbox's settings text) is in files compiled on every OS, so
tests on any system cover it.

### macOS

The Mac app is not the Gio app (`cmd/hover` on a Mac only says so and exits). It is Swift in
`macos/Sources` (notch, menu bar, Settings, voice) around the web office (`web/office/`,
in a WKWebView), and it starts `hover-backend` (`cmd/hover-backend`) through
`hover-guardian` (`macos/Sources/guardian.c`), speaking JSON lines on stdin and stdout.
`scripts/build-macos.sh` makes `Hover.app` from the three.

| Concern | macOS |
|---|---|
| Notch window | `Notch.swift`: an `NSPanel` (non-activating) at `.statusBar` level (25) on all Spaces whose `canBecomeKey` is false while the notch rests; the hardware notch comes from `safeAreaInsets` and the auxiliary areas (`NotchGeometry`) |
| Renderer | the office is the web page (three.js) in a WKWebView |
| Usage, tray | one status item in the menu bar with the usage rings and one menu (`MenuBar.swift`); usage isn't in the island |
| Shortcut | Carbon `RegisterEventHotKey`, plus a local key monitor for the windows that hold the keyboard (`Hover.swift`) |
| Voice hold-to-talk | the hot key's press and release |
| Microphone | `AVAudioEngine`, then Apple's speech recognizer (`Voice.swift`); the system asks the first time |
| Key storage | the login Keychain (service `dev.hover.history`), read by the Swift app and handed to the backend at `initialize` |
| Child processes | `guardian.c` stops the backend and what is left when the app's pipe closes; the tools lead a process group each with a watchdog (no `PDEATHSIG`) |
| Single instance | none in the Swift app |
| Dark mode | the menu bar item redraws when its `effectiveAppearance` changes |
| Launch at Login | `SMAppService.mainApp` |
| Agent browser | `AgentBrowser.swift`: a WKWebView per session, driven by `browser.go`'s calls, relayed by `internal/backend/browser_host.go` |
| Screen panel | `Screen.swift`: ScreenCaptureKit |

## Where to change things

- **A setting.** Add it to `internal/core/settings.go` (keep the JSON names and
  their order: 2.x reads the same file). Show it in `internal/app/pages.go`, and
  handle its click in `internal/app/view.go` (`Toggled`, `Pressed`, `PickedSeg`, `MenuPick`).
- **Something in the office UI.** `internal/ui/office*.go` for the look;
  `internal/shell/office.go` for what it shows and does.
- **The 3D office.** `internal/office`: `scene.go` (the room), `bot.go`, `office.go`
  (behaviour), `render.go` and `office.wgsl` (drawing).
- **A provider.** Add an `AgentTool` variant in `internal/core/model.go`. Then add its
  executable, arguments, install and sign-in hints, and status check in
  `internal/agents/agents.go`, and its capabilities in `runtime.go`. If it speaks ACP,
  `AcpHost` runs it; map its access modes in `AcpHost.configure` and
  `permission`. Otherwise write a runtime like `opencode.go` and put it behind
  `Runtime`. Add its logo to `internal/ui/marks.go`, its colour to `tools` in
  `internal/shell/office.go`, and a settings page in `pages.go`. Test it against a stand-in
  like `internal/agents/fakeacp_test.go`.
- **A quota.** `internal/quota/read.go` and `quota.go`; the notch item id in
  `settings.go`'s notch items.
