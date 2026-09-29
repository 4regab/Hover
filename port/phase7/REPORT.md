# Phase 7: OpenCode (main's 55111fc), end to end again, and the office against main's page

Main's last commit, 55111fc "Add OpenCode as a fifth agent in the office", ported line by
line from `src/Hover/Services/OpenCodeHost.cs` and the rest of that diff, then every
feature run end to end again under Xvfb, and the office compared with main's page
(`src/Hover/Assets/kiro-office.html` at 55111fc, in Chromium at the notch's size). The
page in `native/golden/page/` and `web/office/` are 55111fc's now.

## Evidence

- `cargo test --release --workspace`: **193 passed**, 0 failed (169 before). New:
  `hover-agents/tests/opencode_host.rs` (15, OpenCodeHostTests.cs against a stand-in
  `opencode serve` in Rust: routes, Basic auth, a chunked event stream), unit tests in
  `opencode.rs`, `http.rs`, `agents.rs`, the OpenCode Settings page, `AcpChoice.Levels`
  in settings.json, OpenCode keeping its agent.
- `port/e2e/run.sh`: **79/79** (55 + the new sM's 24). sM runs the real `hover` against
  `fake-opencode.py` (the same routes and events) with real X input: the first-use note,
  OpenCode in the picker, a task, the model pill's menu with the model's own variants
  (the next prompt carries `model` and `variant`), a question answered in the chat, one
  answered from the notch's Review with a reply in one's own words, an approval
  (`once`), Stop (`abort`), and the Settings page.
- **Against the real OpenCode 1.18.33** (`npm i -g opencode-ai`, its free
  `opencode/big-pickle`), by hand with `cargo run -p hover-agents --example opencode_live`:
  an edit (hello.txt written, 8 s, context 4.1 %), its question tool (Tabs picked, then
  written to answer.txt), and read only (`rm hello.txt` refused, the file kept, the answer
  marked as refused).
- `cargo check --target x86_64-pc-windows-msvc --workspace --all-targets`: green. Clippy:
  nothing new.
- Pictures: `shots/` (the port, headless and under Xvfb), `main/` (main's page).

## What was ported

| C# (55111fc) | Rust |
|---|---|
| `AgentTool.OpenCode`, `Agents` (exe with the npm shim, `serve` args, hints, the version floor 1.14.19) | `hover-core::model`, `hover-agents::agents` (`parse_version` is System.Version's) |
| `AcpChoice.Levels`, `IAgentRuntime`, `AgentCaps` | `model::AcpChoice.levels` (written as `"Levels"` in settings.json), `runtime::{Runtime, caps}` |
| `OpenCodeHost` (1165 lines) | `opencode.rs`, threads where C# awaits; `http.rs` is the HttpClient (loopback only) |
| `AgentAsk.Questions`, `AgentQuestion`, `KiroSession.AskQuestion/AnswerQuestion`, `AgentWords` | `ask.rs`, `session.rs` (`ask_question`, `answer_question`), `words.rs` |
| `OwlApp` (Questioning to the session) | `app.rs` |
| KiroPage: models with levels, `effortLabel`, `questions`, `ask.questions`, `answer` with answers, `reveal` | `state.rs`, `office_ui.rs` |
| main.js/page.html: TOOLS/LOGOS, questionHTML, sendAnswers, effortsOf, renderPill/openMenu, the reply that answers | `office.slint` (AskCard's question form, MPill, #mMenu), `office_ui.rs` |
| Marks (OpenCode's mark), Notch (Skip; Review opens the chat) | `marks.slint`, `app.slint`, `main.rs` |
| Pages (the OpenCode section: models, Variant, Agent, access words, footnote) | `pages.rs`, `view.rs` |
| Settings.SetAgentOptions keeps OpenCode's agent | `settings.rs` |

## Found and fixed

Beyond OpenCode itself, comparing the office with main's page and running it end to end
turned up these, all in the port's UI:

| # | What differed from main | Fix |
|---|---|---|
| 1 | The bots' bubbles were pastel with dark Pixelify text; main's are dark (`rgba(14,10,18,.9)`), white Inter 12px, a done one green, a failed one red | `.bub`'s colours, borders, font and size |
| 2 | The office's logos had no Kiro eyes and no Codex prompt (truncated paths), and a greyed tool faded instead of turning grey | The marks' full paths; `.lg.off` as grayscale(1) brightness(.55) |
| 3 | Under femtovg (the real renderer) the marks' small holes vanished: Kiro's eyes, Codex's prompt, in the office and the notch | Those parts drawn again on top in the tile's colour |
| 4 | The notch opens the office 464 px tall, so main's `@media (max-height:620px)` applies: drawer and panel 8 px in and 360 wide, radius 16, the HUD 32 px at 10 px, the fab at 10 px, smaller header and buttons, the toast at 10 px | `OfficeView.compact` |
| 5 | The composer was a text field with a send button beside it; main's is the text, then a bar: +, the model pill, Send | The composer's layout, `ClipButton`, `GoButton` |
| 6 | The model pill and its menu were missing (a Phase 2 gap) | `MPill` in the reply and the task box, `#mMenu` with models, efforts (or the model's variants) and the note |
| 7 | + (attach an image) was missing in both boxes (a Phase 2 gap) | + picks a PNG, JPEG, GIF or WebP (zenity/kdialog, IFileOpenDialog), kept in kiro-images as a pasted one is, shown as `.shots` with ×, sent with the task or reply |
| 8 | The note before the first task never showed (A9 was left for Phase 2) | KiroPage.Notice in place of the office until Got it |
| 9 | The task box: the note sat above the bar in amber, no placeholder, no fold button of its own size | Main's order and colours (the note under the bar, faint), the placeholder, `nFold` |
| 10 | The composer's Default model sent `""` as the model: C# keeps `Str(m, "model")` as given, and OpenCode would then look for a model called "" | Default clears the model (an obvious bug, fixed here only) |

## Differences kept, and why

- The question and the approval in the chat stay pinned over the composer, where
  Phase 5 put them; the page puts them at the end of the thread, which it scrolls to.
  The chat thread is hover-chat's drawing, which holds no interactive controls.
- Pictures in the chat's own messages still don't show (the thread's images; Phase 2's
  gap), nor does pasting an image into a field; + covers attaching one.
- OpenCode on Linux is also looked for in `~/.opencode/bin`, where its installer puts
  it and a desktop session's PATH often doesn't reach (as `~/.local/bin` for the others).
- `examples/opencode_live.rs` stands in for OpenCodeLiveTests.cs (run by hand, as the C#
  one is).

## Not verified

- Anything on Windows: the npm shim's exe, the job holding `opencode serve`, the image
  picker's IFileOpenDialog. They are in the MSVC check only.
- A real OpenCode approval through the UI (the live run allowed it from the example;
  the UI's approvals ran against the stand-in).
