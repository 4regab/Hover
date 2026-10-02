# Hover on macOS

Hover 3.4 runs on a Mac: the same Rust app as on Windows and Linux, with `platform/macos.rs`
in hover-core and `app/src/mac/` in the app for what differs. It is based on Arz's
(@Entourage397) macOS v1.0, which was a Swift shell around the .NET engine of 2.x; none of
that Swift or C# is in this tree.

**Status: it builds, and nobody has run it on a Mac yet.** CI compiles it on Apple Silicon
(`cargo check`), and its pure parts (the menu, the notch geometry, key names, the
Keychain and LaunchAgent logic, the sandbox's settings, the agent browser's JavaScript) are
tested on Windows. Everything that needs AppKit, the window server, the Keychain or a
permission prompt has not been run. The last section lists it.

## Build from source

You need macOS 14 or later, Xcode's command line tools (`xcode-select --install`; full Xcode
is not needed) and Rust through [rustup](https://rustup.rs). `rust-toolchain.toml` picks the
version. From the repo root:

```sh
cargo build --release -p hover         # target/release/hoverai
sh packaging/macos/bundle.sh           # dist/Hover.app (not signed)
open dist/Hover.app
```

Why the bundle: macOS asks for the Microphone, and lists Screen Recording and notifications,
only for an app that has an `Info.plist` and an identifier (`dev.hover.desktop`). Run from a
terminal, `target/release/hoverai` works but the prompts and grants attach to the terminal.
`Info.plist` sets `LSUIElement`, so there is no Dock icon and no menu bar of Hover's own: it
is the notch and its menu-bar items. The bundle is not signed or notarized. The binary is
ad-hoc signed by the linker; to keep the permissions you grant across rebuilds, sign the
bundle with one identity (`codesign --force --sign - dist/Hover.app` is enough on your own
Mac). There is no release package yet, and CI makes none.

Tests: `cargo test --release --workspace` runs on a Mac the same as elsewhere. The CI job
does not run it.

Agents (Kiro, Codex, Cursor, OpenCode, Claude Code) are found through the login shell's
`PATH`, read once at start (`$SHELL -ilc`), because an app opened from Finder has only
`/usr/bin:/bin:/usr/sbin:/sbin`. `brew`, `~/.local/bin`, `~/.cargo/bin` and npm's folders
work. Each agent's page in Settings has a **Set up** row that installs what is missing and
opens the tool's sign-in in a Terminal window.

## What it looks like

- **The notch.** At rest Hover is the camera housing. While agents work the island grows
  wings either side of it (logos, what the front one is doing, for how long); a question
  drops a card under it. Hovering it (after a short dwell), clicking it or Option-N opens the
  office out of the notch. A Mac without a notch gets a notch-sized pill at the top centre.
  A click on the resting notch never activates Hover or takes focus from your app.
- **Usage is in the menu bar**, not the island: each reader you switch on shows the tool's
  logo in a ring of its used share, and the percentage (green, amber from 70 %, red from
  90 %). All open one menu: readings, "Show in menu bar" switches, agents at work, the
  office, the dashboard, Launch at Login, Settings, Refresh, Quit.
- **The dashboard** is an ordinary window with the system's title bar.
- **Settings pages** can be opened from a shell: `open -a Hover --args --settings integrations`
  (the page title in lower case, spaces as `-`).

## Permissions

| Permission | Who asks | For | When |
|---|---|---|---|
| Microphone | Hover | voice dictation with Cloud (Groq) | the first time you hold the voice shortcut |
| Screen Recording | Hover | the desk card's Screen tab showing the agent's apps live | the tab's **Allow…**; macOS wants Hover restarted after the grant. Without it the tab shows the wallpaper. |
| Accessibility | CuaDriver | computer use: reading and operating other apps | Settings → Integrations → **Grant access…** |
| Screen Recording | CuaDriver | computer use: seeing other apps' windows | same |
| Keychain | Hover | its history key (service `Hover`), and reading Claude Code's own entry (`Claude Code-credentials`) for the quota if that reader is on | the first use; allow it |

Computer use's two grants go to **CuaDriver**, not Hover, because CuaDriver is the program
that acts. Hover's hot keys are Carbon hot keys and the pointer is read with
`NSEvent.mouseLocation`, so neither needs Accessibility or Input Monitoring. Notifications
(the end of a task) go through the system's notification centre, or through AppleScript's
`display notification` when Hover runs as a bare binary.

## What is Mac-only

- The notch window and the menu bar items above, and Option-N / Control-Option-Space as
  global hot keys. The recorder takes the physical key, because Option changes the letter.
- **The agent browser.** Every agent gets a browser tool (open, snapshot, click, type, press,
  scroll, screenshot, evaluate, wait, console, back, reload). It runs in a WKWebView per
  session, shown in the desk card's Browser tab, and talks to the agent over a Unix socket
  only you can use. Off elsewhere: "Agent browser needs macOS."
- **One-click agent setup**, with the makers' own installers. Off elsewhere.
- **Computer use** is built for the Mac (Cua Driver's daemon and its permissions).
- **The sandbox and the Screen panel** also work on Linux (the sandbox with bubblewrap, the
  panel over X11); Windows has the panel but no sandbox.

## What is off on a Mac

- **Local speech (Phonon)** has no Mac runtime. The Voice page says "Local speech isn’t
  available on macOS yet; use Cloud (Groq)." Cloud works.
- **Usage in the island**: it is in the menu bar.
- Intel: only `aarch64` (Apple Silicon) is checked in CI.

## Where things are

- Data: `~/Library/Application Support/Hover`. History is encrypted with AES-GCM; its key is
  a generic password in the login Keychain (service `Hover`), named by a small marker in
  `note.key`. If the Keychain refuses the write, `note.key` holds the key in a file only you
  can read (mode 0600). A locked Keychain or a denied prompt stops saving for that run, and
  never makes a new key over the old history. A history made by the first native (Swift)
  build is adopted, re-wrapped under Hover's own item.
- Launch at Login is a LaunchAgent, `~/Library/LaunchAgents/dev.hover.desktop.plist`, that
  runs the current executable (in the bundle, that is `Hover.app/Contents/MacOS/hoverai`).
  Arz's Swift build used SMAppService; an entry made there shows as off here.
- One copy runs at a time; a second launch opens the dashboard of the first.
- The sandbox needs `srt` and `rg`: `npm install -g @anthropic-ai/sandbox-runtime@0.0.78` and
  `brew install ripgrep`. Settings → Integrations shows what is missing, and tools start
  unsandboxed (with a line in `hover.log`) until it is there.

## Unverified

Written from the C# and Swift code and from Apple's documentation; **none of this has run on
a Mac**. Look at these first:

- The notch window: the `canBecomeKeyWindow` class swap on winit's window (if it fails, the
  log says "the window keeps winit's class" and a click on the notch can take focus), level 25
  on all Spaces, mouse pass-through except over the shape, the wings beside the housing on a
  real notch, the first hover over a full-screen app.
- The menu bar items: redrawing when the bar's light or dark changes, and rebuilding an open
  menu when a reading arrives.
- Hot keys, and the local monitor that catches them while a Hover window has the keyboard;
  Cmd-C / V / X in text fields now that the default menu is off.
- WKWebView: placement over the panel, `callAsyncJavaScript`, `takeSnapshot` for a page with
  no window.
- Screen capture through `CGWindowListCreateImageFromArray`, which Apple deprecated (Hover
  looks it up at run time and reports a clear error if a future macOS removes it).
- The Keychain paths (prompt, locked, denied), `defaults read` for dark mode and reduced
  motion, the LaunchAgent being accepted by launchd, the socket under `$TMPDIR`.
- The sandbox: `srt`'s settings as Hover writes them (copied from the C#, not checked against
  srt 0.0.78), Claude Code under `srt` (it writes `~/.claude.json` through a temp file), and
  whether every agent passes the browser token in its MCP server's environment.
- Computer use's guard, Cua's install, the daemon, and the permission check.
- The GitHub CLI flows (Homebrew install, device-code sign-in, Create pull request) and
  agent setup against the real installers.
- Linking: the frameworks (AppKit, WebKit, CoreGraphics, Carbon) are only linked by the
  CI runner.

A Hover killed in the few milliseconds between an agent's start and its watchdog's can leave
the agent running (Linux has `PDEATHSIG` for that; a Mac has nothing to match).

If you run it on a Mac, what worked and what didn't is the most useful report there is.
