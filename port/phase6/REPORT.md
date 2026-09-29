# Phase 6: every feature end to end on Linux, and what that found

The earlier phases tested the parts: goldens, replays, the notch self-test, headless
shots. This pass ran the real `hover` under Xvfb with real X input and clicked through
every feature a user reaches, the way a user does. It found eleven bugs that none of
those tests could see, all in the app's UI layer. They are fixed, and the run is kept as
a suite: `port/e2e/`.

## How it runs

`port/e2e/run.sh` runs eleven scenarios, 55 checks. Each scenario:

- starts Xvfb, an AT-SPI bus and `native/target/release/hover` on a fresh data folder;
- uses `fake-agent.py` in place of kiro-cli, codex-acp and cursor-agent. It answers
  ACP (config options, `session/request_permission`, diffs, command output, errors, a
  crash, cancel, load) and logs every message Hover sends;
- finds what to click through AT-SPI (the accessible ids) and presses it with XTEST.
  It never uses the accessible actions, because those bypass the bugs below.

The X server has no window manager, so the harness does one's job: it gives the
keyboard to an ordinary window when it is clicked. A helper window stands in for the app
the user was in.

| Scenario | What it covers | Checks |
|---|---|---|
| sA | A task: read, edit, run, Markdown, Mermaid; the chat opens from the bot's name | 3 |
| sB | Approvals: over the head (Run), the island and card (Enter allows, Esc denies, the keyboard goes back), a reply that denies | 10 |
| sC | Stop, a queued reply dropped by Stop, an ACP error, a crashed tool starting again | 5 |
| sD | Codex's and Cursor's modes for each tool access | 2 |
| sE | Three running at most, Esc in the task box, the HUD menu, history, delete with the confirm | 6 |
| sF | History after a restart, the transcript, a reply waking it with `session/load` | 5 |
| sH | Settings by mouse: switches, launch at login, office size, the shortcut recorder and the new shortcut | 7 |
| sI | Kiro and Codex quotas in Integrations and the island, Appearance, a second launch opening the app window | 5 |
| sJ | The app window: a task from it, maximize, Close hides it | 4 |
| sK | The Worked line opening the steps, Esc in Settings and after Back, the office dropped after 30 s and restored with its chat | 6 |
| sL | Copy on a code block (read back from the X clipboard), a link opening in the browser | 2 |

Result: **55/55**. Also green: `cargo test --release --workspace` (169),
`cargo check --target x86_64-pc-windows-msvc`, and the notch self-test (13/13). The 31
headless shots were checked by eye. The Slint warnings about `maximized` and `close`
shadowing built-in names are gone.

## Bugs found and fixed

| # | What the user saw | Cause | Fix |
|---|---|---|---|
| 1 | Clicking a bot's name or the Run/Deny/Trust buttons over its head did nothing | The tags' model was made again with every office frame (10–30 fps), so a press never became a click | `view::sync` changes a shown model row by row. Used for the tags, the panel rows, the tool logos, the access menu, the island's quotas and stack |
| 2 | A click in a peeked office didn't keep it open; moving away folded it | Notch.cs's PreviewMouseDown was only on the shape's TouchArea, which the office covers | A winit event hook turns a peek into an open on any press |
| 3 | Start stayed greyed while typing a task when the folder was already chosen | `new-go` was worked out only when something else refreshed the box | The task field reports each edit |
| 4 | Esc did nothing after starting a task or closing the chat | The focused text field was gone, so nothing had the keyboard | The office takes the keyboard back when the box, chat, panel, menu, confirm or Settings go |
| 5 | Esc in the task box, a reply or the search closed the whole notch | The Esc chain lived on a FocusScope that isn't those fields' parent | `OfficeView.escape()` holds the chain. The notch's and the app window's key scopes call it |
| 6 | Pressing, dragging or scrolling on a panel's empty part, or on the greyed Start, reached the scene behind it (the box closed, the camera panned, a bot got picked) | Glass and graphite panels took no pointer input | Each has a TouchArea under its content |
| 7 | The history, board and overview panels put their title halfway down, with a gap above the list | A Flickable is capped at its content's height, so the header took the spare room | The list sits in a box that stretches |
| 8 | Every Settings switch and button (and the confirm's Delete) needed two clicks | Each button's FocusScope sat over its TouchArea and swallowed the press to take focus | `focus-on-click: false`; the click gives focus |
| 9 | The shortcut recorder never recorded | Starting to record rebuilt the page, and the field holding the keyboard went with it. Separately, the Switch and Segments kept a local copy of their value, which a reused row would carry over | `view::sync_blocks` updates Settings in place, down to its rows. Switch and Segments show the model's value |
| 10 | A reply's answer landed below the visible part of the chat | The drawer didn't keep to the bottom (C3) | Within 40 px of the bottom it stays there, and a new answer jumps to it, as `renderDrawer` does |
| 11 | "1 sessions in the office" (the office's accessible name) | Plural always | Singular for one |

Also: the app window's `maximized`/`close` became `is-maximized`/`close-clicked`, and
the maximize glyph follows the window when the desktop maximizes it. The bench channel
gained `state`, for the harness.

## Checked and left as they are (parity or environment)

- The notch's ears are light while it is open in light mode. Notch.cs fades its fill to
  `Ui.Panel` too.
- "Couldn't finish" reads like "Couldn't Anish" in the tags. That is Pixelify Sans's own
  `fi` ligature, which Chromium also applies.
- The question card over a bot at the door runs off the top. The page doesn't clamp it
  either.
- The General footnote still says the app window opens "from a click on its name in the
  notch". That is main's Pages.cs text.
- Chill beats turns itself back off here: the sandbox has no audio device, and the port
  handles that as the page does when `play()` fails.
- With no compositor, the notch's transparent area is black over other windows
  (question 6). With no tray host and no notification daemon, both are only logged. Their
  own tests (`tests/tray.rs`) cover them.

## Still not done (from Phase 2's list)

Text selection in the drawer, images in the chat and the task box, the model pill and
menu, the bot's halo light. None of the scenarios needs them. They remain the office's
gaps.

## Not verified

- Anything on Windows, including bugs 2 and 8 there. The fixes are Slint and winit code
  shared by both platforms.
- Maximize with a real window manager.

Pictures are in `shots/`:

- `e2e-chat-from-tag`
- `e2e-ask-over-head`
- `e2e-ask-card`
- `e2e-queued-reply`
- `e2e-history-panel` (bug 7)
- `e2e-woken-chat-follows` (bug 10)
- `e2e-rest-quotas`
- `e2e-copy-and-link`
