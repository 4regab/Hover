# Working in Hover

Start here when changing Hover. Use the linked documents for full product behavior and
platform-specific procedures; update those documents when behavior or commands change.

## Project map

Hover is a Go desktop app on Linux. Windows uses a C# (WinUI 3) shell in `windows/`, and macOS a
Swift shell and web office, both around the Go backend (`cmd/hover-backend`). The Go app that was the
Windows build is kept for now as `build.ps1 legacy`. The Rust implementation is available at the
`rust-final` tag; .NET 2.x is in the `dotnet` branch.

| Change | Start here |
|---|---|
| Agent protocols, sessions, permissions, checkpoints | `internal/agents/` |
| Backend commands and events (the macOS and Windows hosts) | `internal/backend/`, `cmd/hover-backend/` |
| Settings, history, secrets, projects, platform paths | `internal/core/` |
| Go UI, chat, office, desk cards (Linux, and the legacy Windows app) | `internal/ui/`, `internal/chat/`, `internal/office/` |
| Windowing, tray, shortcuts, app lifecycle (Linux, and the legacy Windows app) | `internal/platform/`, `internal/shell/`, `internal/app/` |
| Windows app: notch, tray, office, chat, Settings, voice, the host of the backend | `windows/` (`Hover`, `Hover.Host`, `Hover.Notch`, `Hover.Office`, `Hover.Chat`, `Hover.Settings`, `Hover.Voice`), [windows-csharp.md](docs/development/windows-csharp.md) |
| macOS windows, menu bar, voice, native host | `macos/Sources/` |
| macOS office page and bridge | `web/office/`, `macos/Sources/OfficeHost.swift` |
| Build and packaging | `build.ps1`, `Makefile`, `scripts/`, `packaging/` (`packaging/windows/Hover.iss` packs the C# publish folder) |
| Stand-ins, golden fixtures, smoke and E2E flows | `tests/`, `tools/` |

The large `internal/agents` package intentionally keeps closely-coupled agent modules
together. Prefer small changes within the package; introduce a package boundary only when it
removes a real dependency problem.

## Source of truth

- Product behavior and platform differences: [README.md](README.md), especially its feature
  and platform sections.
- Architecture and package ownership: [docs/development/architecture.md](docs/development/architecture.md).
- Build prerequisites and commands: [Build from source](README.md#build-from-source),
  [Windows](docs/development/windows.md) (what is where in the C# app:
  [windows-csharp.md](docs/development/windows-csharp.md)), and [macOS](docs/MACOS.md).
- What CI and local checks cover: [docs/development/testing.md](docs/development/testing.md).
- Performance measurements: [docs/development/profiling.md](docs/development/profiling.md).
- Contribution workflow and release rules: [CONTRIBUTING.md](CONTRIBUTING.md).
- Go port notes are historical: [docs/history/go-port.md](docs/history/go-port.md).

`mise.toml` is an optional convenience for selecting Go and Node versions and invoking common
commands. Native prerequisites remain platform-specific; the existing build scripts are the
authoritative implementation.

## Important invariants

- Preserve existing user data formats. `settings.json` intentionally matches the legacy bytes;
  unknown old settings are skipped. History is encrypted. Never replace an unreadable history
  key or index with a fresh one.
- Agent tools are separate child processes. Keep prompts off command lines, preserve permission
  checks, and ensure child processes are stopped with Hover (the Windows host puts the backend in
  a job object that kills it, and so its tree, with the app). Treat protocol and provider input
  as untrusted data.
- Settings and feature support differ by OS. Check the existing platform adapters and explain
  unsupported features to users instead of implying they work everywhere. Linux is the Gio app;
  Windows has a C# UI and macOS a Swift UI, both on the Go backend.
- The backend speaks JSON lines to its host: Swift on macOS, C# on Windows. Linux uses the Go
  app directly. Keep changes to that bridge compatible with both hosts' message handling
  (`macos/Sources`, `windows/Hover.Host/Protocol`); `tests/macos/backend-smoke.py` and
  `windows/Hover.Tests` run it against the real backend.
- Only one normal Hover instance runs per user. For local development, use a separate empty
  `HOVER_DATA_DIR` so a build cannot alter personal settings or history.
- `VERSION` is not always the next release version: CI can publish automatic nightly versions
  without changing it. Follow [CONTRIBUTING.md](CONTRIBUTING.md#releases) before editing it.
- The Go office renderer loads wgpu-native as a separate library, on Linux and in the legacy Windows
  build; its version and runtime lookup are documented in the platform build instructions. The Windows
  C# app draws the office with Direct3D 11 (`windows/Hover.Office`) and the Mac the web office.
- The Windows output is `hoverai.exe` in `Programs\Hover` (Discord's game list matches `hover/hover.exe`),
  with `hover-backend.exe` beside it. Keep the exe name and the single-instance mutex
  `Local\HoverRunningInstance`.

## Build and check

Run commands from the repository root. See the linked testing guide for what CI covers and
what needs a real desktop.

```powershell
.\build.ps1 run             # the C# app and hover-backend (needs the .NET 10 SDK and Go)
.\build.ps1 test            # the Go tests, then every windows\*.Tests project
.\build.ps1 legacy          # the Go Windows app (needs only Go)
```

```sh
make
make test
```

```sh
scripts/build-macos.sh
go test ./cmd/... ./internal/...
```

For Linux Go commands, use `-tags nowayland,nox11,novulkan` as the Makefile does. Linux builds
need EGL development headers; Windows builds need no C compiler. macOS app builds need Xcode's
command line tools and Node for the web office.

Before handing off a change:

- Format touched Go files with `gofmt` and run the relevant checks from
  [testing.md](docs/development/testing.md).
- For UI changes, render and inspect screenshots; a successful compile does not verify layout.
- For agent or protocol changes, use the stand-ins and fixtures where possible; report any
  real-agent or platform check you could not run.
- Keep generated binaries and package output (`publish/`, `publish-legacy/`, `dist/`, `out/`, `lib/`,
  `hover-linux`) out of commits.

## Repository conventions

- Follow nearby naming and error-handling patterns. Comments should explain why a constraint
  exists, not restate the code.
- Add OS-specific Go implementations in files named for their platform or with a build tag.
  Keep platform-independent parsing and policy logic testable on other systems.
- Preserve line endings defined by `.gitattributes`: Go, shell, Swift, Python, Makefile,
  `VERSION`, and Linux packaging files use LF; most other text uses CRLF.
- Run Go package patterns as `./cmd/... ./internal/...`, not `./...`; the web build can create
  nested files that are not Go packages.
- References to Rust files or crates describe the old implementation. Read them from the tag,
  for example `git show rust-final:crates/hover-core/src/store.rs`.
