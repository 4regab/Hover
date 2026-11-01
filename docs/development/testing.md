# Testing

What CI runs, how to run local checks, and which platform scenarios still need a real machine.

## What CI runs

`.github/workflows/ci.yml` runs the Go test suite on Linux, Windows and macOS, then the
platform builds below. Pull requests run the workflow; pushes that only change Markdown,
`docs/`, or `assets/readme/` are skipped. When CI runs, it:

- checks that the Go code is formatted, vets it for Windows, Linux and macOS, and compiles the
  Mac backend for both Macs;
- runs `go test` on Linux (with `-tags nowayland,nox11,novulkan`), Windows and macOS.
  Linux and macOS also pass `-race`. The Windows run includes the headless chat shots,
  which are compiled only there, and skips the race detector: that needs a C compiler, and
  the Windows build has none. Those shots draw the office with the same `wgpu_native.dll`
  the Windows build downloads (`WGPU_NATIVE_PATH`);
- on Windows, builds `hoverai.exe` and its installer, **drives the app** (starts it, opens the
  notch with Alt+N, folds it with Esc, starts it again to ask for the app window) and **installs
  the new setup over the oldest Go release still published**, checks what the installer promises and uninstalls;
- builds the Linux `.deb` and tarball (compiled, not run: the runner has no Wayland desktop);
- builds the macOS app and runs its **packaged backend** against stand-in tools. This checks
  the backend process and protocol, not the native Swift UI or its permission prompts.

By hand (Run workflow), the `pictures` job draws every view on the Windows runner.

## The Windows checks

- `tools/app-smoke.ps1 -Exe publish\hoverai.exe -Out out\app` starts the real app, presses its
  shortcut, folds it, makes a second launch, and writes `report.json`: did each step happen,
  and its private memory and working set at rest and after the app window opened. It exits 1
  when a step didn't happen.
- `tools/installer-check.ps1` installs that older setup, then the new one over it, and checks:
  one product, the new exe in the same folder, `wgpu_native.dll` and the license installed,
  one uninstall entry, the Start Menu entry, the Run key kept, `%APPDATA%\Hover` untouched, and
  a clean uninstall.
- `tools/set-resolution.ps1` sets the runner's screen to 1920 x 1080 first, so the open notch
  fits.

## Local Go tests

Tests live beside the code (`*_test.go`) with shared fixtures in `tests/golden`. Run the
command for your platform. CI runs these, and adds `-race` on Linux and macOS:

```sh
go test -tags nowayland,nox11,novulkan ./cmd/... ./internal/...    # Linux
go test ./cmd/... ./internal/...                                   # Windows or macOS
```

(`make test`, `.\build.ps1 test`, or `mise run test-<platform>`.) The chat layout tests
compare text widths with a Chromium reference measured in DejaVu Sans and DejaVu Sans Mono.
Those faces (DejaVu 2.37) are in `internal/chat/testdata/fonts`, and the tests use them
instead of the fonts installed on the machine. Timing tests (a task that sleeps, a file that
must be written) may be sensitive to a slow or busy machine; inspect the failure before
treating a rerun as evidence of a code issue or a flaky test.

## Pictures

`hoverai --shots DIR` draws every view headless and writes the PNGs. They are for looking at:
a green build doesn't prove the pixels are right.

- The office, desk, chat and expanded-chat pictures need `HOVER_SHOTS_OFFICE=1` and
  wgpu-native (`WGPU_NATIVE_PATH`, or beside the program).
- `HOVER_SHOTS_SKIP=voice,chat` leaves those groups out, to draw the rest sooner.
- On Linux the program needs `-tags shots,nowayland,nox11,novulkan`, cgo and Mesa's EGL; run it
  with `EGL_PLATFORM=surfaceless`.

## macOS

- `tests/macos/backend-smoke.py <Hover.app> <sandbox>` runs the packaged backend against a stand-in
  agent in a disposable folder (CI does this).
- `tests/macos/e2e/run.sh <Hover.app>` is the longer run: the real office, bridge and browser
  with stand-in agent, `gh`, `cua` and `lume`, against a local test site. Not run in CI.
- `tests/macos/SettingsSmoke.swift` is Settings' own smoke test.

## Manual platform checks

Real agents (Kiro, Codex, Cursor, OpenCode, Claude Code) with approvals, a real microphone
(Groq and local speech), a second monitor at 150%, the pull request tab with real GitHub, the
native macOS UI and permissions, and the Linux window on a real compositor are not exercised
by the routine CI workflow. The longer macOS stand-in E2E is documented above and can be run
on a Mac.

## The C# Windows app

`.github/workflows/windows-csharp-dev.yml` runs on every push to `windows-csharp` (Phase 6
folds it into `ci.yml`). On `windows-2022` it:

- builds `hover-backend.exe`, `hover-data.exe` and `gofixture.exe` from the Go code;
- makes a Go data folder (`hover-data write`, then `dump` and `check`) and keeps it in the same
  job, because `note.key` is bound to the Windows user and cannot be carried to another machine;
- builds `windows\Hover.Windows.slnx`;
- runs `dotnet test` on every `windows\*.Tests\*.csproj` it finds, so a new test project needs no
  workflow change;
- runs `hoverai.exe --shots` with an empty data folder;
- uploads everything under `out\` as one artifact (`windows-csharp-<sha>`): test results (`.trx`),
  build and app logs, pictures.

What `windows\Hover.Tests` covers:

- **The backend flows of `tests/macos/backend-smoke.py`**, run by the C# host against the real
  `hover-backend` and `Hover.TestTools` in place of Codex: settings, a task with its permission
  question and a reply, the desk panels, sealed history and a wrong key, a restart, stop, and the
  host's end closing the backend's stdin. Computer use is a Mac feature and is not part of them.
- **The line client's inputs**: bad JSON, bytes that are not UTF-8, a line over the limit (and
  the first line past it), a very large line under the limit, an unknown message type, a known
  type with fields of the wrong kind, CRLF, a handler that throws, and a backend that exits in
  the middle of a command.
- **Data compatibility**: the Go-written folder opened by C# (the key through DPAPI, the sealed
  history and a secret read by test code of its own, then the backend started by the host), and the
  C#-written folder read by `hover-data dump` and `check` and `gofixture`.
- **The notch's rules against Go** (`NotchGoldenTests`): sizes, placement, easing, every frame of
  the shape, the outline text, the hit region at 108 points, the zones, the resting shape's spring,
  opening and closing, every poll of the pointer rules, the island's clock and "took" text, the
  quota numbers, the tray menu, and the virtual key and label of every key. The inputs and the Go
  answers are in `windows\Hover.Tests\golden\notch.json`, written by `go test ./internal/app -run
  TestNotchGolden` (with `HOVER_UPDATE_GOLDEN=1`); the same test fails in Go when the file is out of
  date. `NotchControllerTests` run the whole behaviour (peek, fold, click-away, shortcut, displays
  changing) against a desktop that is not there; `IslandTests` the island for no sessions, work, a
  question, an end nobody saw, and a very long text; `SvgPathTests` the outline and mark readers.
- **The real notch** (`NotchAppTests`, Windows, needs `HOVER_APP_EXE`): the window is where the
  rules put it and has the right styles; the app never becomes the foreground window while the notch
  shows, peeks or folds; the pointer goes to the window only over the shape (`WindowFromPoint` and
  the style bit); Alt+N, sent as keys, opens and folds it and an open notch holds the keyboard; a
  shortcut another program holds is reported (error 1409) and the app goes on; the tray icon is
  added, and Quit in its menu, picked with the keyboard, stops the backend with exit code 0 and
  removes the icon; closing the dashboard hides it and a second launch brings it back.
- **The panels** (`PanelTests`, any system): the start screen starts a task with the tool, folder, access
  and model its pills show; the agent, where-it-runs, repository and access menus; a permission answered
  (`allow`, `trust`, `deny`) from the chat and over the bot; two questions at once answered one by one; a
  question with choices; a reply sent, an empty box pausing the run, a reply queued then sent now or taken
  back into the box; a checkpoint restored only after the confirmation; selected text copied; a chat
  renamed; a 10,000-line answer, a huge change, a broken flowchart and a picture that fails to load; Escape
  keeping the draft. `ChatThreadTests` and `ChatBackendTests` (a real `hover-backend` and the stand-in
  agent: what `state` carries, the rendering commands, queue, pause, stop, rename, checkpoints) cover the thread.
- **The whole flow in the real app** (`PanelsAppTests`, Windows, needs `HOVER_APP_EXE`): with real mouse
  and keyboard input and the stand-in agent, a task is started from the chat view's new-task box (agent
  and access picked from their menus), its permission is answered in the chat, a reply's permission is
  answered from the notch's card, the first turn's checkpoint is restored (the second turn goes) and its
  message tried again, and a reply queued behind a run that never ends goes after Pause. The app writes
  where its buttons are (`HOVER_UI_DUMP`) and the test clicks there. Each of these tests first moves the app's
  window to the screen's top left (`Input.Park`): Windows places every new window a little further down and right,
  and on the runner's 1024-pixel screen a click on the right of a window that hangs over the edge is moved to the
  edge and lands elsewhere.
- **The desk** (any system): `DeskTests` (the card and its tiles, the tabs' rows, the terminal, a file opened, edited
  and refused, a binary file, a file that went, the diff with its numbers and a 50,000-line diff cut at its budget,
  agents, the pull request tab with and without the GitHub CLI, Open in, the cloud chip, the card's keys) and
  `DeskReplyTests` (the stop square, a queue held after a stop, pictures pasted and refused, `@` and `/` lists, chips
  and a chip the backend refuses, the model menu, the board, the history, the Kiro Web and Kiro CLI lists and adopt)
  with a made-up backend; `DeskBackendTests` against a real `hover-backend` (the folder's data, the shell in the
  folder and a command that never ends, saving and the refusal of a path outside the folder, Open in, chips, a 40 MB
  picture, the GitHub CLI) — on Windows the shell is PowerShell.
- **The desk in the real app** (`DeskAppTests`, Windows, needs `HOVER_APP_EXE`): a click on the desk opens its card,
  the terminal runs a command and shows its output, a file is opened, edited and saved (the file on disk changes),
  the diff shows the change, a picture is pasted (Ctrl+V of a clipboard bitmap) and a file attached with `@` and both
  reach the backend and the agent, and a running turn is stopped with the square; the pull request tab with `gh`
  hidden from the app (`PATH` and the Program Files folders) offers the setup, and with `gh` not signed in the sign-in
  starts and is cancelled. The Install button is never clicked: it would really install `gh`.
- **Voice, sound and music** (`windows\Hover.Voice.Tests`, `dotnet test`; needs `HOVER_BACKEND`):
  - *The whole voice flow through the real backend*, with a fake microphone that hands over 50 ms blocks, a fake Groq
    on the loopback (`HOVER_GROQ_BASE`, the only address the backend takes) and the stand-in agent: press, audio blocks,
    release, heard, preview with its countdown, and the task started on the agent (the prompt is what was heard; Groq was
    sent a WAV of exactly the audio and the key as a bearer, and the key is in no message back); toggle mode; an edit
    and Start now; Esc during the countdown (nothing starts); release before any audio ("Nothing was heard", the device
    let go); Groq refusing the key (its words, no retry, the key not shown); no key at all; a missing microphone sent as
    `voiceAudio {error}` and recovering on the next press; a microphone that fails while recording; a press while
    one is in progress; dictation; Phonon not installed and the Groq key check.
  - *The card*: what each stage shows (the Go card's words), the menus and what a pick sends (agent, model and effort that
    keep each other, folder, Kiro Web repository with its search and the 15-second ask), the keys, the amber glow, the
    screenshot chime, flash and note; the 82 pictures have the Go pictures' names (`GoVoicePictures.txt` is the Go list);
    on Windows each is drawn on a software Direct3D device and kept as JPEG under `out\voice`, the card has the Go sizes,
    never grows past the window, and its buttons are where the actions say; odd inputs (5,000-character text, empty
    fields, 300 agents, twelve screenshots).
  - *The shortcut* (Windows): Ctrl+Alt+Space sent with `keybd_event` arrives as a hotkey, holds, and letting go of the key
    (or of Alt first) releases; toggle mode ends on the second press; a chord another program holds is refused with the
    Go app's words; an unmappable key; the chord is free again once let go. `VoiceAppTests` (needs `HOVER_APP_EXE`) do it
    in the real app: with no microphone the card says "No microphone is connected." and Esc clears it; a chord
    another program holds is said on the card at start.
  - *Microphone names*: the 31-character rule, the first device wins, devices that did not answer never match; the
    runner has no input device and gives the Go app's error text for the default and for a named one.
  - *Sound*: the ramp against the numbers the Go `RampStep` prints; the loop (a linked Ogg Vorbis file, 128 s) decodes,
    never goes silent when it starts again, fades in to 0.32 and out to nothing and lets the device go; hidden office,
    switched on while hidden, no output device and an unreadable loop; the chime; the real output opens or says why not.
- **Windows only**: DPAPI, the job object (what is in it dies with its handle; its children too),
  killing a host process kills the backend and the agent's child, single instance, the old
  "Hover Service" task removal, and `hoverai.exe` itself (a second launch exits, the first opens
  its dashboard; killing it kills its backend; `--shots` writes the dashboard, the notch's and the panels' JPEGs: the chat view, the start screen, the new-task box, the questions, the menus, the odd inputs, the desk card and its tabs, the expanded chat and the office's panels).

Run it by hand (on Windows or, for everything not marked Windows only, on Linux). The tests start
real programs and want these set, as the workflow does:

```powershell
$env:HOVER_BACKEND    = "out\bin\hover-backend.exe"     # go build ./cmd/hover-backend
$env:HOVER_DATA_TOOL  = "out\bin\hover-data.exe"        # go build ./cmd/hover-data
$env:HOVER_GOFIXTURE  = "out\bin\gofixture.exe"         # go build ./windows/tools/gofixture
$env:HOVER_FIXTURE_GO = "$env:TEMP\fixture-go"           # made by: hover-data write <dir> <dir>\project 2 <dir>\answer.md
$env:HOVER_APP_EXE    = "windows\Hover\bin\Release\...\hoverai.exe"   # optional: the app's own checks
dotnet test windows\Hover.Tests
```

A test whose program is missing fails; it is not skipped. On Linux the key is the Go app's plain
file, so the DPAPI parts are skipped and the rest runs.
