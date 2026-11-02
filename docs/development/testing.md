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
- on Windows, builds the C# app and its backend with `build.ps1` (`dotnet publish`, self-contained, and
  `go build ./cmd/hover-backend`), **runs every `windows\*.Tests` project** (see
  [The C# Windows app](#the-c-windows-app)), **drives the app** (starts it, opens the notch with Alt+N, folds
  it with Esc, asks for the app window with a second launch, quits from the tray menu, and kills a second copy to
  see its backend go), draws its pictures (`hoverai.exe --shots`), builds the installer and **installs
  the new setup four ways** (over the oldest Go release still published while that app runs, into a clean default
  folder, twice into a folder with spaces in its name (the second time while the app runs), and over a 2.x install), each time checking what the
  installer promises and uninstalling; runs `build.ps1 legacy` in Windows PowerShell 5.1 (`powershell.exe`; every other step uses PowerShell 7), and checks that
  the Go Windows app still builds (`build.ps1 legacy installer`);
- builds the Linux `.deb` and tarball (compiled, not run: the runner has no Wayland desktop);
- builds the macOS app and runs its **packaged backend** against stand-in tools. This checks
  the backend process and protocol, not the native Swift UI or its permission prompts.

By hand (Run workflow), the `pictures` job draws every view of the Go app on the Windows runner, and the
C# app's views with `hoverai.exe --shots`.

## The Windows checks

- `tools/app-smoke.ps1 -Exe publish\hoverai.exe -Out out\app` starts the real C# app with an empty
  `HOVER_DATA_DIR` and checks, from `hover.log`, the processes and the windows: the backend is a child of
  the app, the notch and the tray icon come up, the shortcut is registered, Alt+N opens the notch and Esc
  folds it (the shortcut again if Esc did not; the report says which), a second launch ends and the first
  opens its window, and Quit in the tray menu (opened with the tray message, picked with the keyboard) ends
  the app with exit code 0 and takes the backend and the icon with it. A second copy is then killed and its
  backend must go too (the job object). It writes `report.json`: each step, and the private memory and
  working set of `hoverai.exe` and of `hover-backend.exe`, at rest (two seconds after everything is up, as
  the Go app was measured), settled (ten seconds) and with the app window open. There is no screen capture:
  what the panels drew is read from `HOVER_UI_DUMP` (`ui-dump-*.json`), the pictures come from `--shots`.
  It exits 1 when a step didn't happen.
- `tools/installer-check.ps1` runs four installs, each from a clean state, and writes `report.json`:
  - **A, over 5.x:** the oldest Go setup still published goes in, its app starts, then the new setup is
    installed over it while it runs. The old app must be stopped, `hoverai.exe` replaced, `hover-backend.exe`
    and every file of the publish folder (but the `.pdb` files) installed, `wgpu_native.dll` removed, one
    uninstall entry, the Start Menu entry there, the Run value and `%APPDATA%\Hover` untouched, `--version`
    right. The installed app starts, and an uninstall while it runs stops it and leaves nothing.
  - **B, a clean install** into the default folder (`%LOCALAPPDATA%\Programs\Hover`) with the startup task:
    the Run value is written, the installed app is driven by `app-smoke.ps1`, then the uninstall leaves
    no file, no Run value, no Start Menu entry and no uninstall entry, and keeps the data folder.
  - **C, a folder with spaces** in its name (`/DIR=`), installed twice: the app runs from there when the
    second install of the same version comes, which stops it, changes no file and leaves one uninstall entry;
    the app starts again; the desktop icon goes with the uninstall.
  - **D, over 2.x:** a `Hover.exe` and a Run value that starts it are there before the setup runs (without the
    startup task): `Hover.exe` is removed, the Run value now starts `hoverai.exe`, and the uninstall removes
    the value although the setup did not write it.
- `tools/set-resolution.ps1` sets the runner's screen to 1920 x 1080 first, so the open notch
  fits.

## Local Go tests

Tests live beside the code (`*_test.go`) with shared fixtures in `tests/golden`. The C# tests are
under [The C# Windows app](#the-c-windows-app). Run the
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
a green build doesn't prove the pixels are right. (On Windows that is the legacy Go app,
`build.ps1 legacy`; the C# `hoverai.exe --shots DIR` writes JPEGs no wider than 1400 px, drawn inside
the app, and the job's picture step checks the names and that each is a JPEG.)

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

The `windows` job of `.github/workflows/ci.yml` runs on `windows-2022` and uses `build.ps1`, so a
local `.\build.ps1 test` does the same as CI. `.\build.ps1 test` (or `test -SkipGo`, which CI uses because
the `test` job already runs the Go tests on three systems):

- builds `hover-backend.exe` and `hover-data.exe` from the Go code into `out\bin`;
- makes a Go data folder (`hover-data write`, then `dump` and `check`) and keeps it in the same
  run, because `note.key` is bound to the Windows user and cannot be carried to another machine;
- builds `windows\Hover.Windows.slnx`, then publishes the app, so the app's own checks start the same
  `publish\hoverai.exe` the installer ships, with `hover-backend.exe` beside it;
- runs `dotnet test` on every `windows\*.Tests\*.csproj` it finds, so a new test project needs no
  change to the script or the workflow, and writes `out\test-results\*.trx`.

The job then drives the app and the installer (above), draws the app's pictures with an empty data folder
(`hoverai.exe --shots`, in five minutes at most, and checks that the views the Go app has pictures of are there as JPEG;
among them the first-task note, the long model and repository lists, and the new-task bar at 828 x 346 and 880 x 480:
`office-notice`, `office-model-menu-long`, `office-fab-cloud-repos`, `office-fab-open-long-model` and their `-small` and `-min` twins)
and keeps everything under `out\` (but the Go programs the tests built) as the artifact `windows-reports`.
The job fails if any C# test project fails.

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
  C#-written folder read by `hover-data dump`, `check` and `secret check`. The secret the Go side keeps is
  written with `hover-data secret set`.
- **The notch's rules against Go** (`NotchGoldenTests`): sizes, placement, easing, every frame of
  the shape, the outline text, the hit region at 108 points, the zones, the resting shape's spring,
  opening and closing, every poll of the pointer rules, the island's clock and "took" text, the
  quota numbers, the tray menu, and the virtual key and label of every key. The inputs and the Go
  answers are in `windows\Hover.Tests\golden\notch.json`, written by `go test ./internal/app -run
  TestNotchGolden` (with `HOVER_UPDATE_GOLDEN=1`); the same test fails in Go when the file is out of
  date. `NotchControllerTests` run the whole behaviour (peek, fold, click-away, shortcut, displays
  changing) against a desktop that is not there; `IslandTests` the island for no sessions, work, a
  question, an end nobody saw, and a very long text; `PathDataTests` the outline and mark readers.
- **The real notch** (`NotchAppTests`, Windows, needs `HOVER_APP_EXE`): the window is where the
  rules put it and has the right styles; the app never becomes the foreground window while the notch
  shows, peeks or folds; the pointer goes to the window only over the shape (`WindowFromPoint` and
  the style bit); Alt+N, sent as keys, opens and folds it and an open notch holds the keyboard; a
  shortcut another program holds is reported (error 1409) and the app goes on; the tray icon is
  added, and Quit in its menu, picked with the keyboard, stops the backend with exit code 0 and
  removes the icon; closing the dashboard hides it and a second launch brings it back.
- **The panels on the screen of the open notch** (`NotchScreenTests`, Windows, needs `HOVER_APP_EXE`): the notch is opened and
  folded three times with Alt+N, and each time the test copies the screen (`BitBlt`, the picture Windows composes) over
  the notch and looks for the panels' own colours at the places the dump gives: the lilac dot of the new-task button, the
  lilac icon of the office/chat switch and the menu button's bars. The clicking checks never saw the pixels, and the
  pictures paste the panels onto the office in software. The first round also opens the office menu with a click and
  picks night, day and the clock, which office.json must keep. The copies and `hover.log` are in `out\notch-live`.
- **Lists, the first-task note and the new-task bar** (any system, no window; the cases of v5.0.18's report):
  - `MenuFitTests` opens every menu the panels have (top, agent, where, access, folder, model, repository, the chat's
    more menu, the reply box's model menu, each from the office's box and the start screen or the chat) in four windows
    (828 x 346 the notch of the owner's PC, 840 x 340 the Small notch, 880 x 480 the app window's smallest, 1560 x 600)
    with 40 models, 45 repositories and 30 folders. Each box must be inside the window with 8 px to spare and no taller
    than the window less 16; the wheel must bring every row that can be picked into view, whole and inside the box,
    without the box moving and without the chat behind it scrolling; a model menu keeps its heading, effort levels and
    last row in view; a menu opens on its picked row; the bar can be dragged and a press in its track pages; Up, Down,
    Home, End and Enter pick and Escape closes. It also covers the reply box's `@` list and the desk's "Open file in" menu
    in a short window. `UiTests` (`windows\Hover.Settings.Tests`) does the same for a Settings picker with 41 options.
    On the old code 42 of the 71 cases of the first test failed (a model menu 1458 px tall in a 600 px window).
  - `NoticeTests`: the panels ask for the settings once; the note shows only when the backend's settings say
    `KiroNoticeSeen` is false (a model without the key, or a preferences message that says false, changes nothing); its
    words and Got it are inside the window in the four sizes; under it a press, the wheel and typing do nothing and
    Escape is left to the notch; Got it (click or Enter) sends `saveSettings {noticeSeen: true}`; `setSetting` false (Settings
    > Kiro > Show the note again) brings it back.
  - `TaskBarTests`: at the same sizes and 520 x 420, with a 41-letter model name, a 45-letter folder and a 36-letter
    repository, the pills of the new-task bar stay inside the box, do not run into each other or the send button, the send
    button is 30 (box) or 32 (start screen) wide, and a name that does not fit is cut with an ellipsis. 9 of 11 cases failed
    on the old bar.
  - `StartTaskTests` joins the panels (with a shaper of fixed letters) to a real `hover-backend` and the stand-in agent on
    a data folder that has never run: the backend refuses a task sent too early; the note shows and Got it saves the
    setting (read back with `settings`); an agent, "Ask always" and a folder are picked (Open folder… with the dialog
    answered by the test, and cancelled first); the words are sent by the round button and again with Enter; the agent
    asks, is allowed from the panels, and its answer is in the chat; the second note after `setSetting` false.
- **A first task in the real app** (`FreshStartAppTests`, Windows, needs `HOVER_APP_EXE`): the same flow with real mouse,
  wheel and keyboard on a data folder that has never run and has 30 registered folders. In the app window: the note,
  Got it, the chat view, the folder chip, the wheel to the last of the 30 folders (it is not in view when the list opens),
  the agent and access pills, a task sent by the round button and one by Enter, both allowed and answered. In the open
  notch: the note, Got it, + , the folder pill, a screen copy of the list (it must be inside the notch's window and no
  taller than its office less 16 px), the wheel, the pick. The copies (`notice-open.png`, `folder-list-open.png`,
  `folder-picked.png`) are in `out\notch-live`. Not run: the system's folder dialog itself (a modal window the test cannot
  answer); `StartTaskTests` answers it with a fake.
- **The sweep** (`SweepTests`, any system): the panels are put in 51 states (the office with and without its box, the
  history, board and overview panels, the drawer and the chat view in each stage of a chat, the question and permission
  cards, the desk card and each tab, the note, a confirm, and every menu from `MenuFitTests`) at 828 x 346 (the open
  notch), 880 x 480 (the app window's smallest), 1200 x 720 and 1920 x 1080, with data that is hard on a layout (34 chats
  with long titles, words and paths with no spaces, wide tables and code, right-to-left text, emoji). In each state:
  every place that answers a click is inside the window; no two overlap unless one is wholly inside the other, or is a
  layer put over on purpose; no line of text runs off the window or is cut at its side by a clip; every button looks
  different with the pointer on it (the places that do not by design are named in the test, with the reason); and the
  pointer is the hand over a button, the text cursor over a box of words, the resize cursor on the list's edge. The
  fixture's Markdown is made up, and the shaper has fixed letters, so a width is a rule of thumb: the pictures are what
  to look at. The first run found a few dozen faults (listed in the sweep report, reports/sweep1.md).
  `SweepFixTests` holds the checks of the single faults it found (each failed before its fix): the repository search,
  the folder chip, the Escape order, the keys that must not reach the office, the reply box and the thread, a saved rename,
  drafts kept, Retry, Enter and delete, a question with two parts, the list at the side (folders, fold, delete, resize),
  and sideways scrolling of wide code.
- **The window's smallest size** (`WindowSizeAppTests`, Windows, needs `HOVER_APP_EXE`): the app window answers the
  system's `WM_GETMINMAXINFO` with 880 x 480 scaled by the display, and stays that size when asked for 300 x 200.
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
  agents, the pull request tab with and without the GitHub CLI, Open in, the cloud chip, the card at the place of the
  click with its helpers line, the name tags and the tips, the card's keys) and
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
- **Settings, the Discord status and the Screen capture** (`windows\Hover.Settings.Tests`, `dotnet test`; needs `HOVER_BACKEND`):
  - *The pages* (any system): every page of `internal/app/pages.go` opens from the sidebar with the Go words; the Windows
    rules (Sandbox, Computer use, Agent browser, Agent desktops and Setup shown off with their notes, no Cua rows); each
    kind of control (switch, segments, picker, shortcut, slider, field, tile, link) answers the pointer and the keys on a
    canvas that writes down what it was asked; a row that is switched off draws its text faded (the app's canvas draws the
    colours a text was shaped with).
  - *Round trips through the real backend*: each kind of control is changed, the backend is stopped and started again on the
    same folder, and the setting is still there; `settings.json` passes `hover-data check` and `hover-data dump` opens the
    key the host made (Windows: `note.key` is wrapped for the user). A Groq key saved from the Voice page is in
    `secrets.dat` (`hover-data secret check`), in no message the backend sent and in no line of the log. A project is
    added and made the default; a project whose folder has gone says so and the page still opens; a VS Code theme file
    with comments is applied, and a broken one, an empty one and a missing one say "That file has no VS Code theme colours
    in it." and change nothing. Installed themes are listed (Windows: written into the real profile under a name of their own and removed again) and one is applied as Hover colours; Appearance puts Hover's own back.
  - *Nearby inputs*: a 2,000-character Groq key is kept; a key with a control character is refused under its field; two
    settings changed within 400 ms are both kept; a read-only `settings.json` does not stop the pages; Discord not running
    is not an error.
  - *Discord* (any system): the status client against a fake named-pipe server: the handshake, the `SET_ACTIVITY`
    payloads, the 5 s gap, "Idle" and "N agents working", a ping and a refusal, Discord closing the pipe after the hello, and Discord not running at first and started later.
    `internal/backend/discord_test.go` checks that `initialize {hostDiscord: true}` starts no Go client.
  - *Windows only*: Launch at login writes and removes the Run value with the quoted path of `hoverai.exe` (on a key of the
    test's own); the Screen capture shows only the windows of the processes asked for over the desktop (another process, a
    minimised window, a closed one and one partly off the screen); a folder written by the Go app (`HOVER_FIXTURE_GO`)
    shows on the Kiro page and a change is read back by Go.
  - *The real app* (`SettingsAppTests`, needs `HOVER_APP_EXE`): `hoverai.exe --settings` shows the page in the app
    window and a click on a switch, a click on a segment, the wheel and a typed text change what the backend writes to
    `settings.json`; a shortcut is recorded from the real keyboard; Alt+N opens the notch and its menu opens Settings in
    it, where a click on a switch is written. The places come from `HOVER_UI_DUMP` (`settings-window`, `settings-notch`).
  - *Pictures*: `--shots` writes the `settings-*` and `notch-open-settings*` pictures with the Go names (the Mac-only
    Cua and Setup pictures are not made); the job checks that ten of them are there and are JPEG. `screen-capture.jpg`
    is the Screen capture of the app's own window over the wallpaper on the runner.
  - *The Screen tab* (`windows\Hover.Tests\ScreenPanelTests`, any system): the tile is open with a feed and grey without
    one, and its line says Desktop, Desktop + its apps or Live; the desktop alone is asked for once at rest and shown whole
    in its box; Watch live asks for the agent's processes four times a second and Stop takes the desktop again; an agent
    testing is live without the button; a failure is said under the last picture; the pictures are let go when replaced and
    when the panel shuts; the desktop is taken again when the tab comes back. The wallpaper covers the screen and keeps its
    middle (`WindowsTests`). `desk-tab-screen`, `desk-tab-screen-live` and `desk-tab-small-screen` are drawn with a made-up
    screen.
  - *Dictation*: the line under the reply box (`DictationLineTests`) and the reading of the screenshots said aloud
    (`DictationShotsTests`: a picture in `kiro-images` becomes a data address of its kind; another folder, a name that starts
    like it, a climbing path, a file that is not a picture, an empty one, one over 8 MB and an odd name are not read).
- **Windows only**: DPAPI, the job object (what is in it dies with its handle; its children too),
  killing a host process kills the backend and the agent's child, single instance, the old
  "Hover Service" task removal, and `hoverai.exe` itself (a second launch exits, the first opens
  its dashboard; killing it kills its backend; `--shots` writes the dashboard, the notch's and the panels' JPEGs: the chat view, the start screen, the new-task box, the questions, the menus, the odd inputs, the desk card and its tabs, the expanded chat and the office's panels).

Run one project by hand (on Windows or, for everything not marked Windows only, on Linux). The tests start
real programs and want these set, as `build.ps1 test` does:

```powershell
$env:HOVER_BACKEND    = "out\bin\hover-backend.exe"     # go build ./cmd/hover-backend
$env:HOVER_DATA_TOOL  = "out\bin\hover-data.exe"        # go build ./cmd/hover-data
$env:HOVER_FIXTURE_GO = "$env:TEMP\fixture-go"           # made by: hover-data write <dir> <dir>\project 2 <dir>\answer.md
$env:HOVER_APP_EXE    = "publish\hoverai.exe"            # optional: the app's own checks (.\build.ps1 publish)
dotnet test windows\Hover.Tests
```

`.\build.ps1 test -SkipGo` sets all of these and runs every test project in one go.

A test whose program is missing fails; it is not skipped. On Linux the key is the Go app's plain
file, so the DPAPI parts are skipped and the rest runs.
