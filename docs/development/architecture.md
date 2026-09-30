# Architecture

Hover is one Rust workspace in `native/`. One binary (`hover`) holds the whole
product. The crates keep the parts that have no window apart from the parts that do,
so most of the logic builds and tests anywhere.

## Crates

| Crate | What it owns | Window? |
|---|---|---|
| `crates/hover-core` | The data folder (`paths.rs`, the old `Noty` move), `settings.json` (`settings.rs`, the exact bytes 2.x wrote), the key and encryption (`crypto.rs`: AES-GCM, key kept by DPAPI or the Secret Service), the sealed session history (`history.rs`), images, the single-instance lock (`single.rs`), colours and VS Code themes (`palette.rs`), and the OS adapters (`platform/windows.rs`, `platform/linux.rs`). | No |
| `crates/hover-agents` | Running the agents: the ACP host (`acp.rs`, JSON-RPC over stdio for Kiro, Codex and Cursor), OpenCode's local server (`opencode.rs` over `http.rs`), the `Runtime` both sit behind (`runtime.rs`), the sessions and their limits (`session.rs`), permission questions (`ask.rs`), the office's state message (`state.rs`), process groups and Windows jobs (`proc.rs`). | No |
| `crates/hover-quota` | The four quota readers (Claude Code, Kiro, Codex, Cursor) and their five-minute schedule. | No |
| `crates/hover-md`, `crates/hover-diagram` | Markdown and Mermaid flowcharts, the same output as 2.x's `md.js` and `diagram.js`. | No |
| `crates/hover-chat` | The chat thread: layout per message (cached), selection, copy, images, and a CPU painter. | No |
| `crates/hover-notch` | The notch's geometry, animation and hover rules. | No |
| `crates/hover-office` | The office: scene, bots, wall canvases, camera, picking and pacing (`office.rs`, `scene.rs`, `bot.rs`), the wgpu renderer (`render.rs`, `office.wgsl`), the page's background and vignette (`page.rs`), and its own thread (`live.rs`). | No (renders offscreen) |
| `apps/hover` | The product: `main.rs` (windows, renderer, timers), `office_ui.rs` (the office UI around the scene), `view.rs` and `pages.rs` (Settings), `notch.rs` with `win.rs` / `x11.rs` (placing, focus, click-through), tray (`sni.rs` on Linux, `win.rs` on Windows), `music.rs`, `bench.rs` (the measurement channel), `shots.rs`, `selftest.rs`, and the Slint UI in `ui/*.slint`. | Yes |
| `apps/chat-proto`, `apps/notch-proto` | Prototypes from the port. Not shipped. | Yes |
| `tools/hover-measure` | Dev tools, not shipped: the external memory sampler, the scenario runner, the summary, and `fake-agent`. See [profiling.md](profiling.md). | No |

## Boundaries

- **UI thread.** Slint's event loop runs everything in `apps/hover`. Other threads
  reach it only through `ui_do` (`main.rs`), which posts a closure to the loop.
- **Runtime.** `hover_app::app::Hover` (`app.rs`) is the shared state: settings,
  history, one `Runtime` per tool, the sessions, the quota poller. It has no UI; views
  register hooks (`on_sessions`, `on_quotas`, `on_notify`) that fire off the UI thread.
- **Sessions.** `KiroSessions` keeps every session behind one lock. Each turn runs on
  its own thread. `changed` and `ended` fire with the lock released. At most three
  turns run at once (`MAX_RUNNING`); six sessions keep desks (`MAX_KEPT`).
- **Storage.** `AgentHistory` writes the index and one sealed file per session off the
  UI thread, in order. `Hover::shutdown` flushes the history and settings on quit.
- **Views.** The notch and the dashboard window each have their own Slint globals.
  `office_ui.rs` and `view.rs` push the same state into both (`each!`, `publish!`,
  `show_page!`).

## An agent task, end to end

1. The new-task box (`office.slint`) calls `Office.new-go-clicked`.
2. `office_ui.rs` calls `KiroSessions::start_as` with the tool, folder, prompt, images
   and the access picked.
3. `session.rs` gives the session a desk, saves it, and starts a turn thread that calls
   the tool's `RunTask`.
4. For Kiro, Codex and Cursor the runner is `AcpHost::run_as` (`acp.rs`): it starts
   the tool once (`kiro-cli acp …`, `codex-acp`, `cursor-agent acp`), sends
   `session/new` or `session/load`, sets model, effort and access, then
   `session/prompt`. For OpenCode it is `opencode.rs`: one `opencode serve` on
   127.0.0.1 with a password made for that start, `prompt_async`, and its event stream.
5. Updates (`session/update`, OpenCode events) become `KiroEvent`s and phases. The
   session changes, `changed` fires, and the UI marks the office dirty.
6. A permission request (`session/request_permission`) is answered off the read loop:
   `ask.rs` decides what the access setting allows. The rest goes to
   `KiroSessions::ask`, which shows it in the notch, over the bot and in the chat until
   the user answers or the run stops.
7. The turn ends: the result is saved, `ended` fires, and the notch shows the end (and
   a system notification when no office is in view).

## The office's frames and their lifetime

- The office is made the first time an office is in view (`office_follow`). It runs on
  its own thread (`hover_office::live`). The UI sends it the state (at most every
  120 ms, only when something changed), the pointer and resizes.
- Each frame: the scene is rendered on wgpu, read back, and composed on the CPU over
  the page's background and vignette (`page.rs`). The UI makes a blurred quarter-size
  copy for the glass panels.
- On Windows the office shares the windows' GPU device (`shared_gpu` in `main.rs`). The
  frame goes into one texture that the windows draw directly, written in place each
  frame (`office_ui.rs`, `upload`). On Linux the office has its own device (Vulkan or
  GL), and the frame reaches Slint (femtovg on OpenGL) as a pixel buffer.
- Hidden for 30 s, the office is dropped: its thread, renderer and textures go, and the
  allocator gives the pages back (`office_drop`). Shown again, it is made again at once,
  with the camera and open chat restored.
- Pacing: 30 fps while a bot walks or works, 10 fps idle, 1 fps with animations off,
  nothing while hidden.

## OS adapters

| Concern | Windows | Linux |
|---|---|---|
| Notch window | `win.rs`: borderless, topmost, `WS_EX_NOACTIVATE`; click-through by layered hit mode | `x11.rs`: override-redirect dock window, ARGB visual, XShape input region; Wayland through XWayland |
| Renderer | femtovg on wgpu (DX12, DirectComposition), one shared device | femtovg on OpenGL; office on its own wgpu device |
| Tray, notifications | `win.rs` (Shell_NotifyIcon) | `sni.rs` (StatusNotifierItem over D-Bus) |
| Shortcut | `RegisterHotKey` | `XGrabKey` |
| Key storage | DPAPI | Secret Service, else a file only the user can read |
| Child processes | Job object (tools die with Hover) | Process group with `PR_SET_PDEATHSIG` |
| Single instance | Named mutex `Local\HoverRunningInstance` | Lock file in `$XDG_RUNTIME_DIR` |
| Allocator | mimalloc (freed pages go back to Windows) | glibc malloc, `malloc_trim` after the office drops |

Keep OS code behind `cfg(windows)` or `cfg(target_os = "linux")` in these files.
Check `cfg(not(windows))` branches carefully: they are the Linux path.

## Where to change things

- **A setting.** Add it to `hover-core/src/settings.rs` (keep the JSON names and
  their order: 2.x reads the same file). Show it in `apps/hover/src/pages.rs`, and
  handle its click in `view.rs` (`toggled`, `pressed`, `picked_seg`, `menu_pick`).
- **Something in the office UI.** `ui/office.slint` for the look;
  `office_ui.rs` (`wire_office`, `office_widgets`) for what it shows and does.
- **The 3D office.** `hover-office`: `scene.rs` (the room), `bot.rs`, `office.rs`
  (behaviour), `render.rs` and `office.wgsl` (drawing).
- **A provider.** Add an `AgentTool` variant in `hover-core/src/model.rs`. Then add its
  executable, arguments, install and sign-in hints, and status check in
  `hover-agents/src/agents.rs`, and its capabilities in `runtime.rs`. If it speaks ACP,
  `AcpHost` runs it; map its access modes in `AcpHost::configure` and
  `permission`. Otherwise write a runtime like `opencode.rs` and put it behind
  `Runtime`. Add its logo to `ui/marks.slint`, its colour to `TOOLS` in
  `office_ui.rs`, and a settings page in `pages.rs`. Test it against `fake-agent`
  (`tools/hover-measure`) and a fake host like `tests/acp_host.rs`.
- **A quota.** `hover-quota/src/read.rs` and `lib.rs`; the notch item id in
  `settings.rs`'s notch items.
