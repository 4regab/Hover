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
  where its buttons are (`HOVER_UI_DUMP`) and the test clicks there.
- **Windows only**: DPAPI, the job object (what is in it dies with its handle; its children too),
  killing a host process kills the backend and the agent's child, single instance, the old
  "Hover Service" task removal, and `hoverai.exe` itself (a second launch exits, the first opens
  its dashboard; killing it kills its backend; `--shots` writes the dashboard, the notch's and the panels' JPEGs: the chat view, the start screen, the new-task box, the questions, the menus and the odd inputs).

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
