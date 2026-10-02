# Hover on macOS

Hover now has a native AppKit shell and WKWebView office, backed by the shared C#
agent engine. The Windows WPF app remains available and references the same shared
sources. The Mac app is packaged with its .NET runtime, office page, fonts, audio,
icon, SQLite native library, licenses and process supervisor.

## Installing a release

Use the Apple Silicon (`osx-arm64`) release on an M-series Mac or the Intel
(`osx-x64`) release on an Intel Mac. Requires macOS 14 or later. Unzip and drag
`Hover.app` to Applications, then open it. There is no .NET, Node, Homebrew or
WebView2 installation required to run Hover.

Hover lives over the MacBook's notch, as Notchy does. At rest it is the notch
itself; while agents work it grows wings either side (their logos, what the front
one is doing and for how long), a question an agent is waiting on drops a card
under the notch (Deny, Allow, Review), and a finished task shows its tool's logo
with a badge for six seconds. Hovering the notch (after a short dwell) or clicking
it opens the office out of the notch; Option-N toggles it. Opened by hover, it
folds when the pointer leaves; after a click inside it keeps the keyboard and
folds on a click outside, Esc or Option-N. A waiting question never opens on
hover. Macs without a notch get a notch-sized pill at the top centre.

Usage lives in the menu bar instead of the notch: each tool switched on shows its
logo in a ring of its used share and the percentage (green, amber from 70 %, red
from 90 %). Its menu lists each reading's details, a "Show in menu bar" submenu to
switch readers on or off, the agents at work, Open Office, Office in a Window,
Open on Hover, Launch at Login, Settings, Refresh and Quit.

Voice tasks: hold Control-Option-Space and say the task; let go to see it. A glass
capsule drops in under the notch with Siri's colours turning round its rim, an orb and a
waveform that move with your voice, the words as they're heard, and a soft glow round the
screen's edges. Let go and it opens into a card: the task (editable), the agent (a new
chat with the agent last used in the office, any other agent, or a reply to a bot at a
desk), the folder, and the agent's tool access. It starts after three seconds unless you
edit something; Return starts now, Esc cancels. A quick tap instead of a hold listens
hands-free until the next press. Saying "Ask Codex to…" or "Cursor, …" picks that agent,
and a bot's name ("Pip, …") replies to it. The folder is a project named in what was said
(Settings → Voice → Projects, each with other names it may be called), else the folder
last used, else `~/Hover`, made when first needed. Speech is turned into text on the Mac:
SpeechAnalyzer with SpeechTranscriber on macOS 26 and later (the system downloads its
model once), SFSpeechRecognizer before that. No audio is saved or sent. Hover asks for
Microphone and Speech Recognition the first time. Settings → Voice holds the switch, the
countdown (3 s, 5 s or Return), the default agent, the glow and sounds, the permissions,
Try It (shows what would start without starting it) and the projects. The menu bar has
Start a Voice Task. `open -a Hover --args --settings voice` opens that page.
The shortcut is a Carbon hot key, but the window server gives keys straight to whichever
of Hover's own panels has the keyboard (the office in the notch, the dashboard, Settings),
and there the hot key never fires; a local key monitor catches Control-Option-Space (and
Esc while listening) in those windows too. Kiro's name and the bots' are given to the
model as contextual strings, and the router also takes what it is often heard as ("Piro",
"Kyro"). Each stage is in the system log: `log show --info --predicate 'subsystem ==
"dev.hover.desktop" AND category == "voice"'` (stages and counts, never the words).
`open -n -g -a Hover --args --voice-probe <report> [<audio file>]` checks the pipeline with
no window and no prompts, as Hover itself: the grants, the shortcuts, a second of
microphone level when allowed, and what the model hears in the file and how it is routed
(`say -o clip.aiff "Ask Kiro to …"` makes one).

The chat shows what the tools show of their thinking (a Thought row, folded), the
subagents they start, each change with Copy (and line numbers where the diff names them),
command output with its exit code (green only for a real 0), and under each answer Copy,
Retry on the newest turn and the time it finished. A reply while an agent waits on an
approval waits behind it instead of denying it; Stop with replies queued stops this one
and sends the next.

Settings → Get Started sets up Codex, Kiro and Cursor with one click each.
Set Up installs what is missing with the maker's own installer (Kiro:
`curl -fsSL https://cli.kiro.dev/install | bash`; Cursor:
`curl -fsS https://cursor.com/install | bash`; Codex's ACP adapter, and Codex if it
is missing, from npm into `~/.local`, with Homebrew's Node if there is no Node),
showing the installer's progress, then opens the tool's own sign-in (`codex login`,
`kiro-cli login`, `cursor-agent login`) in a Terminal window and turns Ready by
itself once the tool's status command says signed in. Hover never handles the
credentials. A greyed tool in the office opens its setup page. Each agent's page
holds its tool access, idle timeout and step visibility; every change applies at once.

Settings opens on Get Started until agent access is acknowledged. General holds
hover, concurrency, notifications and login launch; Usage the opt-in readers;
Computer Use the switch that hands every agent Cua Driver (`cua-driver mcp`), with
Install (Cua's own installer: CuaDriver.app in /Applications, `cua-driver` in
`~/.local/bin`) and Grant Access (`cua-driver permissions grant`, so the
Accessibility and Screen Recording grants go to CuaDriver, not to Hover).
`open -a Hover --args --settings computer-use` opens that page.
Agents get Cua Driver behind Hover's guard (`/usr/bin/perl <Support>/cua/guard.pl
cua-driver mcp`, `ComputerUse.Guard`), so their computer use never gets in the user's
way: every input goes to the app the agent names, in the background (Cua's own agent
cursor, no real pointer moves, no focus change). Foreground delivery is rewritten to
background and taken out of the tool list; desktop-scope input, input with no app
named, `bring_to_front`, window moves, `kill_app`, the clipboard, replays and Cua's
settings are refused with a note the agent reads. Under srt every tool runs through a
blocking-pipe relay (`Sandbox.Relay`): srt's non-blocking stdio made Cua's 67 KB tool
list fail with EAGAIN and took Kiro down with it.
A click on a desk opens its panels menu (Browser, Terminal, Files, Diff, Pull request,
Linked pull requests, Agents, Screen); a click on the bot opens its chat. Screen shows
the desktop at rest and goes live while the agent uses computer use; the live picture
needs Hover's own Screen Recording grant (Allow… in the panel; quit and reopen after).
The office's web view takes the first click, so a bot or desk acts at once even when
Hover's window is behind another app.
Models and effort are selected in the office, which on the Mac draws at the
display's density with antialiasing and runs at the display's frame rate while
anything moves (Windows keeps its 1x, 30 fps budget).
Quota readers depend on the tool's current storage/API format; failures appear as
readable details. Claude's default macOS Keychain entry is read only after opting
in; custom Claude credential service names may need further support.

Data lives in `~/Library/Application Support/Hover`. History is encrypted with
AES-GCM; its key is stored in Keychain. An unreadable or missing key does not cause
new history to overwrite existing history. Windows DPAPI keys cannot be copied to
macOS directly. Back up both your data and the associated Keychain entry.

## Building locally

Developer requirements only: .NET 10 SDK, Node/npm, Apple Command Line Tools
(`xcode-select --install`). Full Xcode is not required for this CLI build.
Run commands from the repo root:

```sh
./scripts/build-macos.sh
HOVER_ARCH=x64 ./scripts/build-macos.sh
```

Outputs are `dist/macos-osx-arm64/Hover.app` and
`dist/macos-osx-x64/Hover.app`. Each contains a self-contained runtime.
Local builds are ad hoc signed. These are suitable for local development, but an
app downloaded by other people needs Developer ID signing and notarization for
normal Gatekeeper acceptance. The checked-out build does not contain signing keys.

## Producing a distributable release

Import your Developer ID Application certificate into the build machine's
Keychain. Store notarization credentials in a named `notarytool` profile using
Apple's documented workflow; do not put secrets in the repository.

```sh
HOVER_SIGN_IDENTITY='Developer ID Application: Your Name (TEAMID)' \
HOVER_NOTARY_PROFILE='hover-release' ./scripts/package-macos.sh
```

Set `HOVER_ARCH=x64` for the Intel artifact. The script signs each native runtime
library and executable, applies the JIT entitlement only to the .NET apphost,
signs the outer app, submits it to Apple, staples and validates the notarization
ticket, checks Gatekeeper, and recreates the shareable ZIP. Release signing uses
the hardened runtime; local ad hoc builds omit it because their libraries have no
Developer Team ID. Signing/notarization has not been validated on this device:
it currently has no valid Developer ID identity.

The ZIP contains a normal app bundle. Recipients install the app, rather than
running a source checkout or a setup script. Agent installation and provider
accounts remain the user's own.

## Testing without touching personal data

```sh
./scripts/test-macos.sh
```

The runner makes a separate `/private/tmp/hover-sandbox.*` checkout, clones the
SDK, redirects package caches, CLI state, app data and temporary files, and builds
there. SDK certificate generation is disabled. Package restore/build can download
public dependencies; executable tests run under `sandbox-exec` with personal
files, Keychains, writes outside the sandbox and external network access denied.
A denied write probe must pass before any tests run. Localhost is permitted for
fake OpenCode servers. No real coding agent is launched: packaged tests use a
synthetic executable and a restricted PATH. Native smoke mode uses an ephemeral
WebKit store and a temporary key instead of Keychain, and skips login registration,
notifications, global hotkeys and normal input polling.

Checks include the portable NUnit suite; packaged fake-agent initialization,
approvals, replies, cancellation, encrypted history, restart, deletion and UI-loss
cleanup and settings persistence; native Settings hit tests and control actions,
100 repeated preference replies, draft preservation, Save/Close behavior and
hover suppression; and real WKWebView JavaScript/bridge/canvas loading with a
saved screenshot. The interaction harness is omitted from the production app,
which is rebuilt and smoke-tested in the same sandbox. Foreground activation and
physical mouse dispatch cannot be verified by the inactive sandbox launcher;
those still require a manual check in a normal launch.
Reports and screenshots remain in the sandbox for inspection. This is macOS
process sandboxing, not a VM: WebKit rendering is provided by macOS XPC services.
No live-provider tests or Windows pointer-driving E2E tests run on this device.

Windows compatibility is checked with `dotnet build Hover.slnx -c Release`.
Windows WPF tests still require a Windows machine. Intel artifacts can be built
here but need validation on Intel hardware; the Apple Silicon artifact is the one
executed by the local sandbox suite. Physical notch, multiple-monitor, fullscreen,
login-item and notification permission behavior still need normal release QA on
representative Macs. These integrations are skipped by the isolated smoke test.

## End-to-end run, in the background

```sh
scripts/sandbox.sh -- scripts/build-macos.sh
tests/macos/e2e/run.sh "$PWD/dist/macos-osx-arm64/Hover.app"
```

It drives the real office page, its bridge (`OfficeHost.swift`), Hover's browser and the
screen feed with the packaged backend, a stand-in agent (`tests/macos/e2e/fake-agent.py`)
and a stand-in `gh`, against a local test site: a task from the circle, an approval,
two subagents and their helpers, the desk card, the agent driving Hover's browser over
its MCP relay, the agent's desktop (apps, activity, Control's input as Hover maps it),
the answer, every desk panel, Create pull request (a real push to a local remote) and a
reply from the card. No window is made and the harness never activates; it writes only
to its temp folder (`sandbox-exec`), reaches only localhost, and touches no real app.
Screenshots of each step are kept in that folder.

## Architecture

- `src/Hover.Shared`: portable engine plus a Windows target for the existing WPF
  settings/shortcut and DPAPI integrations, with existing source files linked
  rather than forked implementations.
- `src/Hover.Backend`: a single-threaded event loop, private JSON pipe protocol,
  settings/history/approval routing and the office's serialized state.
- `macos/Sources/Hover.swift`: app lifecycle, backend pipe, pointer logic, dashboard,
  Settings, hotkey, Keychain, local resource scheme and JavaScript bridge.
- `macos/Sources/Notch.swift`: notch geometry (`auxiliaryTopLeftArea`/`TopRightArea`),
  one fixed window whose spring-animated mask grows from the notch to the island and
  the office, and the island's drawing and hit areas.
- `macos/Sources/MenuBar.swift`: the usage rings and the status menu.
- `macos/Sources/Settings.swift`: the SwiftUI Settings window and its model.
- `src/Hover/Services/AgentSetup.cs`: one-click install and sign-in.
- `macos/Sources/Marks.swift`: the tools' logos from the Windows path data.
- `macos/Sources/Voice.swift`: live dictation (AVAudioEngine; SpeechAnalyzer on macOS 26+,
  SFSpeechRecognizer before) and the microphone and speech permissions.
- `macos/Sources/VoicePanel.swift`: the voice flow (shortcut, routing to an agent and a
  folder, countdown, start), its SwiftUI capsule and card, and the screen's edge glow.
- `macos/Sources/ShellEnvironment.swift`: the login-shell environment probe.
- `macos/Sources/guardian.c`: a supervisor holding the UI pipe; UI EOF shuts down
  the backend and terminates the backend process group, including agent descendants.
- `tests/Hover.Portable.Tests`: existing non-WPF fixtures against the shared code.

Local image serving is restricted to known session folders, raster images and
size limits, with canonical-path checks against traversal and symlink escapes.
The webview cannot navigate to remote pages or grant the host bridge to subframes.
Secrets travel over private pipes and are not put on command lines or in logs.

References: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution),
[.NET macOS publishing](https://learn.microsoft.com/en-us/dotnet/core/deploying/macos),
[.NET supported systems](https://github.com/dotnet/core/blob/main/release-notes/10.0/supported-os.md).
