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
| Office | Keep the WGSL shaders on wgpu-native through Hover's own small binding (`go/internal/gpu`) | Gio has no 3D. No C compiler; it ships `wgpu_native.dll`. Same design as now: render, read back, show as an image. `go-webgpu/webgpu` was tried first and dropped (below). |
| Data | A Go build installs over 5.x and keeps everything | `settings.json` stays byte for byte as .NET writes it (port `hover-core/src/json.rs`, not `encoding/json`). Sealed files are nonce + ciphertext + tag, which Go's `crypto/cipher` writes as is. |
| Mac | Keep the Swift app; port `hover-backend` to Go with the same JSON lines | The Swift app doesn't change. |
| Regex | `dlclark/regexp2` where a pattern looks behind or ahead | Go's `regexp` can't; the patterns came from C#, and regexp2 copies .NET's engine. |

Rules for the port:

- No C compiler on Windows: `GOOS=windows go build` works from any machine.
- Port line by line, as 3.0 ported 2.x. Phases 0 to 2 ported each crate's tests with it. From phase 3 on the
  tests are not ported (the owner's call); the Go code is checked by running it: pictures against the Rust
  build's, and `go/tools/app-smoke.ps1` driving the real app on Windows. The tests already written still run.
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

## Phase 1: `internal/core` (done on Windows)

`crates/hover-core` is ported file for file to `go/internal/core`, with its tests (70 pass,
with `-race` on Linux and plainly on Windows). `go/cmd/hover-data` is the Rust `hover-data`,
plus `check`: whether Go would save a data folder's files with the bytes it read.

CI checks it on Windows: the Rust and the Go `hover-data` each write a data folder (a DPAPI
`note.key`, `settings.json`, a sealed session), and each reads the other's. Both write the
same `settings.json` bytes, and Go saves Rust's session and index files unchanged.

Left for later:

- Linux's Secret Service (phase 6). A `note.key` that names a Secret Service item is left
  alone, and there is no history that run. Nothing is replaced.
- The settings portal for Linux's dark mode (phase 6). The desktop's own files are read.
- `platform/macos.rs`, the Keychain and the LaunchAgent (phase 7).
- `encoding/json` isn't used for anything Hover keeps. It writes other bytes than
  System.Text.Json.

## Phase 2: agents, quota, md, diagram (done)

Every Rust crate for this phase is ported, with its tests. 428 Go tests pass, on Linux
(`-race`) and Windows; one skips (it needs a real `kiro-cli`).

- `internal/diagram` and `internal/md`: the golden fixtures, the image rule, and 3,956
  random cases compared with md.js (44 skipped where md.js throws or hangs).
- `internal/agents`: starting tools (a Windows job per tool; a process group and watchdog
  on Linux and macOS), finding each tool and its sign-in, the update stream, questions and
  approvals, the three hosts (ACP for Kiro, Codex, Cursor and Antigravity; OpenCode's server;
  Claude Code's SDK mode) behind `Runtime`, checkpoints, the sandbox, computer use, the
  agent browser's server, Cua Spaces, setup, the GitHub CLI, the terminal tab, chips,
  handoffs, orchestration, folder holds, sessions with their queue, stop, pause, rewind,
  provider switch, fork and Kiro Web reconnect, voice's routing, the Discord status, Open in
  editor, the desk card's backend (`desk.go`) and the office's state message (`state.go`,
  checked byte for byte against `tests/golden/fixtures/office-state.json`).
- `internal/quota`: the four quota readers, the five-minute poller, the daily Kiro usage
  file and Kiro's credits by day.

The Rust tests were matched by name against the Go ones. What has no Go test is only what
belongs to a later phase: the Linux desktop files and portal, the Secret Service and the
Mac's Keychain and LaunchAgent in `hover-core` (phases 6 and 7).

How the Go code differs, on purpose:

- **One package.** The crate's modules call each other in a ring, which Go only allows
  inside one package. Files keep the module names.
- **No drop.** Rust ends a tool when its handle is dropped. Go code calls `Close` or
  `Kill` at the same places, and a cleanup ends the tool should one be missed.
- **HTTP.** Rust wrote its own small client to need no crate. Go uses `net/http`, with no
  proxy, no keep-alive and no compression, as Rust's had none. `internal/quota` uses
  `net/http` as it is, as the Rust quota readers used `ureq`.
- **A .cmd or .bat shim** gets Rust's own safe command line (its `make_bat_command_line`).
  Go would hand cmd the arguments unchecked.
- **Test stand-ins.** The fake gh is the Go test program itself, linked (or on Windows
  copied) under gh's name. Rust built a separate program with rustc.
- **Errors from the system read differently.** Rust says "No such file or directory (os
  error 2)", Go "open x: no such file or directory". Some of these reach the user, for
  example "Couldn't reach OpenCode: …". The words around them are the same.
- **Files that Hover replaces by renaming are read with `core.ReadFile`.** On Windows, Go
  opens a file without letting others rename it; Rust's `std::fs` does let them. A save
  that met a reader lost the save (`agent history: save failed - rename ...index.dat.tmp`,
  seen in two of three Windows CI runs). `ReadFile` opens it the way Rust does. The
  history and the sealed stores use it; two tests in `readfile_windows_test.go` show the
  cause and the fix.
- **A wait for "nothing is running" is `quiet(k)` in the tests.** Between one queued turn
  ending and the next starting, `Running()` is 0 for an instant (a probe saw it 218 times
  in 3 s). The Rust tests wait the same way and have the same gap, but have not met it on
  CI. Only the Go tests were changed.
- **Cursor's sign-in is read only on Windows for now.** It is in an SQLite file. Windows
  calls `winsqlite3.dll`, which ships with it, so nothing is bundled and there is no C
  compiler. Linux (phase 6) needs a reader with no C compiler and no system libsqlite3 (an
  AppImage can't count on one); the Mac (phase 7) has `/usr/lib/libsqlite3.dylib`. Until
  then Cursor's quota says it isn't supported there.

## Phase 3: UI, bottom up (in progress)

Order: `hover-chat` first, then the shared widgets, Settings, the desk card, the chat view,
and the office last. Nothing here opens a window yet; each module draws into a plain image,
so it can be looked at as a PNG and checked on any machine.

### Done: `internal/text`, `internal/raster`, `internal/chat`

- `internal/text` is parley's job: runs with their own font, size and colour, wrapped to a
  width, with line heights, caret and selection positions, and inline gaps (the 5 px
  around inline code). Shaping and line breaking are go-text/typesetting's (already in
  the build through Gio, now a direct dependency).
- `internal/raster` is tiny-skia's and resvg's job: rounded boxes, borders, glows, images,
  glyph shapes (from font outlines, with the Rust painter's quarter-pixel positions and
  14° slant), and a small SVG reader for the chat's own icons, the tools' logos and the
  flowcharts.
- `internal/chat` is `crates/hover-chat`: theme, scroll math (Chromium's thin scrollbar and
  smooth scroll), state parsing, the image cache, the Markdown layout, the thread (turns,
  timeline, commands, thoughts, subagents, answers, changes, buttons), selection and the
  Chromium-style copy, and the painter. All 35 Rust tests are ported and run against the
  same goldens (`tests/golden`).

What differs from the Rust crate, on purpose:

- **Words in Chinese and Japanese.** ICU finds them with a dictionary; the Go side uses
  Unicode's rules with none, so each ideograph is a word. The ported test skips those
  probes and counts them (18 of 3,600). Thai is the same. Already listed under Known costs.
- **go-text's shaper rounds the font size up to a whole pixel** (11.5 px came out 12 px,
  12.5 px came out 13 px). `internal/text` shapes at 32 times the size and scales back.
  Without this the widths of every answer were wrong.
- **Line boxes follow Chromium:** the font's ascent and descent are rounded, and the extra
  height of the line is split with the top half rounded down. That is what made the code
  and table boxes match the page to under half a pixel.
- **Paragraphs break anywhere** (`overflow-wrap: anywhere`), so a table column's smallest
  width is one character, as in the Rust build.
- **Bidirectional text** is shaped and ordered by go-text, but the caret and selection
  boxes are tested for left to right only.
- **A step's "none" is an empty string** (`Name`, `Dir`, `Cmd`, `Diff`, `Out`, `Tag`), not
  an option. A demo step with an empty command is therefore treated as having none.
- **No synthetic bold.** A font with no bold face draws its regular face. Inter ships
  bold, and so do Segoe UI and DejaVu.
- **The broken-image icons** (`broken_image_100.png`, `_200.png`) are copied into
  `go/internal/chat/assets` because `go:embed` cannot reach outside the module.

### Done: `internal/office` (the model; not yet the renderer)

`crates/hover-office` without `render.rs`, `live.rs`, the WGSL shaders and the GPU half of
`page.rs`: the seeded random numbers (the same sequence as the page's, checked against
Node's numbers), the matrices and colours, the room (built in the page's order, since every
jittered box draws from one generator), the bots and their poses, the subagent helpers, the
sessions from Hover's `state` message, the camera, picking, the frame pacing, the bubbles,
and the TV, board, clock and sky pictures on small canvases. The CPU page composition
(`Composer`) is ported too. All 13 of the crate's model tests and its 3 small ones pass.

- The wall canvases use `internal/text` and `internal/raster` for their text, with the two
  fonts the page embeds (copied to `go/internal/office/assets`, as `go:embed` cannot reach
  out of the module).
- The scene and its tests never touch a GPU, so they run on every machine.

### Done: the office renderer and its thread (`render.go`, `live.go`)

`render.rs` and `live.rs` on `internal/gpu`: the same pipelines, shadow map, draw order,
textures and uniforms, and `office.wgsl` as it is (a copy; a test checks it hasn't
drifted). `cmd/office-shot` is the Rust `shot` example: the fixture at its fixed clock,
1104 x 424, night or day.

- **The frame is always read back** and composed on the CPU, the Rust office's Linux path.
  The Rust office on Windows can share the app's wgpu device and keep the frame on the GPU.
  Gio draws with its own device, so that path (its slots, `In::NoGpu`) has no Go version.
- **A GPU error doesn't end the office.** Rust's wgpu panics on one; here it comes back in
  `Out.Error` and the office goes on.
- Looked at here as PNGs on llvmpipe (night and day). CI renders both on Windows' WARP with
  the Go tool and with the Rust example, into the `spike-reports` artifact (`office/`).
  On run 69bc2e5 the two matched except 638 of 468,096 pixels, night and day alike. All of
  them are the small text on the board and the TV. The canvases draw text with
  `internal/raster`, and Rust draws it with swash, so the glyph edges differ.

### Done: `internal/gpu` (Hover's own wgpu-native binding)

About 50 of wgpu-native v29's functions: what the office and the spike draw with, no more.
Windows calls `wgpu_native.dll` through `syscall`; Linux and macOS call the shared library
through goffi. Neither needs a C compiler. Each call copies its Go values into the C
structs, pins them while the call runs and lets go after.

- **Why not `go-webgpu/webgpu` v0.5.5.** Its structs no longer match wgpu-native v29: a
  bind group layout entry is missing `bindingArraySize`, and a vertex attribute and a depth
  attachment are missing `nextInChain`. It refuses a pass with no colour target, which the
  shadow map is. On Linux and macOS it passes the callback structs by pointer, where the C
  side wants them by value.
- **`wire_test.go`** compares every struct's size and every field's offset, and every
  enum value the binding uses, with what a C compiler printed for wgpu-native's own
  `webgpu.h` and `wgpu.h`.
- **wgpu-native's default error handlers panic**, which ends the process when it happens
  inside a C call. Hover sets its own, and `Device.Errors` returns what they caught.
- **On Windows the instance asks for DX12 only**, as the Rust office does.
- **A correction.** Commit 3e52354 blamed goffi for a crash in the Linux test. The fault
  was that first version's own enum values (the WGSL source's struct type was 0x305, not
  2), so wgpu-native read the shader as missing.
- `live_test.go` draws a triangle into a texture and reads the middle pixel back, after a
  depth-only pass. It passed here on Mesa's llvmpipe (Vulkan); CI runs it on Windows' WARP.

### Done: `internal/ui` (widgets, icons, marks, bot)

`widgets.slint`, `icons.slint`, `marks.slint` and `bot.slint` on Gio: the palette, the
Lucide icons, the pill button, switch, segments, slider, ring, tile, text field and hold
button, the tools' marks and live marks, and the bot glyph. Lengths are Slint's logical
pixels.

- **Text follows Slint's layout:** a line is the font's ascent minus its descent tall
  (Inter: 1.21 em), with the baseline at the ascent. Gio shapes the glyphs.
- **What Gio can't draw is drawn on the CPU once and cached as an image:** drop shadows
  (a Gaussian blur), radial gradients and the marks, whose fills need gradients of three or
  more stops and even-odd holes.
- **Slint's software renderer draws no drop shadows** (its `draw_box_shadow` is a TODO),
  so the Rust `--shots` pictures have none. The Go UI draws them, as femtovg does in the
  running app. Shadows are the one expected difference against the shots.
- Looked at here as a headless Gio render (Mesa EGL, `EGL_PLATFORM=surfaceless`).

### Done: Settings as data and on screen (`internal/app/pages.go`, `internal/ui/settings.go`)

`pages.rs` (every section's rows, words and automation ids) with all 20 of its tests, and
`settings.slint` on Gio: the sidebar, the scrolling pane, every row and control, the theme
tiles, Kiro's credits card and MCP servers (list, remove, add and edit form), the picker's
menu. `cmd/ui-shots` renders every section like the Rust `hover --shots`, under the same
names; CI uploads them (`spike-reports`, `ui/`).

- **Gio is patched to blend sRGB-encoded colour** (`go/third_party/README.md`). Unpatched,
  every see-through palette colour came out lighter than Slint draws it. The notch's
  DirectComposition view is now a plain UNORM one to match.
- **Slint's measures, found by comparing shots:** a text line is the font's ascent minus
  descent rounded up to a whole pixel (13 px Inter is 16 px); a pill button is its content
  plus four times its side padding (`widgets.slint` counts it twice); a shortcut pill's own
  min-width replaces that. Text is shaped at 32 times its size (as in `internal/text`):
  Gio's shaper rounded 12.5 px up to 13.
- `app.rs` (the shared state: settings, runtimes, sessions, quotas, credits, the ends
  nobody saw) and `keys.rs` are `internal/app/hover.go` and `keys.go`, with their 5 tests.
  `view.rs`'s handlers (every switch, button, box, segment, picker, theme tile and the
  shortcut recorder) are `internal/app/view.go`. Keys arrive as Gio's key names, not
  Slint's key text.
- Driven here with real pointer and key events through Gio's router: a switch, a segment,
  the shortcut recorder (Ctrl+Shift+K saved), a sidebar section, a picker's menu (opened,
  closed by a click outside, an option picked) and the wheel all reach the settings.
- **Gio's focus tags go under the touch areas.** A focus tag added over a button in the
  same clip took its presses, and one outside any clip took every press in the window.
- Compared here with the Rust shots from Windows CI (run 37900054115): the boxes, rows and
  controls sit on the same pixels. What differs: glyph edges, the title bar (not ported
  yet), the drop shadows (the Rust software renderer draws none), the marks with
  gradients (it fills them flat), and a few wrapped lines below the fold.

Not run here: this sandbox has no display, GPU or Windows fonts. The chat was looked at as
PNGs (the failed and the rich sessions of the office-state fixture), with DejaVu Sans, Noto
Sans and DejaVu Sans Mono. On Windows the width-based goldens skip, as in the Rust tests.

### Done: the live Windows shell (`internal/platform/win`, `internal/shell`, `cmd/hover`)

`hoverai.exe` starts: the notch window at the top centre, its island and the question's card,
the app window with Hover's own title bar, Settings in both, the tray icon with its menu and
balloons, `Alt+N`, and a second launch that opens the app window. Built with no C compiler:
`go build -ldflags "-H=windowsgui" ./cmd/hover`.

- `internal/platform/win` is `win.rs` plus what winit did for Slint: windows that Gio draws into
  (one shared Direct3D 11 device; the notch's swap chain goes through DirectComposition for its
  per-pixel alpha, the app window's is an ordinary one), the message loop with `UIDo` for other
  goroutines, mouse, wheel (a notch scrolls 60 logical pixels, as Slint's winit does), keys and
  text into Gio events, the clipboard, the notch's click-through and focus rules, the shortcut,
  the tray, and the system's file dialogs.
- `internal/shell` is `main.rs` and `notch.rs`: the notch's state, the island (`app/rest.go` is
  `rest.rs`), the card, the app window, Settings' events, the shortcut warning, and Cua and
  setup in Settings → Integrations. It never touches Win32: `Env` is what each OS fills in.
- `internal/ui` has the notch (`notchview.go`), the app window's title bar and menus
  (`dashboard.go`, `barmenu.go`), the Settings overlay (`overlay.go`) and the warning dialog.
- **The office view is a stand-in** (`ui/placeholder.go`: a title, and a Settings button) until it
  is ported. It is not in the Rust app and goes when the office lands.
- Looked at here as PNGs from the real shell (`cmd/ui-shots`: `notch-rest-pill|working|ask|card|
  done-2x`, the Rust shots' names). CI starts the exe on a Windows runner, presses `Alt+N` and
  Esc, launches it twice, and uploads pictures and `hover.log` (`app-windows` artifact).
- **Gio drops a focus request for a key area no frame has shown.** The open notch asks for the
  keyboard on its first frame, when it is still 0 px wide; the ask now waits for the area.

### Done: the desk card's pictures (`internal/ui/deskrows.go`, `deskpanel.go`, `deskcard.go`)

`desk.slint` and office.slint's `DeskCard` on Gio, next to the rows already laid out in
`internal/app/desk.go`: all 29 row kinds, the tabs, the windowed list with its thin bar, the
terminal (prompt, history, Ctrl+L), the open file's bar and editor, Open in, the browser bar,
the screen, GitHub's setup and the pull request form, and the card (header chips, the status
box with its question buttons, the eight tiles, the reply box, the chat button).
`cmd/ui-shots` draws them from fixtures (`desk-card-working.png`, `desk-tab-files.png`).

Left for the desk: its glue (`desk_ui.rs`'s second half: `impl App`, about 1,200 lines) needs the
office page's state, so it goes in with the office view. Two things are stand-ins until then: row
kind 25 (the pull request's description as a picture of Markdown) and the Screen tab's picture.

### Left in phase 3

In order: the office view (`office.slint`, about 3,300 lines, with `office_ui.rs`, 2,700) and
the desk's glue; the chat view; `music.rs` (the office's beats), `net.rs` (web images in
chats), `screen.rs` (the Screen panel's capture), `aura.rs` (voice's aura); the `--shots` run
and the `Alt+N` self-test of the product. Then phases 4 to 7 as listed above.

## Known costs

- Memory, above.
- Gio is before 1.0, so upgrades can break code. It's pinned in `go/go.mod`.
- Double-click word select in Thai, Chinese and Japanese: Rust uses ICU's dictionaries,
  and no Go package with them was found.
- Semi-transparent colour over the desktop: Gio blends in linear light and the swap chain
  wants premultiplied sRGB. Black and its shadow are exact. A light colour at partial alpha
  over the desktop (the rim) comes out slightly bright. Phase 3 checks it on screen.
