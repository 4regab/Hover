# Handoff

Delete this file in your first commit once you have read it.

## Where things are

- Branch `rust-port/restructure`, pushed. It also fast-forwards the open PR #5
  branch `rust-port/phase-0-1`.
- The workspace now lives at the repo root (not `native/`): `app/`, `crates/`,
  `tools/`, `tests/golden/`, `packaging/`. `evidence/voice-chat/CONTEXT.md`
  (git-ignored) still uses the old `native/` paths.
- CI: `.github/workflows/ci.yml` only (jobs `windows`, `linux`, and `release` on
  `v*` tags). Test on GitHub Actions, not with long local runs.

## Pushed last, not built or tested

The user asked to push without a check. Run CI first and fix what breaks.

1. `app/ui/office.slint`: the model pill shows only the model. The effort is
   picked in the menu.
2. `app/src/main.rs`: an end counts as seen only when the notch is open or the
   app window has the focus. A window behind others no longer hides failures
   (`watching_changed`, `focused`, the `E::Focused` event).
3. `crates/hover-agents/src/{stream.rs,words.rs}`, `tests/words.rs`: activity
   words. MCP calls (`@server/tool`, `MCP: tool`) show as "Using server: tool".
   `tool_phase` matches whole words, so "playwriter" no longer reads as Editing.
   `hover-data steps <data> [n]` prints real step titles (read only).
4. Voice default agent (`projects.rs` `VoiceSettings.agent`, JSON key `Agent`;
   `pages.rs` picker `VoiceAgentTool`; `view.rs`; `voice/mod.rs` `change_agent`,
   `hold`, `ready_tools`; `voice_ui.rs`; `app.slint`). The notch preview card has
   an agent menu and a model/effort menu. Opening a menu stops the countdown, and
   Start is then needed. New tests are in `voice/tests.rs`.
5. `crates/hover-chat/src/paint.rs`: test `regular_after_bold_is_regular` only.
   No fix yet. It may fail on CI. It is meant to catch the "random bold" glyphs
   (suspected: the glyph cache key ignores variable font axis coordinates).

## Still to do (user requests)

- Bold glyphs in the chat: find the root cause in `paint.rs` and fix it. The
  test above should then pass.
- Thoughts and tool runs fold while the turn runs. Keep them open during the
  turn, fold after it, and respect the user's own toggle (`hover-chat/src/doc.rs`).
- Remove the Copy button under the user's own messages. Keep it on answers.
- MCP startup failure must not end the turn. `acp.rs` ~253 `mcp_failed` and
  ~566 `_kiro/mcp/status` when `require_mcp`; setting `KiroRequireMcp`. Let the
  turn go on and show a notice. Check `agents.rs` for a kiro-cli flag too.
- Headless shots in CI: the user wants them gone. Measured 135 s against 995 s
  of tests, and nothing compares the images. Remove the step and its upload from
  `ci.yml`, or keep one OS only if the user agrees.
- Restructure steps left: LF line endings (CRLF only for `.ps1`/`.iss`) with
  `.git-blame-ignore-revs`; split `app/src` into modules; rename the `Kiro*`
  types used for every agent (keep JSON and history keys). One commit and one
  CI branch (`rust-port/restructure-sN`) per step.
- App tests write to the real `%APPDATA%\Hover\hover.log`. Point them at a
  temp folder.
- Optional: Phonon install can skip `_distutils_hack`, `hf_xet`, `jinja2`,
  `markupsafe`, `networkx`, `pip`, `pygments`, `setuptools` (~35 MB). Verify a
  full transcription first.

## Rules the user set

- No destructive git commands. No images over 5 MB or 2000 px.
- Plain English in comments, commits and replies.
- End the final report with one line per steering note:
  `[STEERING <id>: <what was done>]` for steer-54b38f20, steer-f619fc7b,
  steer-fc298275, steer-c01f7098, steer-79267a9a.
