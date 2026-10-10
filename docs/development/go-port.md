# The Go port

> **Done. The port is finished and the Rust code has been removed.** This page is the history of
> the port; it is not kept up to date. The Go code is now at the repo root (`cmd/`, `internal/`,
> module `github.com/4regab/Hover`) where this page says `go/`. The last of the Rust is the git
> tag `rust-final`: the files named below (`crates/`, `app/`, `tools/notch-proto`) can be read
> with `git show rust-final:<path>`. For how Hover works now, read
> [architecture.md](architecture.md) and [testing.md](testing.md).

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
| 6 | Linux | Native Wayland window (no X11 or XWayland), D-Bus tray, Secret Service, PipeWire audio and screen capture (no PulseAudio). |
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
- **Cursor's sign-in is in an SQLite file.** Windows calls `winsqlite3.dll`, which ships with
  it, so nothing is bundled. Linux and the Mac read the file with a small reader in plain Go
  (`internal/quota/sqlite_reader.go`: tables, overflow pages, the write-ahead log), checked
  against files that SQLite itself wrote.

## Phase 3: UI, bottom up (done)

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
through purego. Neither needs a C compiler of its own (the Linux app has one anyway, for Gio's EGL; purego works with it, where goffi could not link into a cgo build). Each call copies its Go values into the C
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
- The office view that first stood here as a stand-in is the real one now (see the office below).
- Looked at here as PNGs from the real shell (`cmd/ui-shots`: `notch-rest-pill|working|ask|card|
  done-2x`, the Rust shots' names). CI starts the exe on a Windows runner, presses `Alt+N` and
  Esc, launches it twice, and uploads pictures and `hover.log` (`app-windows` artifact).
- **The first text waited 3 s for the system's fonts** (Gio scans them when its shaper is made, and
  the first run on a machine has no cache). On the runner the open notch drew no frames until it
  finished. `ui.Warm()` makes the shaper in the background at start.
- Seen on the Windows runner (`app-windows` artifact): `Alt+N` opens the notch in about 560 ms
  and Esc folds it in about 340 ms, both drawing every frame; the app window draws its title bar,
  menus and caption buttons; a second launch opens it; private memory 130 to 160 MB with the
  stand-in office (the Rust app's number is still to be measured, see Phase 5).
- **Gio drops a focus request for a key area no frame has shown.** The open notch asks for the
  keyboard on its first frame, when it is still 0 px wide; the ask now waits for the area.

### Done: `internal/music` and `internal/audio`

`music.rs`: the Ogg Vorbis loop decoded as it plays (`jfreymuth/oggvorbis`, pure Go), its
fade (16 steps up and 11 down, as the Rust test counts them), the chime, and the system's
audio output: Windows' `waveOut` through `winmm.dll`, no COM and no C compiler. Linux's output
(PipeWire; PulseAudio is legacy and not a target) is phase 6. Checked here: the loop decodes at 22.05 kHz and comes round again;
not checked: sound itself (the runner has no audio device, so its path is "no output").

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

### The rest of phase 3, as it stands

Done and on `go-port`: the office view and its drawer, the side panels (board, overview,
history), the chat view with its session list and start screen, the desk card and its panel
(glue and all), the Screen panel on Windows, the aura, the music and chime, and `net.rs`'s
web images in chats (`shell/net.go`). `hoverai --shots DIR` draws every view
(`internal/shots`; `cmd/ui-shots` is the same program without the product).

Not done, on purpose or for lack of a way to check it:

- **Pictures: all 384 are drawn.** `hoverai --shots DIR` makes a Go twin of every picture the
  Rust build makes, with the same name and size: each view at each office size (small,
  default, large, extra large), narrow and 2x. They were compared with the Rust build's by eye
  (side by side, Rust above Go), group by group: voice 82, Settings 88, the chat 58, the desk 71,
  the office 33, the expanded chat 13, the chat view 30, the notch 8, the new-task box 1. A pixel
  score is no help for the office pictures (the room is drawn at another time of day, and the
  glass blurs it through), and fonts differ between Linux and the Windows runner, so those were
  left alone. What the comparing found, all fixed: the voice card's menu and task box shrank
  in the wrong order; the desk card's top row squeezed its chips differently; its answer box
  was two lines tall, not four; the helpers line sat on the permission buttons; desk buttons were
  24 px narrow; the file editor and the pull request description started their words in the
  middle of the box; the access menu wrapped its notes early; a chat title that fit was cut with
  letter spacing; the repository search box did not show what was typed; the chat header's
  branch icon was not drawn; and the new-task box did not start in the default project folder.
  The line spacing of wrapped text was 16 px at 12.5 px size, where Slint gives 15.1: that moved
  every paragraph, and 47 of the voice and chat pictures (23 and 24) got closer to Rust's from
  that one change.
- **Known picture differences, not bugs.** The time of day of the room and what its bots say;
  the clock and "took N s" in fixtures; fonts; the Screen tab, which this Linux machine
  cannot draw (the Windows CI run's picture is the one to look at); a focus ring where no real keyboard focus is; and the
  Rust build's own shots of the terminal, where its harness never ran the typed command (Go
  runs it in a real shell, as the scenario says).
- **`bench.rs`** (the Rust build's profiling harness, 390 lines) is not ported. It is for
  tuning the Rust build and has no part in the product.
- **`selftest.rs`** drives X11, which is not a target; on Windows the Rust product has none
  either (`notch-proto --selftest` is its check). `hoverai --selftest` says so.

## Phase 4: voice (done on Windows)

`crates/hover-app/src/voice` and `voice_ui.rs` as `internal/voice` (the flow: hold to talk,
Groq or local speech, cleanup, routing, the preview and its countdown, dictation into a
chat's reply box, screenshots by voice) and the notch's card for every stage
(`internal/ui/voicecard.go`, `shell/voice.go`). Phonon (local speech) downloads, verifies and
runs its helper as in Rust.

- The microphone on Windows is winmm's `waveIn` (16 kHz mono, no COM, no C compiler). Names
  are matched by their first 31 characters, which is all winmm gives.
- Checked here as pictures against the Rust build's (the preview, listening with its aura,
  editing, the long menu), and by the Windows CI run. Not checked: a real microphone, Groq,
  or Phonon's speech.
- `Text` that is cut short with "…" now loses its last letters, not its last word, as Slint's
  does (one line of text, everywhere).

## Phase 5: shipping Windows (done)

- The exe has Hover's icon (`cmd/hover/rsrc_windows_amd64.syso`, made with `go-winres`).
- `packaging/windows/Hover.iss` also carries `wgpu_native.dll` when it is beside the exe.
- CI (`installer-windows`) builds the setup, installs it over the 5.0.2 release the way a
  user would, and checks: the exe is replaced in the same folder, one uninstall entry, the
  Start Menu entry, the Run key kept, `%APPDATA%\Hover` untouched, and a clean uninstall.
  It passes. The same job keeps the setup as an artifact.
- **Memory (the rule above).** Measured by `memory-windows` (run by hand) with the same
  script on both apps, at rest: Go **69.8 MB** private, Rust **34.7 MB**: **2.01 times**,
  against the rule's 1.25. The working sets are alike (42.7 MB against 44.6 MB); the extra is
  committed memory the Go runtime and Direct3D hold. The rule says the port stops here and we
  decide again. **Decided: accepted** (the owner's call), so the port goes on.

## Phase 6: Linux (written and run against stand-ins; not run on a real desktop)

Wayland only (no X11, no XWayland) and PipeWire (no PulseAudio). The binary is a cgo build
(Gio's EGL): `go build -tags nowayland,nox11,novulkan ./cmd/hover`. Everything else is Go:

- **Window** (`internal/platform/wayland`): the wire protocol spoken directly; the notch is
  a layer-shell surface at the top of the primary display, taking the pointer only where
  the notch needs it (Wayland shows a program the pointer only over its own surface, so the
  hover zone is the surface's input region); the app window is an xdg toplevel with Hover's
  own title bar. Frames are rendered off screen by Gio and handed over in shared memory.
  Keys go through libxkbcommon (called with purego). A desktop without layer-shell (GNOME's
  Mutter) cannot place the notch; Hover says so and stops.
- **Shortcuts**: the GlobalShortcuts portal (KDE, GNOME 48+, Hyprland). Elsewhere (Sway),
  bind a key to `hover --toggle`.
- **Tray and notifications**: StatusNotifierItem with its menu, and the notification service.
- **Secret Service** keeps `note.key`'s key (the file with mode 0600 where there is none);
  the **settings portal** says dark or light, and tells when it changes.
- **Sound**: `pw-cat` plays and `pw-record` records (PipeWire's own tools; no PulseAudio).
- **Pickers, clipboard, screenshots**: the FileChooser portal (else zenity or kdialog),
  `wl-copy` and `wl-paste`, the Screenshot portal (else `grim`).
- **wgpu** is called through purego (goffi could not link into a cgo build).
- **Dead keys** (libxkbcommon's compose tables), **fractional scaling** (`wp_fractional_scale`
  with `wp_viewporter`: the buffer is the size times the scale, shown at the logical size) and
  **input methods** (`zwp_text_input_v3`: the text being composed is real text in the box,
  underlined, and secret fields say they are passwords and are never sent to the input method)
  are in. Run against the stand-in compositor only, so Chinese, Japanese and Korean input
  through a real IME is unchecked.
- **Not there**: the Screen panel (a Wayland program may not see another's windows), and a
  primary display (the first the compositor lists, or `HOVER_OUTPUT`).
- **Packages**: `packaging/linux/package-linux-go.sh` makes a `.deb` and a tarball, and CI
  builds them.

What was run here: the audio against a real PipeWire; the Secret Service, the portal, the
tray, the shortcuts and the pickers against stand-ins on a real D-Bus; the SQLite reader
against SQLite's own files; the GPU tests on Mesa's llvmpipe with cgo on; and the whole app
against a stand-in compositor built from the Wayland protocol files, which decodes each
request by the types the protocol declares. All 79 opcodes the window code uses were checked
against those files. No real compositor, GPU, speaker or microphone was available.

## Phase 7: the Mac backend (done; run on a Linux machine and, in CI, on a Mac)

`cmd/hover-backend` replaces `crates/hover-backend`: the same JSON lines on stdin and stdout
(`internal/backend`, one file for each of the Rust ones). The Swift app is unchanged.

- `go/tools/backend-diff/run.py` starts the Rust backend and the Go one with the same scripted
  sessions (the protocol tests, with the stand-in tools and `gh`) and compares every message
  field by field: 69 messages, and the only differences are a few timing or random ones, which
  it lists. CI runs it on Linux and on a Mac.
- **The Mac platform layer** (`internal/core/macos.go`, `platform_darwin.go`, `keychain_darwin.go`,
  and the quota readers' Keychain): the data folder under `~/Library/Application Support`, the
  Keychain item names, the LaunchAgent and the `defaults` reads, as `platform/macos.rs` has
  them. The Keychain is called through purego (Security.framework; no C compiler). CI
  (`core-macos`) runs it on a real Mac: the data folder, its own key, four reads across Rust
  and Go, and the same `settings.json` bytes.
- **The Mac build uses the Go backend.** `scripts/build-macos.sh` builds `go/cmd/hover-backend`
  (`CGO_ENABLED=0`, arm64 or amd64); `ci.yml`'s macOS job and `app-macos` run the Swift app's
  smoke test on both backends. The Rust crates stay in the repo. `AGENTS.md` and `docs/MACOS.md`
  still describe the Rust backend: they are the release's words, left for when the Rust one is
  dropped.
- The smoke test's check for "exactly one MCP server" was stale (the app offers two,
  `cua-driver` and `hover-browser`); it fails the same way on the Rust backend, and is fixed.
  A stop that lands during shutdown can leave a history stage unset; `run.py` ignores that.

## Check on Windows by hand

What CI runs: the Go tests, the installer over 5.0.2, the app driven by `app-smoke.ps1`, the
pictures on WARP (no GPU), and, when started by hand, the memory run. What it cannot do is use
a real machine. Before the Go build ships, someone should:

1. Install the setup from the `installer-windows` artifact over a 5.x install with real data;
   open Hover and check the history, the keys and the settings are all still there.
2. Press Alt+N, hover the notch, click it: the office opens and folds without the notch
   stealing focus from the window behind it. Do it with the taskbar at the top and on a second
   monitor at 150%.
3. Tray menu: open the app window, Settings, Quit. Start Hover a second time: the first one's
   window opens.
4. Run a real agent each in Kiro, Codex, Cursor, OpenCode and Claude Code, with Ask first:
   approve, deny and trust one; stop one; reply while it works.
5. A chat's panels: the terminal (run a command, Ctrl+C), the files (edit and save one), the
   diff, the pull request (set up `gh`, open one), the screen.
6. Voice with a real microphone: Groq, then local speech (Phonon download and a spoken task),
   and dictation into a reply box.
7. Memory at rest, about 70 MB, with Task Manager's "Memory (private working set)".
8. Uninstall, and check `%APPDATA%\Hover` is left alone.

## Known costs

- Memory, above.
- Gio is before 1.0, so upgrades can break code. It's pinned in `go/go.mod`.
- Double-click word select in Thai, Chinese and Japanese: Rust uses ICU's dictionaries,
  and no Go package with them was found. Accepted as a cost.
- Two tests compare text widths with a Chromium reference measured in DejaVu Sans
  (`internal/chat`: the double and triple clicks, and the scrolling boxes); they fail on a
  Linux machine without that font and pass on the Windows runner.
- `internal/app`'s notification test waits on a clock, and the Windows runner once stalled for
  4 s under load. The wait is 10 s now. The settings write test had the same trouble and was
  fixed the same way.
- `go/hover` (a 2.3 MB Linux program) has been tracked in git since an early commit. It should
  be removed and ignored; that is a change to the repository, left for you.
- Semi-transparent colour over the desktop: Gio blends in linear light and the swap chain
  wants premultiplied sRGB. Black and its shadow are exact. A light colour at partial alpha
  over the desktop (the rim) comes out slightly bright. Phase 3 checks it on screen.
