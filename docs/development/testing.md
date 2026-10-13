# Testing

What CI runs, how to run local checks, and which platform scenarios still need a real machine.

## What CI runs

`.github/workflows/ci.yml` does not run the Go test suite. Pull requests run the workflow;
pushes that only change Markdown, `docs/`, or `assets/readme/` are skipped. When CI runs, it:

- checks that the Go code is formatted, vets it for Windows, Linux and macOS, and compiles the
  Mac backend for both Macs;
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

Tests live beside the code (`*_test.go`) with shared fixtures in `tests/golden`. CI currently
does not run them, so run the command for your platform when a change touches tested Go code:

```sh
go test -tags nowayland,nox11,novulkan ./cmd/... ./internal/...    # Linux
go test ./cmd/... ./internal/...                                   # Windows or macOS
```

(`make test`, `.\build.ps1 test`, or `mise run test-<platform>`.) Two layout tests in
`internal/chat` need DejaVu Sans, as the Chromium reference was measured with it. Timing tests
(a task that sleeps, a file that must be written) may be sensitive to a slow or busy machine;
inspect the failure before treating a rerun as evidence of a code issue or a flaky test.

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
