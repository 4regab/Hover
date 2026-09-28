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
