# Testing

What CI runs, what you can run yourself, and what nothing runs yet.

## What CI runs

`.github/workflows/ci.yml` does not run the Go tests. On every push and pull request it:

- checks that the Go code is formatted, vets it for Windows, Linux and macOS, and compiles the
  Mac backend for both Macs;
- on Windows, builds `hoverai.exe` and its installer, **drives the app** (starts it, opens the
  notch with Alt+N, folds it with Esc, starts it again to ask for the app window) and **installs
  the new setup over the 5.0.2 release**, checks what the installer promises and uninstalls;
- builds the Linux `.deb` and tarball (compiled, not run: the runner has no Wayland desktop);
- builds the Mac app and runs its **packaged backend** against stand-in tools.

By hand (Run workflow), the `pictures` job draws every view on the Windows runner.

## The Windows checks

- `tools/app-smoke.ps1 -Exe publish\hoverai.exe -Out out\app` starts the real app, presses its
  shortcut, folds it, makes a second launch, and writes `report.json`: did each step happen,
  and its private memory and working set at rest and after the app window opened. It exits 1
  when a step didn't happen.
- `tools/installer-check.ps1` installs the 5.0.2 setup, then the new one over it, and checks:
  one product, the new exe in the same folder, `wgpu_native.dll` and the license installed,
  one uninstall entry, the Start Menu entry, the Run key kept, `%APPDATA%\Hover` untouched, and
  a clean uninstall.
- `tools/set-resolution.ps1` sets the runner's screen to 1920 x 1080 first, so the open notch
  fits.

## The Go tests

They live beside the code (`*_test.go`) with their fixtures in `tests/golden`. Nobody keeps
them green any more and CI does not run them; run them when you want to know what changed:

```sh
go test -tags nowayland,nox11,novulkan ./cmd/... ./internal/...    # Linux
go test ./cmd/... ./internal/...                                   # Windows
```

(`make test`, `.\build.ps1 test`.) Two layout tests in `internal/chat` need DejaVu Sans, as the
Chromium reference was measured with it. Timing tests (a task that sleeps, a file that must be
written) can fail on a slow or busy machine; run them again before you look for a bug.

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

## Not checked by anything

Real agents (Kiro, Codex, Cursor, OpenCode, Claude Code) with approvals, a real microphone
(Groq and local speech), a second monitor at 150%, the pull request tab with real GitHub, and
the Linux window on a real compositor. `docs/development/go-port.md` has the list of what to
try by hand on Windows.
