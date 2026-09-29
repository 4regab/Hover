# Phase 5: main's 2.x UI/UX changes, ported

main moved on after the port branched (59bac1f). Its four commits are ported here:

- 93bbd64: notch island, provider logos, live activity, approvals.
- f3dd499: new chat, safer task start, the app window's own title bar.
- 661ba29: even space at both ends of the notch.
- 639c01c: simpler office controls, Codex approvals, fresh Codex quota.

The office page itself (`web/office/main.js`, `page.html`) came over from main unchanged, and `native/golden/page/kiro-office.html` is its build. The goldens were regenerated from it.

## What was ported

| main | Rust |
|---|---|
| `AgentApproval`, `AgentOptions.WithAccess`/`AccessId`, `KiroApproval` in settings.json, `SavedSession.Access` | `hover-core` model, settings, history |
| `KiroStep` diff, output, exit code, time; `DiffOf`, `OutputOf` | `hover-agents::stream` |
| `AcpHost.Permission`, `NeedsAsking`, `Describe`, Trust kept by Hover, Codex's new modes | `hover-agents::acp`, `ask` |
| `KiroSession.Ask`/`Answer`/`DenyAll`, `AgentWords` | `hover-agents::session`, `words` |
| Codex quota ranked by each log's last event | `hover-quota::read` |
| KiroPage's state: `access`, `readOnly`, `waiting`, `ask`, step rows as objects; `answer` and `new` with `access` | `hover-agents::state`, `apps/hover/src/office_ui.rs` |
| The office: the waiting pose, the question over the head and in the chat, one HUD menu, smaller new-task circle, the access chip and menu, graphite drawer and panel, new history rows, the composer | `hover-office`, `apps/hover/ui/office.slint` |
| The chat: the Worked/Working line, the timeline, reads folded, diffs and output, files changed, code blocks with Copy | `hover-chat::doc`, `state` |
| The notch island: marks, rings, stack, the question island and card, the ended island and glow, the spring, the cross-fade, open and close timing, no greeting or alert | `marks.slint`, `app.slint`, `rest.rs`, `main.rs`, `hover-notch` |
| Settings → Tool access (Full, Ask first, Ask always, Read only) | `pages.rs`, `view.rs` |
| The app window's title bar | `app.slint` DashboardWindow, `main.rs` |

## Evidence

- `cargo test --release --workspace`: 169 passed. That includes main's new tests (SettingsTests; KiroRunnerTests: approvals, trust, modes, deny, withdraw; KiroSessionTests: questions, AgentWords), and tests for the step details, rest sizes and springs.
- `cargo check --target x86_64-pc-windows-msvc`: green.
- The chat's copy text, timeline row positions, code-block sizes and broken images match the new page in Chromium. The goldens were regenerated with `gen-copy.mjs`, `gen-scroll.mjs` and `gen-broken.mjs`, adapted to the new DOM.
- The office state message is the fixture's bytes. The fixture was converted to the new shape and given a diff, command output and exit codes.
- Notch self-test under Xvfb: 13/13.
- Screenshots are in `shots/`: the island (`notch-rest-*`), the office (`office-*`), and the app window.

## Differences from main

1. **The question in the chat** sits over the composer while the agent waits. In the page it sits at the end of the thread and scrolls with it. The thread here is one painted image, and a card inside it would need its own buttons in the painter.
2. **The notch card's "more" menu** (Trust for the session / Allow everything) is two buttons, Trust and All, not a chevron with a menu.
3. **The cross-fade** fades the new content in; the page also fades a snapshot of the old content out.
4. **Turn times** (`.me .when`) are written as en-US writes them ("1:47 PM"), as WebView2 with an English UI does. The page uses the system's locale.
5. **Resizing the app window** uses winit's drag-resize on a 6 px border. The C# keeps the system frame's own through WindowChrome.

## Not verified

Nothing here has run on Windows. On Windows, check:

- the title bar's drag, snap and maximize;
- the island on a real display at 100–200 %;
- the question card taking the keyboard and giving it back;
- the approvals against real kiro-cli, codex-acp and cursor-agent.
