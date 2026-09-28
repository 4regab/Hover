# Phase 3 report: sessions, persistence, product

**Status: 3B (persistence) built and tested on Linux; its Windows checks are pending.**
3A and 3C follow in this file as they are built. Nothing here has run on Windows. The
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

Known, small, by design:
4. **An enum given as a number that names no member** (`"Appearance": 7`): .NET keeps
   the number and writes it back; the port reads the property's default.
5. **Which name .NET writes for a WPF key with two names** (Return/Enter, Prior/PageUp,
   Capital/CapsLock, the OEM keys) can't be settled from the source alone. The port
   writes the first declared and reads both. Only a shortcut on one of those keys is
   affected; a Windows check is listed.

### Questions (answer like `1A 2B`)

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
