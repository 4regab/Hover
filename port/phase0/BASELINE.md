# Phase 0 baseline: the current Hover

This is the reference that the Rust candidate is compared against. Every later
measurement and screenshot names the snapshot it was taken from.

## Source snapshot

| Item | Value |
|---|---|
| Repository | 4regab/Hover |
| Commit | `59bac1f450222bbc15922d6fdc7886a7b24ea43d` ("Make the Agent office Hover's only page", 2026-09-28) |
| Working tree when captured | clean, `main` up to date with `origin/main` |
| Uncommitted removals | none were present in this checkout. Planner, Calendar, FocusTimer, Images and Launcher are gone in the commit itself. What remains of them is intentional: `OwlApp.DropPlanner` (deletes `planner.dat*` on first run), unused icon names in `Icons.cs`, and the 1.x E2E suite. |
| Branch for port work | `rust-port/phase-0-1`, cut from the commit above. It changes no file under `src/`, `web/` or `tests/`. |
| `kiro-office.html` (built page) | sha256 `a84b11f2ff0058614a8c23a036e205f11c3ab0c2014cf055619a5407c9ad0218` |
| `office-beats.ogg` | sha256 `e57ba821d4db8d66783ca984a2778ec68d1439e862239ec36407344ef3bd1eab` |

The Windows workflow (`.github/workflows/native.yml`) rebuilds the C# app from this
commit and records `git rev-parse HEAD`, the SDK and the runner image in its artifacts.

## Dependencies and build settings (C#)

- .NET 10 SDK; target `net10.0-windows10.0.17763.0`; WPF + WinForms (NotifyIcon and Screen only).
- `ConcurrentGarbageCollection=false`, `System.GC.ConserveMemory=5`.
- Packages: `Microsoft.Web.WebView2 1.0.4191.47`, `Microsoft.Data.Sqlite 10.0.12`.
- Runtime dependency: the Evergreen WebView2 Runtime (not installed by the installer; the office shows a card when it is missing).
- Publish (`build.ps1 publish`): `-r win-x64 --self-contained -p:PublishSingleFile=true -p:PublishReadyToRun=true -p:IncludeNativeLibrariesForSelfExtract=true`, no trimming.
- Installer: Inno Setup 6/7, per-user (`{localappdata}\Programs\Hover`), optional Run-key task.
- Office page: `web/office` built with esbuild 0.24.0 and three 0.170.0 (`npm ci`, `node build.mjs`).
- Tests: NUnit 4.2.2 (`tests/Hover.Tests`); `tests/Hover.E2E` still targets the 1.x workspace.

## Environments

| Environment | OS | CPU / RAM | GPU / driver | Used for |
|---|---|---|---|---|
| Development VM (this session) | Amazon Linux 2023, kernel 6.1.186, x86_64 | 8 vCPU, 31 GB | none (no display, no GPU) | portable code, golden tests, headless software-rendered screenshots |
| GitHub Actions `windows-latest` | recorded per run in `env.json` | recorded per run | none: D3D12 runs on WARP (Microsoft Basic Render Driver) | builds, unit tests, notch self-test, indicative memory numbers |
| Real Windows desktop | **pending** | pending | pending | the benchmark of record and every visual/interaction gate |

Toolchains in the VM: Rust 1.92.0, Node 22.23.2, .NET SDK 8.0.423 and 9.0.316 (no .NET 10, so the
C# app is built and tested only on Windows CI).

A VM without a GPU cannot establish performance. Numbers from CI runners are labelled
*indicative* and never used to pass the performance gate.
