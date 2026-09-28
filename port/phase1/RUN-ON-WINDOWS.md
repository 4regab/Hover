# Running the Phase 1 prototypes on a real Windows desktop

These steps settle the Windows gates, on a real PC with a GPU. There are no CI
artifacts (the `native` workflow is manual-only), so build first:

```powershell
winget install Rustlang.Rustup   # once; then restart the shell
cd native
cargo build --release -p notch-proto -p chat-proto
cd target\release
```

The prototypes find their fonts and fixtures in the source tree when run from
`native\target\release`. Nothing gets installed.

## Notch (`notch-proto.exe`)

1. Close Hover if it is running (it owns Alt+N).
2. Run `notch-proto.exe --selftest out`. It takes about 12 s, moves the pointer and
   types. `out\report.json` lists every check as PASS or FAIL. `rest.png`, `open.png`
   and `collapsed.png` are the composed desktop behind a magenta test window.
3. Run `notch-proto.exe` on its own and check by hand:

| Check | How |
|---|---|
| Transparency over the real desktop | Wallpaper and windows show around the pill and in the pad under the open office; no black or grey rectangle. |
| Click-through | Click the pixels beside and below the pill (a browser tab, desktop icons): they reach the app underneath. |
| Non-activating rest and peek | Type in Notepad, rest the pointer on the top centre. The office peeks, and Notepad keeps the caret. |
| Composer focus | Click the task box and type. An IME (Japanese or Chinese) composes in place. |
| Activation changes at runtime | Alt+N opens with the keyboard; Esc returns focus to the app you were in. |
| Click-away | Open, then click another window: the notch folds. |
| DPI | Change the primary display scale in Settings → Display (100 % → 150 %). The notch stays centred on the work area, sharp, and the right size. |
| Multiple monitors | With two displays at different scales, the notch sits on the primary only. Swap the primary: within 2 s it moves. |
| No hidden redraws | Task Manager → Details → GPU / CPU columns: 0 % while resting. |
| Flicker | Open and close 20 times quickly: no blink, no black frame, no ghost window. |

Record the Windows build, GPU and driver, and the scale factors in the report.

## Chat (`chat-proto.exe`)

- `chat-proto.exe` shows the drawer with the rich answer. `chat-proto.exe --turns 200`
  shows a long conversation; `chat-proto.exe --stream` streams an answer.
- Drag to select from the prose into the list, the table and the code. Press Ctrl+C,
  then paste into Notepad.
- Double-click a word: it and the spaces after it are selected (WebView2's Windows
  behaviour). Triple-click: the line up to the next break. Drag after either: the
  selection grows by whole words or lines. Compare with the same clicks in Hover.
- The thread's scrollbar and the code block's sideways bar: drag the thumbs, press
  and hold the arrows and the track, Shift+wheel over the code, and hover a thumb (it
  darkens). Compare their look with Hover's office side by side (Windows 10 draws
  classic bars, 11 Fluent ones; the port draws Fluent's).
- Scrollbar width: the port assumes WebView2's thin bar is 10 px, as Chromium's is on
  Linux. Screenshot the rich session in Hover and in `chat-proto.exe`: the thread's
  lines should wrap at the same words, and the bars should be equally wide.
- Click a link: the browser opens it.
- Type in the composer, including through an IME. Enter sends; Shift+Enter adds a new line.
- Run Accessibility Insights (or Inspect.exe) over the window. The conversation should
  read as text nodes, and the composer as an edit named "Reply".

## Benchmark

Follow `port/phase0/BENCHMARK.md` with `port/bench/Measure-Hover.ps1`. The prototypes
do not contain the whole app, so their numbers can't pass G1–G4; they are recorded for
the Phase 2 comparison.
