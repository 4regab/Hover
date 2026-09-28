# Phase 3 on a real Windows PC

Build once (the Rust toolchain: `winget install Rustlang.Rustup`, then a new shell):

```powershell
cd native
cargo build --release -p hover-core --bin hover-data
dotnet build ..\port\tools\HoverFixture\HoverFixture.csproj -c Release
```

## 3B: persistence

| Check | How | Pass when |
|---|---|---|
| C# data read by the port | `dotnet ..\port\tools\HoverFixture\bin\Release\net10.0-windows10.0.17763.0\HoverFixture.dll $env:TEMP\hx\data $env:TEMP\hx\project 200 golden\fixtures\rich.md`, then `target\release\hover-data dump $env:TEMP\hx\data` | the dump lists 1 session, Kiro, Completed, 200 turns, and exits 0 (DPAPI key unwrapped, every file opened) |
| Port data read by C# | `target\release\hover-data write $env:TEMP\hr\data $env:TEMP\hr\project 50 golden\fixtures\rich.md`; stop Hover; `$env:HOVER_DATA_DIR="$env:TEMP\hr\data"; .\Hover.exe` | the office's history lists "A long rich conversation"; opening it shows 50 turns with the rich answer |
| settings.json byte for byte | In the C# app change every setting on Settings → General and one agent page; copy `settings.json`; run `hover-data dump` on the folder and compare its first block with the file (`fc /b` after saving the block) | identical (CRLF, escapes, order) |
| A shortcut on a two-named key | Bind the notch shortcut to Ctrl+Enter and Ctrl+PageUp in the C# app; read `"Key"` in settings.json | note the names written (`Return`/`Enter`, `Prior`/`PageUp`); the port writes `Return` and `Prior` (REPORT 3B, difference 5) |
| Noty migration | With no `%APPDATA%\Hover`, make `%APPDATA%\Noty\settings.json`; with `HOVER_DATA_DIR` unset, run `target\release\hover-data where` | it prints `%APPDATA%\Hover`, which now holds the file and `Noty` is gone (take a copy of your real `%APPDATA%\Hover` first, or use a test account) |
| Launch at login | Not wired to a UI until 3C: then toggle it in Settings and read `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Hover` | the value is `"<path to the exe>"`, and gone when off |
| Single instance with the C# app | Start the C# Hover; start the port (from 3C on) | the port exits and the C# dashboard opens; and the other way round |

## 3A: sessions and ACP

Build `cargo build --release -p hover-agents --example probe -p chat-proto`.

| Check | How | Pass when |
|---|---|---|
| Each tool found and checked | `target\release\examples\probe.exe C:\some\project` | each installed tool shows its exe; signed-in tools check as installed and signed in, others with the right hint |
| A real turn per tool | `probe.exe C:\some\project kiro`, then `codex`, then `cursor` | `Completed` with a one-word answer; `offers:` lists the tool's models (and efforts) |
| Cursor's shim | with Cursor installed by its installer (`%LOCALAPPDATA%\cursor-agent\cursor-agent.cmd`), run the cursor turn | it runs (through `cmd /d /c`) |
| No console window | watch the screen during the turns | no console window flashes |
| Killed Hover leaves nothing | `chat-proto.exe --acp <path to kiro-cli.exe> --folder C:\some\project`, send a long task, kill `chat-proto.exe` in Task Manager | no `kiro-cli` (or what it started) is left in Task Manager |
| Non-ASCII prompt | send `Réponds «oui» 👋` in chat-proto --acp | the tool's answer shows it read the prompt intact |
| FakeAcp live | `dotnet build ..\port\tools\FakeAcp -c Release`; `$env:FAKEACP="…\FakeAcp.exe"; cargo test --release -p hover-agents --test fakeacp` | 5 passed (the same recordings as on Linux) |

## 3C: the product

Build `cargo build --release -p hover`; run `target\release\hover.exe` with `$env:HOVER_DATA_DIR` set to a fresh folder unless a check says otherwise. Stop any C# Hover first.

| Check | How | Pass when |
|---|---|---|
| Notch at rest | start; switch on a quota in Settings → Integrations | a black pill at the top centre of the primary display; Task Manager shows no taskbar button; typing elsewhere keeps its focus |
| Click-through | click the desktop 100 px left of the pill and far below it | the click reaches what is under it |
| Peek | rest the pointer on the pill (or the top-centre strip) | opens after ~120 ms without taking focus; folds 350 ms after the pointer leaves |
| Shortcut | press Alt+N; then Esc | opens with the keyboard in it; Esc folds and the previous app has the focus again |
| Rebinding and conflict | Settings → General → Notch shortcut, press Ctrl+Alt+Delete-free chords such as Ctrl+Shift+K; then a chord another app holds (e.g. Win+L) | the new chord toggles the notch, the old one doesn't; the taken chord shows "Global shortcut unavailable" once |
| Greeting | first open after launch; lock and unlock; sleep and resume | "Welcome back" grows with the notch each time |
| Tray | left-click the tray icon; right-click it | the app window opens; the menu shows 5 items with Launch at Login ticked as set; each works |
| Balloon | run a task (`chat` in the notch is Phase 2; meanwhile `probe.exe`) and fold the notch | an alert in the notch for 8 s and a balloon naming the tool |
| Second launch | start `hover.exe` again | the second exits, the first opens its app window in the foreground |
| Dashboard caption | open the app window in dark and light | the title bar is the panel's colour with the matching text |
| Pickers | Settings → Kiro → Change…; General → Import a VS Code theme file… | the system's folder and file dialogs; a theme file applies |
| Music | the office's music button | the loop fades in; folding the notch fades it out |
| Quotas | sign in to each tool; switch each on | each row shows its percentage and detail as the C# app shows it, side by side |
| DPI | 100, 125, 150, 175, 200 %, and a second monitor at another scale | placed on the primary work area, crisp, the pill's hit area matches its shape |
| UIA | Accessibility Insights on the notch and Settings | the ids in SCREENS.md are there (`HoverNotch`, `NotchQuota*`, `NotchKiro`, `NotchAlert`, every Settings id) |
| Launch at login | toggle in Settings and in the tray | `HKCU\…\Run\Hover` holds the quoted exe path, and goes when off |
