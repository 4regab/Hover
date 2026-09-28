# Feature checklist

Every row must pass on Windows in the native build before cutover. The **Rust** column
says where the candidate stands after Phase 1:

- `proto`: a Phase 1 prototype implements it, with evidence in `port/phase1/REPORT.md`.
- `port`: logic ported and tested headlessly. Windows verification may still be pending.
- `—`: not started. It is scheduled for the phase in brackets.

Source references are to the C# app or the office page (`web/office`).

## Office scene (`web/office/main.js`)

| # | Feature | Source | Rust |
|---|---|---|---|
| O1 | Isometric voxel room: 14×11 tiles, walls, door, window, TV, coffee counter, bookcase, board, clock, painting, lounge, beanbags, plants, 6 desks with chairs/lamps | main.js ~60-230 | — (P2) |
| O2 | Materials: `MeshStandardMaterial` (roughness 0.88 voxels; visor metalness 0.35), ACES filmic tone mapping, exposure per time of day | main.js 20-30, TIMES | — (P2) |
| O3 | Lights: hemisphere, sun with PCF soft shadows (1536², bias -0.0004, normalBias 0.03), fill, 6 desk point lights, floor lamp | main.js 230-250 | — (P2) |
| O4 | Shadow map redrawn only when something moved (walks: every frame; otherwise ≤10 Hz; at once on light change) | frame() | — (P2) |
| O5 | Additive glow sprites (radial texture), window light patch + beam, dust points, steam, vacuum robot loop | main.js | — (P2) |
| O6 | Canvas textures: sky (day/night), TV stats (4 Hz), session board (kanban, empty state), LED clock (2 Hz, blinking colon) | drawSky/drawTV/drawBoard/drawClock | — (P2) |
| O7 | Time of day: auto (7–19 = day), night, day; persisted `office.time`; checked every 60 s | setTime | — (P2) |
| O8 | Bots: 6 named/coloured, walk in through the door and out, sit, poses per stage/act (waking stretch, thinking, reading, editing typing, running, done cheer, failed, stopped asleep), blinking, bulb colour/halo per stage, desk screen glow | class Bot | — (P2) |
| O9 | Camera: orthographic iso, drag to pan (6 px threshold), wheel zoom about the pointer (0.85–2.8), double-click and `0` reset, `+`/`-` keys, clamp, eased follow (k=5), focus on open bot / board / TV / history, persisted `office.view` | main.js camera | — (P2) |
| O10 | Picking: raycast bots and props (TV, board, clock, window, door, bookshelf); hover highlight (pane ×1.35, bot ring); tooltip | pick() | — (P2) |
| O11 | Name tags and bubbles over heads: typed-out text (45 chars/s), stage colours, tool badge, click opens session, hot state | frame(), spawn | — (P2) |
| O12 | Frame pacing: 30 fps when lively, 10 fps idle, 1 fps with reduced motion; paused while hidden | frame() | — (P2) |
| O13 | Reduced motion (`prefers-reduced-motion`) | `still` | — (P2) |
| O14 | Panels: session board (kanban), office overview (stats, context meters), history (search, day groups, delete) | renderPanel | — (P3C) |
| O15 | Toast (2.8 s), confirm dialog (delete), model menu, tip | main.js | — (P3C) |
| O16 | HUD: brand (opens app window; still in window mode), time segment, beats, history, settings, close (notch only) | page.html | — (P3C) |
| O17 | New-task circle: rest → pick (tool logos, greyed with hint) → open box (prompt, images, folder chip, model pill, send), draft dot, Esc/chevron/click folds, arrow keys between logos | setFab/renderNew | — (P3C) |

## Chat drawer

| # | Feature | Source | Rust |
|---|---|---|---|
| C1 | Drawer: slide-in (0.35 s cubic-bezier(.2,.9,.25,1)), header (avatar, name, tool badge, title, folder, context ring with tooltip), delete, close | renderDrawer | proto (layout only) |
| C2 | Thread: you-bubbles with images and queued note, step lists as `<details>` (live step shimmer, kept open/closed per user), status line, answer with who-line and took-time, fade-in of fresh answers | renderDrawer | proto (steps, live step, toggle; no fade) |
| C3 | Stick to bottom when within 40 px of it, or when a fresh answer arrives | renderDrawer | proto |
| C4 | Safe Markdown exactly as `md.js` (see MARKDOWN.md) | md.js | port (golden parity) |
| C5 | Mermaid flowcharts exactly as `diagram.js`, others shown as code | diagram.js | port (golden parity) |
| C6 | Selectable text across blocks, and copy | browser | proto (copy identical to Chromium on 605 cases; no double/triple click) |
| C7 | Links: http(s) only, open in the browser through Hover | md.js, `link` | proto |
| C8 | Session images: web URLs, and files inside the session folder through a per-session host; `..` and absolute paths outside rejected | imageFor | port (resolver) + proto (drawing) |
| C9 | Composer: autosize 28–112 px, Enter sends, Shift+Enter newline, paste/drop/pick images (≤4, shrunk to 2000 px, JPEG 0.9), one round button: send / queue / stop | composer | proto (text, send; images —) |
| C10 | Model pill and menu (models, efforts, arrow keys, Esc) | openMenu | — (P3C) |
| C11 | Transcript view of a history session, reply wakes it | showTranscript | — (P3C) |
| C12 | IME composition in the composer | browser | proto (Slint TextInput) |

## Agent system

| # | Feature | Source | Rust |
|---|---|---|---|
| A1 | One shared session system across Kiro, Codex, Cursor; per-tool adapters (exe lookup, args, config options, read-only mapping) | AcpHost, Agents | — (P3A) |
| A2 | ACP over stdio, newline JSON-RPC; initialize/session new/load/prompt/cancel/set_config_option | AcpHost | — (P3A) |
| A3 | Permissions: read-only refuses writes (allow read/search/fetch/think), `-32601` for other requests | AcpHost.Permission | — (P3A) |
| A4 | Concurrency: ≤3 running across tools, newest 6 kept, seats and bots lowest free | KiroSessions | — (P3A) |
| A5 | Queue replies during a run; stop drops queued replies; cancel with 8 s shutdown rule | KiroSession, AcpHost | — (P3A) |
| A6 | Idle shutdown (5/15 min), restart with `session/load` (replay muted) | AcpHost | — (P3A) |
| A7 | Sign-in/installation checks (status commands, 5-min cache, hints), MCP failure detection (Kiro) | Agents, AcpHost | — (P3A) |
| A8 | Stream reading: phases, steps, context %, answer = last message after last tool | KiroStream | — (P3A) |
| A9 | Folder first; one-time full-access notice | KiroPage | — (P3C) |
| A10 | End announcements (notch alert + tray balloon) unless the office is watched | OwlApp | — (P3C) |
| A11 | Windows Job Object (`KILL_ON_JOB_CLOSE`), all tools shut down on quit | ChildJob | — (P3A) |

## Persistence

| # | Feature | Source | Rust |
|---|---|---|---|
| P1 | `note.key`: 32 bytes, DPAPI CurrentUser, no entropy | Crypto | — (P3B) |
| P2 | AES-256-GCM, `nonce(12) ‖ ciphertext ‖ tag(16)`, UTF-8 JSON, no AAD | Crypto | — (P3B) |
| P3 | `agents/index.dat` + `agents/<key>.dat`, tmp + replace, ordered off-thread writes, 10 s flush | AgentHistory | — (P3B) |
| P4 | System.Text.Json shapes (PascalCase, enum strings, local-offset dates) | AgentHistory | — (P3B) |
| P5 | `settings.json` (indented, string enums, debounced 400 ms) | Settings | — (P3B) |
| P6 | `HOVER_DATA_DIR`, `%APPDATA%\Noty` migration, `planner.dat*` removal | Paths, OwlApp | — (P3B) |
| P7 | `kiro-images` (pasted images), swept after 14 days | KiroPage | — (P3C) |

## Integrations

| # | Feature | Source | Rust |
|---|---|---|---|
| Q1 | Claude Code: `.credentials.json`, `api/oauth/usage`, never refreshed | Quota.Claude | — (P3C) |
| Q2 | Kiro: `kiro-cli chat --no-interactive /usage` parse | Quota.Kiro | — (P3C) |
| Q3 | Codex: newest `rollout-*.jsonl` `rate_limits` | Quota.Codex | — (P3C) |
| Q4 | Cursor: `state.vscdb` token (SQLite, read-only) → `usage-summary` | Quota.Cursor | — (P3C) |
| Q5 | Off until switched on; refresh 5 min after the last read; readable failures | OwlApp | — (P3C) |
| T1 | Palette: Hover light/dark (Apple colours), System follows Windows | Palette, Theme | — (P3C) |
| T2 | VS Code theme import (JSONC, `include`), installed-theme finder (VS Code, Cursor, Kiro, Windsurf) | Palette | — (P3C) |
| M1 | `office-beats.ogg`: off until switched on, remembered, fades to 0.32, silent while hidden, needs a click first | beats | — (P3C) |

## Windows shell

| # | Feature | Source | Rust |
|---|---|---|---|
| W1 | Notch window: borderless, topmost, tool window, per-pixel transparent, click-through where empty, full size in every state | HostWindow, Notch | proto |
| W2 | `WS_EX_NOACTIVATE` at rest, removed while open; previous foreground restored on collapse | HostWindow, Notch | proto |
| W3 | Placement: primary display work area, DIP layout × monitor scale, 4 office sizes capped to work area −24 | NotchHost.Layout | proto |
| W4 | Pointer poll 50 ms; dwell 120 ms to peek; leave grace 350 ms; re-arm after collapse; click promotes peek | NotchManager | proto |
| W5 | Openness animation: 300 ms cubic ease-out open, 220 ms ease-in close; content fade; greeting keyframes | NotchShell | proto (no greeting) |
| W6 | Resting pill: quota rings, running bot + status + work dots, done bot; alert 8 s | UpdateRest | proto (glyph only) |
| W7 | Resting-notch bot glyph and work dots (30 fps only while live) | Bot.cs | proto |
| W8 | Global hotkey (default Alt+N, rebindable, conflict warning) | HotKeys | proto (fixed Alt+N) |
| W9 | Esc and click-away collapse | Notch | proto |
| W10 | Display/DPI change handling (2 s signature poll, DpiChanged) | Notch | proto (poll) |
| W11 | Single instance: `Local\HoverRunningInstance` mutex; second launch signals `Local\HoverShowApp` → dashboard | App.xaml.cs | — (P3B) |
| W12 | Dashboard window 1200×620 (min 880×480), DWM dark/caption colours | DashboardWindow | — (P3C) |
| W13 | Tray icon, menu, balloon; launch at login (HKCU Run) | TrayIcon, Actions | — (P3C) |
| W14 | Settings: General, Integrations, Kiro, Codex, Cursor (every control in SCREENS.md) | Pages.cs | — (P3C) |
| W15 | UI Automation ids (list in SCREENS.md) | all | proto (notch ids) |
| W16 | Deployment: single-file exe, Inno installer, Run key, no admin | build.ps1, Hover.iss | — (P4) |
