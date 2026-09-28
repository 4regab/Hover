# Phase 3 report: sessions, persistence, product

**Status: 3B (persistence) and 3A (sessions and ACP) built and tested on Linux; their
Windows checks are pending.** 3C follows in this file as it is built. Nothing here has run on Windows. The
steps for the pending checks are in `RUN-ON-WINDOWS.md`. The C# app is unchanged and
is still the product.

Order of work: 3B, 3A, 3C (the user's order). No dependency forced a change.

## 3B: persistence (`native/crates/hover-core`)

| C# | Rust | What it does |
|---|---|---|
| `Core/Paths.cs` | `paths.rs` | `HOVER_DATA_DIR` (made absolute as `Path.GetFullPath` makes it), else `%APPDATA%\Hover` (the roaming known folder) or `$XDG_DATA_HOME/Hover` (`~/.local/share/Hover`; a relative `XDG_DATA_HOME` is ignored, as the spec says). The `Noty` → `Hover` move runs once, only when the old folder exists and the new one doesn't. `drop_planner` is `OwlApp.DropPlanner`. |
| `Core/Settings.cs` | `settings.rs` | The `Model`, field for field, with its defaults. Setters as in C#: notch items filtered to the canonical order (the getter writes the filtered list back, as C# does), a blank folder is none, `SetAgentOptions` normalises (`auto` → none, blank agent → none, idle minutes 5 or 15, Kiro's effort defaults to `high`), offers written only when they change. Writes wait until nothing has changed for 400 ms; `flush` writes at once. Launch at login: HKCU `Run` on Windows, `~/.config/autostart/hover.desktop` on Linux (the AppImage's own path when run from one). |
| `System.Text.Json` | `json.rs`, `time.rs` | A reader as strict as `Utf8JsonReader`'s defaults (no comments, no trailing commas, depth 64, no lone surrogates, only the four JSON spaces) and a writer that escapes as `JavaScriptEncoder.Default` does (`"` is `\u0022`; `& ' + < > \`` escaped; everything outside printable ASCII as upper-case `\uXXXX`), indents with two spaces and `Environment.NewLine`, writes doubles as .NET's shortest round-trip (`1E+15`, `1E-05`) and `DateTime` in the trimmed round-trip form with the local offset (`2026-09-28T18:44:07.1234567+02:00`). Enums by name (read in any case, or by number), flags as `"Control, Shift"`, WPF `Key` names. |
| `Core/Crypto.cs` | `crypto.rs` | AES-256-GCM, `nonce(12) ‖ ciphertext ‖ tag(16)`, no associated data, UTF-8 text; `open` gives `""` for missing, short or foreign data. The key: 32 random bytes, loaded or made on first use, wrapped by the platform's `KeyGuard`. |
| DPAPI | `platform/windows.rs` | `CryptProtectData`/`CryptUnprotectData`, no entropy, `CRYPTPROTECT_UI_FORBIDDEN`: the call `ProtectedData.Protect(…, CurrentUser)` makes, so either build opens the other's `note.key`. |
| (none: Linux) | `platform/linux.rs` | Secret Service over D-Bus (zbus): the key is an item in the default collection (`xdg:schema dev.hover.Key`, a random id), and `note.key` holds `hover-key:secret-service:<id>`. With no Secret Service, `note.key` holds the key itself, mode 0600. |
| `Owl/AgentHistory.cs` | `history.rs` | `agents/index.dat` and `agents/<key>.dat`, each sealed; compact JSON with string enums; writes to `.tmp` then replace, in order, on a thread of their own; `flush` waits up to 10 s; entries newest first (stable); a key that isn't 1–64 ASCII letters and digits never becomes a path. The records (`SavedTurn`, `SavedSession`, `HistoryEntry`, `KiroStep`) in declaration order. |
| `KiroPage.ImagesFolder`, `SaveImages` | `images.rs` | `kiro-images`, swept of files older than 14 days once per run; data URLs saved as `yyyyMMdd-HHmmss-<guid>` cut to 24 characters, four at most, 8 MiB each, PNG/JPEG/GIF/WebP only; `Convert.FromBase64String`'s rules. `chat-proto` now uses these. |
| `App.OnStartup` (mutex, show event) | `single.rs` | Windows: `Local\HoverRunningInstance` and `Local\HoverShowApp`, the C# app's own names (so either build defers to the other). Linux: a lock on `$XDG_RUNTIME_DIR/hover.lock` (released by the kernel however the process ends) and `hover.sock`, which a second launch writes `show <activation token>` to. |
| `port/tools/HoverFixture` | `hover-data` (a bin in hover-core) | `write`: HoverFixture's data folder, written by the port (settings, key, a sealed long session). `dump`: what a data folder holds, read by the port. The two halves of the C#↔Rust round trip on Windows, and the Linux benchmark's data. |

### Evidence

`cargo test --release --workspace`: all green, 33 new tests in hover-core (31 unit, 2
against a real Secret Service). `cargo check --release --workspace --target
x86_64-pc-windows-msvc`: green. `cargo clippy -p hover-core --all-targets`: no warnings.

Where the expected values come from. HoverFixture can't run on Linux (WPF and DPAPI),
no output of it is in the repo, and new .NET fixture tools are ruled out, so the
expected values are derived from the C# source and the .NET serializer's rules, and
each test says so:

| Test | Expected from |
|---|---|
| `settings::a_fresh_model_writes_as_system_text_json_writes_it` | `Settings.Model`'s declaration order and initialisers, `WriteIndented`, `JsonStringEnumConverter`: the whole default file, byte for byte, with CRLF |
| `settings::the_shortcut_and_notch_items_as_settings_tests_expect`, `the_folder_escapes_its_backslashes…` | `tests/Hover.Tests/SettingsTests.cs`, ported (its literal `"Key": "H"` and `"KiroFolder": "C:\\\\Projects\\\\Hover"`) |
| `settings::a_bad_file_gives_the_defaults` | `Settings.Load`: any exception, the defaults; case-sensitive names, unknown keys ignored, last duplicate wins |
| `json::strings_escape_…`, `doubles_format_…`, `the_reader_is_as_strict_…` | `JavaScriptEncoder.Default`'s allowed ranges, `Utf8JsonWriter`'s escape switch, `double.ToString()` on .NET Core 3.0+, `Utf8JsonReader`'s defaults |
| `time::written_as_…`, `read_as_…` | `JsonWriterHelper.WriteDateTimeTrimmed` and `JsonHelpers.TryParseAsISO` |
| `history::a_session_writes_as_the_serializer_writes_the_record` | the records' parameter order; the full compact JSON of a session |
| `history::sessions_are_sealed_…`, `a_key_that_isnt_hovers…` | `tests/Hover.Tests/AgentHistoryTests.cs`, ported |
| `crypto::frames_a_known_gcm_vector` | the GCM specification's test case 14 (256-bit key): the frame is IV ‖ C ‖ T end to end, so any AES-GCM, .NET's included, opens it |
| `crypto::round_trips_unicode…` | `tests/Hover.Tests/CryptoTests.cs`, ported |
| `images::*` | `KiroPage.SaveImages` and `ImagesFolder`, line for line |
| `secret_service::*` | runs GNOME Keyring 46 on a private D-Bus (never the user's): the key is stored and found again; with the keyring gone, the marker is an error, never mistaken for a key; the old item survives a new key; with no bus, a 0600 file |

The round trip at size, on Linux:

```
$ hover-data write /tmp/hd/data /tmp/hd/project 200 golden/fixtures/rich.md
hover: no Secret Service (...); note.key keeps the key, for this user only
6a894c904beb4d3180c73ddf6a087613
$ hover-data dump /tmp/hd/data
history: 1 sessions
  6a894c90… Kiro Completed "A long rich conversation" 200 turns, 229200 answer bytes, updated 2026-09-28T17:49:11.4337679+00:00
$ ls -l /tmp/hd/data /tmp/hd/data/agents
-rw-------  32      note.key
-rw-r--r--  115     settings.json
-rw-r--r--  390934  6a894c904beb4d3180c73ddf6a087613.dat
-rw-r--r--  231     index.dat
```

3B has no UI, so there are no screenshots for it.

### The key on Linux: the trade-off

| | Windows (DPAPI) | Linux, Secret Service | Linux, no Secret Service |
|---|---|---|---|
| At rest (disk taken away) | encrypted with the user's login | encrypted with the keyring's password (the login's) | **plain** unless the disk is encrypted |
| Other users | can't | can't | can't (0600) |
| Other programs of the same user | can (DPAPI has no per-app rule) | can (no per-app rule in GNOME Keyring) | can |
| Moved to another machine | useless there | useless without the keyring | works (it is the key) |

So the Secret Service matches DPAPI; the 0600 file is weaker at rest, and is what a
headless or minimal session (no keyring daemon) gets. A `note.key` from Windows is a
DPAPI blob and can't be read on Linux; the history can't move between platforms.

### Differences from the C# app

Bugs fixed (listed, not asked):
1. **A `null` shortcut in settings.json** left C# with no shortcut object (a crash where
   it is read). The port reads it as unset.
2. **A session record missing a list** (`Turns`, `Images`, `Steps`) made C# hold a null
   and throw later; the port reads an empty list. Only hand-edited files have this.
3. **chat-proto's `SaveImages` had drifted from the C#**: the size check left out the
   comma C# counts, and `data:image/png;foo=1;base64,…` was refused where C# takes it
   (C# reads the type up to the first `;`). Both now as C#. Its file names used UTC on
   Linux; now local time, as `DateTime.Now`.

Changed by decision (question 1): C# replaced an unreadable key and lost the history
sealed with it; the port never does (above).

Known, small, by design:
4. **An enum given as a number that names no member** (`"Appearance": 7`): .NET keeps
   the number and writes it back; the port reads the property's default.
5. **Which name .NET writes for a WPF key with two names** (Return/Enter, Prior/PageUp,
   Capital/CapsLock, the OEM keys) can't be settled from the source alone. The port
   writes the first declared and reads both. Only a shortcut on one of those keys is
   affected; a Windows check is listed.

### Questions, answered (2026-09-28: 1 → B and C together, 2A, 3A, 4A)

Built as answered (`crypto::load_or_create`, tested in `crypto::tests` and
`tests/secret_service.rs`): **a key is never destroyed.** One that can never be read
(DPAPI refuses, the keyring item is gone, a foreign file) is kept as
`note.key.unreadable-<yyyyMMddHHmmss>` and a new key made; one that can't be read *now*
(the keyring isn't running or stays locked) is left untouched and that run has no
history (`crypto::global()` is `None`), and the next start tries again. A new key that
can't be stored also means no history that run, where C# sealed with it and lost it on
the next start. Linux without a Secret Service keeps the key in a 0600 file (2A),
`settings.json` is LF on Linux (3A), and the tools aren't paused with Hover (4A).

1. **When `note.key` can't be unwrapped** (DPAPI refuses after a profile repair; on
   Linux the keyring isn't running), C# makes a new key and overwrites the file, so
   every saved session is lost for good.
   - **A.** As C# (what the port does now).
   - **B.** Keep the old file as `note.key.unreadable-<time>`, then make a new key: the
     history can be recovered by hand once the cause is fixed.
   - **C.** On Linux only, when the marker names a keyring item and the keyring can't be
     reached, don't make a new key: run this session without history, say so once, and
     try again next launch. Windows as A.
2. **Linux without a Secret Service:**
   - **A.** The key in a 0600 `note.key` (what the port does now).
   - **B.** No history at all until a keyring is available.
   - **C.** A, and Settings → General says once that the key is not encrypted at rest.
3. **Line endings of `settings.json` on Linux:**
   - **A.** LF, what .NET itself writes on Linux (what the port does now).
   - **B.** CRLF, the Windows file byte for byte.

### Pending on Windows (RUN-ON-WINDOWS.md)

- HoverFixture's data folder read by `hover-data dump` (DPAPI key, sealed history).
- `hover-data write`'s folder opened by the C# Hover (history listed and opened).
- `settings.json` written by C# and by the port compared byte for byte, including a
  shortcut on Enter or PageUp (difference 5).
- `%APPDATA%\Noty` moved on first run; the HKCU `Run` value set and cleared.
- The single instance against a running C# Hover and a running port.

### Costs of what's left in 3B

None in code. The Windows run above is about half an hour with the Rust toolchain
installed.

## 3A: sessions and ACP (`native/crates/hover-agents`)

| C# | Rust | What it does |
|---|---|---|
| `Services/KiroRunner.cs` `KiroStream` | `stream.rs` | Reads updates loosely: phases from tool kinds then titles, steps (first call names it, updates carry the status, target from `locations` then `rawInput`), context from `usage_update` and Kiro's `session_info_update` (reported when it moves half a point), the answer as the last message (a new message id, text after a tool call, or Codex's `final_answer`), 64 K kept, the outcome and its sign-in / MCP / last-lines explanations. |
| `Services/AcpHost.cs` | `acp.rs` | One process per tool shared by its sessions; `initialize` (`loadSession`), `session/new`, `session/load` with its replay muted, config options set where offered and different (model, then the effort it may only offer then, then autopilot/mode per tool), `session/prompt` over stdin only, `session/cancel` and the 8 s rule, permission answers (read only allows read/search/fetch/think), `-32601` for the rest, `_kiro/mcp/status`, idle shutdown after 5 or 15 min, a dead tool failing its runs with the end of its stderr. Every message is the C# anonymous object's bytes. |
| `Services/Agents.cs` | `agents.rs` | Where each tool is (`kiro-cli`, `codex-acp`, Cursor's `%LOCALAPPDATA%\cursor-agent\cursor-agent.cmd` then `cursor-agent`), its ACP arguments, install and sign-in hints, the status check kept 5 minutes and shared while it runs. |
| `ChildJob`, `Quota.Hidden`, `Quota.OnPath` | `proc.rs` | Windows: a job object (`KILL_ON_JOB_CLOSE`) per tool, `CREATE_NO_WINDOW`, `.cmd` through `cmd /d /c`. Linux: the tool leads its own process group (`setsid`), gets `PR_SET_PDEATHSIG`, and a watchdog `sh` kills the group when Hover's end of its pipe closes, however Hover ended. `NO_COLOR`, `TERM=dumb`, started in the home folder. |
| `Owl/KiroSession.cs` | `session.rs` | Turns, queue (a reply during a run waits; a stop drops the waiting ones), images sent as paths after the prompt, three running across tools, six kept, seats and bots the lowest free, wake from the history onto a free desk, delete, dismiss, select; saved on start, reply and end. The C# lives on the UI thread; here one lock, a thread per turn, events raised with the lock released. |
| `KiroPage.Push`, `State`, `Row`, `Relative`, `Short`, `Models`, `History` | `state.rs` | The office's `state` message, `transcript` and `say`, byte for byte. |
| (the page) | `chat-proto --acp <agent>` | The drawer on a real session: sends start it or reply in it, Stop stops it, and the state is read every 120 ms (KiroPage's push timer) through hover-chat, as the page reads the C# message. |

### Evidence

`cargo test --release --workspace`: 102 tests, all green (35 in hover-agents).
`cargo check --release --workspace --all-targets --target x86_64-pc-windows-msvc`: green.
`cargo clippy -p hover-agents -p hover-core --all-targets`: no warnings; chat-proto's
three are the ones it had before.

| Test | Against |
|---|---|
| `tests/acp_host.rs` (8) | `AcpHostTests`, ported with its stand-in agent line for line: a turn and a reply in one session and one process; load after shutdown with the replay ignored; a tool that dies fails its run and the next starts it again; stop; read only refuses a write and says why; model then effort; missing folder or tool. Plus: `-32601` for unserved requests, a permission for an unknown session cancelled, a sign-in error explained, and the `initialize` and `session/prompt` bytes as System.Text.Json writes them (`\u0022`, `\u0026`, `\u0027`, `\u00FC`) |
| `tests/fakeacp.rs` (5) | **FakeAcp recordings** in `native/golden/acp/`: recorded against the real `port/tools/FakeAcp` process (.NET 10 on Linux) with `FAKEACP=… HOVER_RECORD=1`, replayed without .NET: every line Hover sends must be the recorded one, and FakeAcp's own bytes are played back. A turn with steps, usage and the rich answer, then a reply; a stop at the second step; an idle shutdown and `session/load`; FakeAcp killed mid-run; a queued reply in the same conversation. Live and replayed both pass. |
| `tests/sessions.rs` (9) | `KiroSessionTests`, `KiroSessionsTests` and the wake/delete cases of `AgentHistoryTests`, ported |
| `tests/state.rs` | **`golden/fixtures/office-state.json`**, the state the page goldens were made from: the port's message is its bytes. The fixture was written by hand, so four values in it can't be what C# writes, and the expected value corrects them, each commented: session titles (C# derives them from the prompt), a `t0` written as a double (a `long` in C#), Cursor's empty model list (C# always puts Default first), and a history row's `waking` (C# writes `working` for a running entry). |
| `proc::a_killed_group_takes_what_it_started` | a grandchild dies with its tool |

Killing Hover, on Linux: `probe` (an example in hover-agents) running a turn on FakeAcp,
then `kill -9` on it:

```
before:
    346  304  346 /projects/sandbox/fakeacp/FakeAcp acp --agent-engine v3 --auth-method cli
    347  304  347 /bin/sh -c read _; kill -KILL -- -"$0" 2>/dev/null 346
after SIGKILL of the host:
  nothing left
```

The real CLIs on Linux: `codex-acp` and `codex` installed from npm are found on PATH;
the status check says "Sign in: run “codex login” in a terminal." and a turn fails with
the sign-in explanation in 0.3 s. No sign-in exists here, so no real turn ran; kiro-cli
and the Cursor CLI are not installed here.

Screenshots (headless, 2×, a live FakeAcp session, the drawer drawn from the Rust state):
- [mid-run: the live step, Searching…, the queue placeholder and Stop](https://github.com/4regab/Hover/blob/rust-port/phase-0-1/port/phase3/shots/chat-live-fakeacp-working-2x.png)
- [done: FakeAcp's answer (the rich fixture) with its code, diagram and image](https://github.com/4regab/Hover/blob/rust-port/phase-0-1/port/phase3/shots/chat-live-fakeacp-done-2x.png)

### Differences from the C# app

Fixed (listed, not asked):
6. **A reply whose id is a number but not an integer** (`"id": 1.5`) threw in
   `AcpHost.Handle` outside what `Read` catches, which ended the reader for good: the
   tool looked alive and every later run hung until its timeout. The port ignores the line.

Linux adaptations (the C# never ran there):
7. **Step lines relative to the folder**: C# compares with a backslash root, so on Linux
   no target would ever be made relative. The port uses `/` there.
8. **Tools in `~/.local/bin`** are found when that folder isn't on PATH (a desktop session
   often lacks it; the counterpart of C#'s Cursor shim).
9. **Cursor's install hint** is the Linux one (`curl https://cursor.com/install -fsS | bash`).
10. **Codex read only** is offered on Linux, as C#'s `ReadOnlyWorks` already says (it has
    a sandbox there).
11. **One job per tool** on Windows where C# has one for all; both kill on close.

### Question, answered (4A: as now)

4. **An agent's process on Linux when Hover is suspended** (`kill -STOP`, or a laptop
   lid with `systemd` freezing the session): nothing special is done; the tool keeps its
   turn going.
   - **A.** As now.
   - **B.** Stop the tools' groups with Hover (`SIGSTOP`/`SIGCONT` follow Hover's).

### Pending on Windows (RUN-ON-WINDOWS.md)

- A real turn with kiro-cli, Codex and Cursor (the Cursor `.cmd` through `cmd`).
- Hover killed from Task Manager mid-turn: the tool and what it started go (job object).
- The console window never flashes (`CREATE_NO_WINDOW`).
- Non-ASCII prompts reach each tool intact over stdin.

### Costs of what's left in 3A

Nothing in code. The notch alert and tray balloon on a turn's end (A10) come with 3C.

## 3C: the product (`native/apps/hover`, `native/crates/hover-quota`)

**Status: built and run on Linux (X11 under Xvfb, and XWayland inside headless
weston); the Windows half type-checks for MSVC and its checks are pending.**

| C# | Rust | What it does |
|---|---|---|
| `Core/Quota.cs` | `hover-quota` (`lib.rs`, `read.rs`, `sqlite.rs`, `num.rs`) | The four readers line for line: kiro-cli's report (25 s deadline for the exit and both pipes, the tree killed), Codex's newest `rollout-*.jsonl`, Cursor's token from `state.vscdb` (SQLite read-only, 2 s lock wait; built in on Linux, `winsqlite3.dll` on Windows) then `usage-summary`, Claude Code's `.credentials.json` then `api/oauth/usage`, never refreshed. .NET's `"0"`/`"0.##"` rounding (15 digits, half away from zero) and `d MMM HH:mm` in local time. |
| `OwlApp` quotas | `schedule.rs` | Off until switched on; read five minutes after the last read (30 s tick), at once when forced; a quota switched off loses its reading; one read per quota at a time, each on a thread. |
| `Core/Palette.cs`, `Owl/Theme.cs` | `hover-core::palette`, `platform::look` | Hover's light and dark, `From` a VS Code theme, JSONC files with `include` (depth 4), the installed-theme finder, the colour arithmetic (Math.Round's half-to-even). System follows the platform: Windows' `AppsUseLightTheme` and "Animation effects"; on Linux the settings portal (`color-scheme`, GNOME's `enable-animations`), else gsettings, `kdeglobals`, GTK's `settings.ini`; changes watched. |
| `Owl/OwlApp.cs`, `App.xaml.cs` | `apps/hover/src/app.rs`, `main.rs` | One owner of the settings, key, history, the three ACP hosts and the sessions. Ends announced (notch alert 8 s + notification) unless an office is in view, counted until seen. Single instance (a second launch opens the app window). Quit, the tray's Quit, SIGTERM or SIGINT: sessions stopped, tools shut down, history and settings flushed. |
| `Owl/Pages.cs`, `Themes/Owl.xaml` | `pages.rs` (the page as data), `ui/settings.slint`, `ui/widgets.slint` | The five sections row for row with the C#'s words and automation ids (`accessible-id`, which AccessKit gives UIA as AutomationId and AT-SPI as its id); switch, segmented control with its sliding thumb, pills, pickers and their menu, theme tiles, the shortcut recorder (Esc stops, a modifier is required, the chord is kept as WPF's Key). |
| `Owl/Notch.cs`, `Owl/Bot.cs` | `notch.rs`, `rest.rs`, `ui/app.slint`, `ui/bot.slint` | Rest (nothing, the pill, the alert), quota rings coloured by level, the working bot with the status words rising in (260 ms) and the dots, the done bot's hop (0.9 s), "Welcome back" on the first opening and after resume/unlock (160/560/940 ms keyframes), peek and open, Esc and click-away, the four office sizes. The office itself is a stand-in until Phase 2. |
| `Interop/HotKeys.cs` | `win.rs` (`RegisterHotKey`), `x11.rs` (`XGrabKey` with and without Caps/Num Lock) | Rebindable; refused chords warned about once per chord (App.ShowHotKeyWarning). |
| `Services/TrayIcon.cs`, `Actions.cs` | `win.rs` (`Shell_NotifyIcon`, a popup menu, the balloon), `sni.rs` | Linux: StatusNotifierItem with its menu over `com.canonical.dbusmenu`, notifications over `org.freedesktop.Notifications`. The menu is BuildMainMenu's seven entries. |
| `KiroPage`'s beats | `music.rs` | `office-beats.ogg` decoded in Rust (lewton), played through WASAPI/ALSA (cpal): off until switched on, remembered (`office.json`), the page's fade (16 frames up to 0.32, 11 down), silent while no office is in view, the device let go when silent. |
| `DashboardWindow` | `DashboardWindow` in `ui/app.slint` | 1200×620, min 880×480; on Windows the title bar takes the panel's colour (DWM). |

New crates, each with its reason in `Cargo.toml`: `libsqlite3-sys` (Cursor's database), `lewton` and `cpal` (the music).

### Evidence

`cargo test --release --workspace`: **145 passed** (103 before 3C). MSVC `cargo check --workspace --all-targets`: green. `cargo clippy -p hover -p hover-quota -p hover-core --all-targets`: no warnings.

| Test | Against |
|---|---|
| `hover-quota/tests/quota.rs` (15) | `LayoutAndQuotaTests.cs` (QuotaTests) case for case; plus the readers against stand-ins: a local HTTP server checks the headers and every refusal's words, a real SQLite file holds Cursor's token (text and UTF-16 blob), a script stands in for kiro-cli including a grandchild holding stdout past the deadline. Run in four time zones. |
| `hover-quota` unit (3) | .NET's custom-format rounding; OwlApp's five-minute book |
| `hover-core::palette` (5), `tests/look.rs` | Palette.From's values worked out line by line; Read with includes and a loop; the installed-theme finder over fake extension folders (nls labels, fillers, newest version, a broken manifest); a stand-in settings portal on a private bus answering `ReadOne` and sending `SettingChanged` |
| `apps/hover` lib (12) | OwlApp's end announcements and unseen count, quota switching, the pill's model, Pages' ids and words from SCREENS.md and Pages.cs, the shortcut recorder, VK and keysym tables, the music's fade and loop |
| `apps/hover/tests/tray.rs` | a stand-in panel on a private bus: registers the item, reads its title, icon sizes and the menu (labels, separators, Launch at Login's tick), clicks an item and the icon; a notification with its 6 s timeout |
| `hover --selftest` on X11 | **13/13** under Xvfb and under XWayland in headless weston: placement on the primary work area, override-redirect + dock + 32-bit visual, the notch never takes focus at rest, the input shape is the pill (then the open panel), a click on the empty part reaches the window below, hover peeks without focus and folds after the leave grace, Alt+N opens with the keyboard, Esc folds and gives the focus back, a click on the pill opens, a chord another client holds is refused, **0 frames drawn in 3 s at rest**. [report](shots/x11-selftest-report.json) |
| second launch, Linux | the second process exits at once (7 ms); the first logs "another launch: opening the app window" |

Screenshots (`shots/`): on the X11 screen [the pill](shots/x11-rest-pill.png), [peek](shots/x11-peek.png), [open](shots/x11-open.png), [Settings over the notch](shots/x11-open-settings.png) (Xvfb has no compositor, so the transparent part shows black); headless (software renderer) [pill with rings and a task at work](shots/notch-rest-pill-2x.png), [alert](shots/notch-rest-alert-2x.png), [unseen ends](shots/notch-rest-done-2x.png), the greeting at [120](shots/notch-greeting-120ms.png)/[400](shots/notch-greeting-400ms.png)/[700 ms](shots/notch-greeting-700ms.png), [Settings in the notch](shots/notch-open-settings.png), each section in the app window dark and light (`settings-*-dark.png`, `settings-*-light.png`), [a VS Code theme](shots/settings-general-vscode-dark-plus.png), [the model picker](shots/settings-kiro-model-menu.png).

Fonts: Inter and Inter Display, embedded as in the C# app (not Segoe UI: the C# app draws Settings and the notch in Inter too); fontconfig resolves `sans-serif` to DejaVu Sans here, used only as a fallback for glyphs Inter lacks.

### Where each quota lives on Linux

| Quota | Windows (C#) | Linux | Seen here |
|---|---|---|---|
| Claude Code | `%USERPROFILE%\.claude\.credentials.json`, `CLAUDE_CONFIG_DIR` | `~/.claude/.credentials.json`, the same variable | not signed in; tested with a stand-in |
| Kiro CLI | `kiro-cli` on PATH | PATH, then `~/.local/bin` | not installed; tested with a script |
| Codex | `~/.codex/sessions`, `CODEX_HOME` | the same | tested with a fixture folder |
| Cursor | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` | `$XDG_CONFIG_HOME/Cursor/…/state.vscdb` (`~/.config`) | tested with a real SQLite file |

### Differences from the C# app

Fixed or adapted (listed, not asked):
12. **Linux words**: "starts when you log in", "Win" → "Super", "the system" for Windows in Appearance, "computer" for PC; Codex's read only is offered (3A, 10).
13. **Month names** in quota details are English (`26 Sep`); C# used the current culture's. Only non-English Windows shows the difference.
14. **Installed themes sort** by lower-cased label, where C# used the culture's comparer; the same for ASCII names.
15. **System's dark mode on Windows** is re-read every second where C# listened to `UserPreferenceChanged` (no hidden message window needed); a switch shows within a second.
16. **Signals on Linux**: SIGTERM/SIGINT (logout, `kill`) quit as Quit does, flushing everything; C# had no such path.
17. **Pickers on Linux** go through zenity or kdialog (whichever the desktop has); without either, the button does nothing and says so in the log.

Nearest equivalents on Linux (asked below): the notch on Wayland; the notch's transparency without a compositor.

### Questions (answer like `5A 6B`; work continues meanwhile)

5. **The notch on native Wayland.** winit has no layer-shell, and Wayland lets no ordinary window place itself, stay on top or pass clicks through. Today Hover runs its notch through XWayland on Wayland desktops (tested inside weston: 13/13); GNOME and KDE both ship XWayland.
   - **A.** Keep XWayland (now). One caveat: on GNOME Wayland, XGrabKey only sees keys while an X window has focus, so Alt+N must come from the portal (C below).
   - **B.** Add a native path through `wlr-layer-shell` (sway, Hyprland, KDE; not GNOME) with smithay-client-toolkit, XWayland elsewhere. About 3–4 days.
   - **C.** A, and take the shortcut from the GlobalShortcuts portal on Wayland (GNOME 48+, KDE 6); about 1 day.
6. **No compositor (bare X11, no picom):** the transparent part of the notch shows black.
   - **A.** Leave it (every desktop environment composites).
   - **B.** Detect it (`_NET_WM_CM_S0` unowned) and shrink the window to the shape while resting (the window resizes then, which C# avoided on Windows because it blinked).

### Pending on Windows (RUN-ON-WINDOWS.md, 3C)

The notch self-test in the product (not only notch-proto), the tray icon, menu and balloon, the rebindable hotkey and its conflict box, the dashboard's caption colours, Settings' pickers, the music through WASAPI, the four quotas at their Windows paths with real sign-ins, DPI 100–200 % and a second monitor, UIA ids with Accessibility Insights.

### Costs of what's left in 3C

The Windows run: about half a day. Questions 5 and 6 if B: 1–4 days.
