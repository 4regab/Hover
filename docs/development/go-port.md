# The Go port

Hover is moving from Rust to Go. The Go code lives in `go/` on the `go-port` branch. The
Rust app stays on `main` and keeps shipping until the Go one matches it. Windows comes
first.

## Why, and the one cost

The reasons: slow builds and the MSVC toolchain, Slint's limits, Rust being hard to work
in, memory and speed, and others.

Go fixes the first three. Memory is the exception: Go's garbage collector keeps more
memory than Rust with mimalloc. Phase 0 measures how much on the same runner. The proposed
rule is that the Go notch at rest stays within 25% of `tools/notch-proto`'s private
commit. If it doesn't, the port stops there and we decide again.

## Decisions

| | Decision | Why |
|---|---|---|
| UI | Gio (`gioui.org`, pinned) | Draw-it-yourself, like Hover's UI. Pure Go on Windows. Its `gpu` package draws into a target we own. Fyne needs a C compiler and has its own look. |
| Notch window | Our own Win32 window, not Gio's | Gio's windows can't be see-through, and Gio takes focus when it opens one. We use DirectComposition with premultiplied alpha, as notch-proto does through wgpu. |
| Office | Keep the WGSL shaders on wgpu-native through `go-webgpu/webgpu` | Gio has no 3D. No C compiler; it ships `wgpu_native.dll`. Same design as now: render, read back, show as an image. |
| Data | A Go build installs over 5.x and keeps everything | `settings.json` stays byte for byte as .NET writes it (port `hover-core/src/json.rs`, not `encoding/json`). Sealed files are nonce + ciphertext + tag, which Go's `crypto/cipher` writes as is. |
| Mac | Keep the Swift app; port `hover-backend` to Go with the same JSON lines | The Swift app doesn't change. |
| Regex | `dlclark/regexp2` where a pattern looks behind or ahead | Go's `regexp` can't; the patterns came from C#, and regexp2 copies .NET's engine. |

Rules for the port:

- No C compiler on Windows: `GOOS=windows go build` works from any machine.
- Port line by line, as 3.0 ported 2.x. Port each crate's tests with it; they prove the Go code behaves the same.
- Same exe name (`hoverai.exe`), single-instance mutex and installer (`packaging/windows/Hover.iss`).

## Layout

```
go/cmd/hover/              the app (Windows, then Linux)
go/cmd/hover-backend/      the Mac app's backend
go/cmd/notch-spike/        phase 0
go/internal/core/          hover-core
go/internal/agents/        hover-agents
go/internal/quota/         hover-quota
go/internal/md/, diagram/  hover-md, hover-diagram
go/internal/notch/         hover-notch (done)
go/internal/office/        hover-office
go/internal/ui/            notch, office UI, desk card, chat, Settings
go/internal/voice/         capture, Groq, cleanup, Phonon
go/internal/platform/      Windows, then Linux
go/tools/                  fake-agent, fake-opencode, fake-anthropic, measure
```

## Phases

| Phase | What | Done when |
|---|---|---|
| 0 | `notch-spike`: the notch on Gio in a DirectComposition window, the office stand-in on wgpu-native | Every notch-proto self-test check that passes in Rust passes in Go on the same runner. `office.wgsl` and `page.wgsl` compile. Memory is within the rule above. |
| 1 | `internal/core` | Opens a real 5.x data folder. `settings.json` saves byte for byte. History and `secrets.dat` decrypt. |
| 2 | agents, quota, md, diagram | Ported tests and `tests/golden` pass against the fake agents. |
| 3 | UI: notch, office, desk card and chat, Settings | Screenshots of each view next to the Rust build's. |
| 4 | Voice | Records, Groq, cleanup, Phonon reads `check.wav`. |
| 5 | Ship Windows | Installs over 5.x and keeps the data. |
| 6 | Linux | X11 window, D-Bus tray, Secret Service. |
| 7 | Mac | `cmd/hover-backend` replaces the Rust one. |

## Phase 0: the notch spike

`.github/workflows/go-port.yml` runs on every push to `go-port`. It builds the spike with
no C compiler, runs `notch-spike --selftest`, then builds and runs `notch-proto --selftest`
on the same runner. The run summary has a table of both reports and their private commit.
The job fails when Go fails a check that Rust passes.

To run it on a Windows PC, put `wgpu_native.dll` (wgpu-native v29.0.0.0) next to the exe:

```powershell
cd go
go build -o ..\spike\notch-spike.exe .\cmd\notch-spike
..\spike\notch-spike.exe                     # Alt+N, hover the top centre, Esc
..\spike\notch-spike.exe --selftest out --wgsl ..\crates\hover-office\src
```

Things the spike leaves out on purpose (each marked `ponytail:` in the code): the drop
shadow, Inter and system fonts, and display changes.

### Results so far (run 4, windows-2022, no GPU)

- All 16 Go checks pass: notch-proto's 14, plus the office stand-in and both WGSL files.
- Private commit at rest after the run: 52 to 61 MiB over runs 2 to 4. Of that, the Go
  runtime holds about 29 MiB (`go_sys`) and the Go heap about 12 MiB; the rest is
  Direct3D on WARP, wgpu-native and the DLLs.
- No Rust number yet. `notch-proto --selftest` panics at `main.rs:498` on the runner:
  `hwnd_of` finds no window right after `show()`, so `n.hwnd` is `None`. The memory rule
  above can't be checked until that is fixed or another Rust baseline is chosen.

Where the Go self-test differs from notch-proto's, and why:

- **Click on the resting pill.** notch-proto moves the pointer and clicks 30 ms apart on
  the window's own thread, so the 50 ms poll can't run in between. Go's log showed the
  window still click-through at the click, and the click went to the window underneath.
  The Go test moves first and clicks 70 ms later, before the 120 ms hover delay. Rust
  likely has the same problem, since its test sleeps on the same thread. Not confirmed:
  it crashes before this step.
- **The pill's colour.** notch-proto reads it at 12 dp, where the status text crosses the
  centre (Go read a letter there). The Go test reads 28 dp, still inside the pill.

## Known costs

- Memory, above.
- Gio is before 1.0, so upgrades can break code. It's pinned in `go/go.mod`.
- Double-click word select in Thai, Chinese and Japanese: Rust uses ICU's dictionaries,
  and no Go package with them was found.
- Semi-transparent colour over the desktop: Gio blends in linear light and the swap chain
  wants premultiplied sRGB. Black and its shadow are exact. A light colour at partial alpha
  over the desktop (the rim) comes out slightly bright. Phase 3 checks it on screen.
