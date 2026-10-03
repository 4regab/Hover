# Hover on macOS

Hover 3.4 runs on a Mac as a Swift app (`macos/Sources`: the notch, the menu bar, Settings,
voice, and the office in a WKWebView) on the same Rust backend as Windows and Linux
(`crates/hover-backend`, over `hover-agents`, `hover-core` and `hover-quota`), which the app
starts over JSON lines on stdin and stdout. The office is the web page in `web/office`
(three.js). It is based on Arz's (@Entourage397) macOS v1.0, which had the
same Swift shell around a C# engine; the C# is gone, and the Slint app (`app/`) is Windows
and Linux only.

**Status: it builds, and nobody has run it on a Mac yet.** CI compiles the backend crates on
Apple Silicon (`cargo check`), builds `hover-backend`, and builds and signs (ad hoc)
`Hover.app` from the Swift sources, and uploads it as a workflow artifact; a release puts it on
the release page as a disk image for Apple silicon and one for Intel. CI runs no test
on a Mac. `scripts/test-macos.sh` and `tests/macos/e2e/run.sh` (below) are written to run
one, in the background, and the pure parts of the Rust (the Keychain and LaunchAgent
logic, the sandbox's settings) are tested on Windows.
Everything that needs AppKit, the window server, the Keychain or a
permission prompt has not been run. The last section lists it.

## Build from source

You need macOS 14 or later, Xcode's command line tools (`xcode-select --install`; full Xcode
is not needed) and Rust through [rustup](https://rustup.rs). `rust-toolchain.toml` picks the
version. From the repo root:

```sh
scripts/build-macos.sh                 # dist/macos-osx-arm64/Hover.app (HOVER_ARCH=x64 for Intel)
open dist/macos-osx-arm64/Hover.app
```

The script also needs Node (it bundles the office page from `web/office` with esbuild), builds
`hover-backend` with cargo, compiles the Swift in `macos/Sources` with `swiftc`, and signs the
bundle ad hoc (`HOVER_SIGN_IDENTITY` names a Developer ID instead). A local build is signed
with the bundle id as its requirement, so a rebuild keeps the permissions you granted.
The bundle has an `Info.plist` and an identifier (`dev.hover.desktop`) because macOS asks for
the Microphone, and lists Screen Recording and notifications, only for such an app. It sets
`LSUIElement`, so there is no Dock icon: it is the notch and its menu-bar item (the app does
have a small main menu with Settings, Quit and the Edit commands, which the office's text
boxes need). The bundle's `Info.plist` is written by the build script, with the version from
`Cargo.toml`. Local builds are not notarized; `scripts/package-macos.sh` signs with a
Developer ID (`HOVER_SIGN_IDENTITY`) and notarizes. The release's disk images are signed ad
hoc, so macOS asks once: right-click → Open, or System Settings → Privacy & Security → Open
Anyway.

Tests: `scripts/test-macos.sh` (see the end-to-end section below for its sibling). It builds
in a disposable copy and runs the cargo tests of `hover-backend`, `hover-core`,
`hover-agents`, `hover-quota`, `hover-md` and `hover-diagram` under `sandbox-exec`, then
`tests/macos/backend-smoke.py` against the packaged backend and the app's own
`--smoke-test`. `cargo test --workspace` is not the command: the workspace includes the
Slint app, which refuses to compile on a Mac. The CI job does not run any of them.

Agents (Kiro, Codex, Cursor, OpenCode, Claude Code) are found through the login shell's
`PATH`, read once at start (`$SHELL -ilc`, `ShellEnvironment.swift`), because an app opened
from Finder has only `/usr/bin:/bin:/usr/sbin:/sbin`. `brew`, `~/.local/bin`, `~/.cargo/bin`
and npm's folders work. Settings → Get Started has a **Set Up** button for each agent that
installs what is missing and opens the tool's sign-in in a Terminal window.

## What it looks like

- **The notch.** At rest Hover is the camera housing. While agents work the island grows
  wings either side of it (logos, what the front one is doing, for how long); a question
  drops a card under it. Hovering it (after a short dwell), clicking it or Option-N opens the
  office out of the notch. A Mac without a notch gets a notch-sized pill at the top centre.
  A click on the resting notch never activates Hover or takes focus from your app.
- **Usage is in the menu bar**, not the island: one status item shows, for each reader you
  switch on (Codex, Kiro, Cursor), the tool's logo in a ring of its used share, and the
  percentage (green, amber from 70 %, red from 90 %). It opens one menu: readings, "Show in
  menu bar" switches, agents at work, Open Office, Office in a Window, Start a Voice Task,
  Open on Hover, Launch at Login, Settings, Refresh, Quit.
- **The dashboard** ("Office in a Window", or the office's full-screen button) is an
  ordinary titled window with a transparent title bar, holding the same web office.
- **Settings** is a window with a sidebar: Get Started, General, Usage, Computer Use, Voice
  and a page per agent. Its pages can be opened from a shell:
  `open -a Hover --args --settings <page>` with `start`, `general`, `usage`, `computer-use`,
  `voice`, or an agent (`codex`, `kiro`, `cursor`, `opencode`, `claude`).

## Permissions

| Permission | Who asks | For | When |
|---|---|---|---|
| Microphone | Hover | voice dictation | the first time you hold the voice shortcut |
| Speech Recognition | Hover | voice dictation (Apple's recognizer) | the first time you hold the voice shortcut |
| Screen Recording | Hover | the desk card's Screen tab showing the agent's apps live | the tab's **Allow…**; macOS wants Hover restarted after the grant. Without it the tab shows the wallpaper. |
| Accessibility | CuaDriver | computer use: reading and operating other apps | Settings → Integrations → **Grant access…** |
| Screen Recording | CuaDriver | computer use: seeing other apps' windows | same |
| Keychain | Hover | its history key (service `dev.hover.history`), and reading Claude Code's own entry (`Claude Code-credentials`) for the quota if that reader is on | the first use; allow it |

Computer use's two grants go to **CuaDriver**, not Hover, because CuaDriver is the program
that acts. Hover's hot keys are Carbon hot keys and the pointer is read with
`NSEvent.mouseLocation`, so neither needs Accessibility or Input Monitoring. Notifications
(the end of a task) go through the system's notification centre
(`UNUserNotificationCenter`), when "Notify me when an agent finishes" is on in Settings.

## What is Mac-only

- The notch window and the menu bar items above, and Option-N / Control-Option-Space as
  global hot keys. They are fixed; Settings shows them as text.
- **The agent browser.** Every agent gets a browser tool (open, snapshot, click, type, press,
  scroll, screenshot, evaluate, wait, console, back, reload). It runs in a WKWebView per
  session, shown in the desk card's Browser tab, and talks to the agent over a Unix socket
  only you can use. Off elsewhere: "Agent browser needs macOS."
- **One-click agent setup**, with the makers' own installers. Off elsewhere.
- **Computer use** is built for the Mac (Cua Driver's daemon and its permissions). Off
  elsewhere: "Computer use needs macOS."
- **Agent desktops (Cua Spaces).** Settings, Computer Use, "Give agents a desktop of their
  own". Each project folder gets one Space, a macOS VM made with Cua's `cua` CLI and Lume,
  which the agents working there share for computer use instead of your screen; the desk
  card's Screen panel shows it live, and a click in it is you stepping in. It is made when
  the project's first agent starts, turned off when none of its agents is in the office,
  after 15 idle minutes and when Hover quits, and deleted with the project's last session.
  Drag an app or files onto the notch and the office opens on the desktops to send them to.
  The full-screen button in the office opens Hover in a window. Needs macOS 26 or later on
  Apple silicon (two macOS desktops at a time, 8 GB of memory each); elsewhere the switch is
  off: "Agent desktops need macOS 26 or later on Apple silicon." The desktop may also be a
  Linux image, which needs Docker or Colima on the Mac.
- **The sandbox and the Screen panel** also work on Linux (the sandbox with bubblewrap, the
  panel over X11); Windows has the panel but no sandbox.

## What is off on a Mac

- **Local speech (Phonon), Cloud (Groq) and the text cleanup** have no Mac counterpart.
  Voice uses Apple's recognizer on the Mac (`Voice.swift`): SpeechAnalyzer on macOS 26 and
  later, SFSpeechRecognizer before, on device where the language allows (Settings → Voice
  says which).
- **Usage in the island**: it is in the menu bar.
- Intel: only `aarch64` (Apple Silicon) is checked in CI.

## Where things are

- Data: `~/Library/Application Support/Hover`. History is encrypted with AES-GCM. Its key is
  a generic password in the login Keychain (service `dev.hover.history`, account `history-v1`,
  this device only, after first unlock), which the Swift app reads or makes and hands to the
  backend when it starts. A Keychain error stops Hover with a message, and a missing key next
  to an existing history (`agents/index.dat`) is never replaced by a new one.
- Launch at Login is `SMAppService.mainApp`, switched from the menu bar item or Settings.
- The Swift app has no single-instance check of its own.
- The sandbox needs `srt` and `rg`: `npm install -g @anthropic-ai/sandbox-runtime@0.0.78` and
  `brew install ripgrep`. Without them tools start unsandboxed, with a line in `hover.log`;
  Settings
→
General has the switch, but the Mac Settings does not list what is missing.

## End-to-end run, in the background

```sh
scripts/sandbox.sh -- scripts/build-macos.sh
tests/macos/e2e/run.sh "$PWD/dist/macos-osx-arm64/Hover.app"
```

It drives the real office page, its bridge (`OfficeHost.swift`), Hover's browser and the
screen feed with the packaged Rust backend (`hover-backend`), a stand-in agent
(`tests/macos/e2e/fake-agent.py`) and a stand-in `gh`, against a local test site: a task
from the circle, an approval, two subagents and their helpers, the desk card, the agent
driving Hover's browser over its MCP relay, the agent's desktop (apps, activity, Control's
input as Hover maps it), the answer, every desk panel, Create pull request (a real push to a
local remote), a reply from the card, the window button, and agent desktops through
stand-ins for `cua` and `lume` (a Space made before the run, the agent's computer use
reaching it over the relay, the viewer laid over the panel, an app and files dropped on the
notch, deletion). The stand-ins are `tests/macos/e2e/fake-cua.py` and `fake-lume.py`,
copied as `cua` and `lume` into a `bin` folder that leads the backend's `PATH`; they keep
their state and a call log (`cua.log`) in the run's temp folder. No window is made and the
harness never activates; it writes only to its temp folder (`sandbox-exec`), reaches only
localhost, and touches no real app. Screenshots of each step are kept in that folder.

## Unverified

Written from the Swift and Rust code and from Apple's documentation; **none of this has run on
a Mac**. Look at these first:

- The notch window (`Notch.swift`): the non-activating `NSPanel` whose `canBecomeKey` follows
  whether the office is open (if that fails, a click on the resting notch can take focus),
  `.statusBar` level (25) on all Spaces, mouse pass-through that follows the polled pointer,
  the wings beside the housing on a real notch, the first hover over a full-screen app.
- The menu bar item: redrawing when the bar's light or dark changes.
- Hot keys, and the local monitor that catches them while a Hover window has the keyboard;
  Cmd-C / V / X in text fields through the main menu's Edit items (the app has no Dock icon).
- WKWebView: placement over the panel, `callAsyncJavaScript`, `takeSnapshot` for a page with
  no window.
- Screen capture through ScreenCaptureKit (`Screen.swift`) with Hover's own Screen Recording
  grant, and the window list it reads with `CGWindowListCopyWindowInfo`.
- The Keychain paths (prompt, locked, denied), `SMAppService` accepting the login item, the
  socket under `$TMPDIR`, and Apple's speech recognizer (both engines) with the two
  permission prompts.
- The sandbox: `srt`'s settings as Hover writes them (copied from the C#, not checked against
  srt 0.0.78), Claude Code under `srt` (it writes `~/.claude.json` through a temp file), and
  whether every agent passes the browser token in its MCP server's environment.
- Computer use's guard, Cua's install, the daemon, and the permission check.
- Agent desktops: Cua's `cua` and Lume as Hover calls them (the arguments are checked against
  the stand-in in `tests/macos/e2e`, not against a real `cua spaces`), a macOS 26 VM's size
  and memory on a real Mac, the viewer's stream, and dragging a window or files to the notch.
- The GitHub CLI flows (Homebrew install, device-code sign-in, Create pull request) and
  agent setup against the real installers.
- Launching the signed bundle: CI builds it (so the Swift compiles and links, and the build
  script's `codesign --verify` passes), but nobody has seen the hardened runtime, the
  entitlements, or `hover-guardian` starting the backend, run.

A Hover killed in the few milliseconds between an agent's start and its watchdog's can leave
the agent running (Linux has `PDEATHSIG` for that; a Mac has nothing to match).

If you run it on a Mac, what worked and what didn't is the most useful report there is.
