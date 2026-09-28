# Screen-and-state checklist

A view is complete only when both implementations match under the same conditions:
Windows build, DPI, fonts, viewport, content fixture, camera and animation time. Match
means passing the screenshot comparison (region tolerances are in BENCHMARK.md §4) and
passing the interaction script, which is checked separately.

The baseline IDs in the table name the shots in `port/phase0/baseline/`. Shots marked
*(browser)* come from the built office page in headless Chromium at 1120×440 with the
page's demo data. That is the notch's Default size and the page's own renderer, but not
WebView2 in the notch. The Windows workflow repeats the WebView2 captures from the real app.

## Notch (native)

| View / state | Baseline | Interaction to record |
|---|---|---|
| Rest: nothing (0×0, 220×6 DIP wake strip) | Windows run | pointer dwell 120 ms opens peek; click passes to the window below |
| Rest: pill with 1–4 quota rings (ok / failed "—", levels <70 / <90 / ≥90) | Windows run | ring colours; ids `NotchQuota*` |
| Rest: running bot + "Kiro · Reading the code · 2" + work dots | Windows run | text fade 260 ms; id `NotchKiro` |
| Rest: done bot + "3 tasks ended" (hop 0.9 s) | Windows run | id `NotchKiroDone` |
| Rest: alert (title + text, 200–420 wide, 46 high, 8 s) | Windows run | id `NotchAlert` |
| Opening / closing mid-animation (t = 0.25, 0.5, 0.75) | Windows run | 300 ms ease-out / 220 ms ease-in |
| Greeting "Welcome back" (first open, resume, unlock) | Windows run | keyframes 160/560/940 ms |
| Peek (opened by hover) | Windows run | leave grace 350 ms; no focus taken |
| Open (hotkey or click) | Windows run | composer takes keys; Esc folds; click-away folds |
| Sizes Small 840×340, Default 1120×440, Large 1320×520, Extra large 1560×600, and capped | Windows run | Settings → Office size |
| Light and dark (fill mix after t>0.2, rim, shadow blur 24/36) | Windows run | theme switch rebuilds |
| DPI 100/125/150/175/200 %, second monitor at another DPI | Windows run | placement on the primary work area |

## Office (page)

| View / state | Baseline |
|---|---|
| Night, empty office (board "The office is quiet") | `office-night-empty` (browser) |
| Night, demo sessions in every stage | `office-night` (browser) |
| Day, demo sessions | `office-day` (browser) |
| Hover on a bot (ring, hot tag), on a prop (tip, brightened pane) | interaction |
| Camera zoomed in 2×, panned, reset | `office-zoom` (browser) |
| Board panel, overview panel, history panel (empty, list, no match) | `panel-board`, `panel-tv`, `panel-history` (browser) |
| New task: rest, draft dot, pick (logos, off tool with hint), open box (no folder, folder, disabled send, note) | `fab-pick`, `fab-open` (browser) |
| Model menu open (models, efforts) | `menu-model` (browser) |
| Confirm delete | `confirm` (browser) |
| Toast | interaction |
| Reduced motion | interaction |

## Chat drawer

| View / state | Baseline |
|---|---|
| Waking (status line) | `chat-waking` (browser) |
| Working with live step shimmer, steps open | `chat-working` (browser) |
| Done answer (Markdown), steps folded with count and time | `chat-done` (browser) |
| Failed answer (red rule) | `chat-failed` (browser) |
| Stopped | `chat-stopped` (browser) |
| Queued reply ("Queued · sends when this run ends") | interaction |
| Rich answer: headings, lists (nested, numbered with start, tasks), quote, table with alignment, code with language tag, inline code, links, image, rule, flowchart | `chat-rich` (browser, fixture `rich.md`) |
| Long conversation (200 turns) scrolled to the bottom | benchmark only |
| Composer: empty (send disabled), text (send), busy + empty (stop), busy + text (queue), images strip, drag-over ring, focus ring | interaction |
| Transcript of a history session (bot "History", reply placeholder) | interaction |
| Selection across paragraph → list → code, copied text | interaction |

## Settings (native, over the office)

| Section | Controls (automation id) |
|---|---|
| General | `LaunchAtLogin`, `HoverOpens`, `WorkspaceShortcut` (recording states), `Quit`; `WorkspaceSize{Small,Default,Large,Extra large}`; `Appearance{System,Light,Dark}`; theme tiles `Theme{name}`, `ImportTheme` |
| Integrations | `NotchItem{claude,kiro,codex,cursor}`, `QuotaStatus{id}`, `RefreshQuotas` |
| Kiro / Codex / Cursor | `{Agent}Recheck`, `{Agent}Model`, `{Agent}Effort{Level}`, `KiroAgent`, `{Agent}Tools{Full,Read only}`, `KiroRequireMcp`, `{Agent}ShowSteps`, `{Agent}Idle{5,15} min`, `SettingsKiroFolder`, `KiroNoticeAgain` |
| Chrome | `SectionGeneral`, `SectionIntegrations`, `SectionKiro`, `SectionCodex`, `SectionCursor`, `BackToOffice`, `Close` |

Each control also needs its hover, pressed, focused (focus ring 2.5 px, radius 11) and
disabled states.

## Other native views

| View | Automation id |
|---|---|
| First-visit notice ("Before an agent starts", Got it) | `KiroNotice`, `KiroNoticeOk` |
| WebView2 missing card. The native port has no WebView2, so this view goes away and is replaced by nothing. That change needs the user's approval. | `KiroNoWebView` |
| Dashboard window | `HoverDashboard` |
| Notch window | `HoverNotch` |
| Tray menu (5 items, checkable Launch at Login) | — |
| Hotkey-conflict message box | — |
