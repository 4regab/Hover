# Changelog

What changed in each Hover release. Versions follow [Semantic Versioning](https://semver.org).
3.x is the native (Rust) Hover; 2.x was the .NET app.

## [Unreleased]

## [3.4.0] - 2026-10-02

### Added

- Checkpoints in the chat. Hover keeps the project folder before and after every turn (in a
  Git store of its own in the data folder; your project's own Git is never touched, and
  what its .gitignore leaves out is left out). Under an answer, **Restore** puts the files
  and the chat back to just after that answer, and **Try again** puts them back to before
  that message and sends it again. Both ask first and only work while nothing runs, and the
  agent is told once that its folder and chat went back. Needs Git installed; deleting a
  session deletes its checkpoints.

### Fixed

- The model menu in the office showed only the first models of a long list (Codex's can
  have a dozen or more), with no sign of the rest: the last ones, and the effort choices
  under them, were cut off. The models now scroll inside a box of their own with a bar, the
  menu opens on the picked model, and the effort choices and the note stay in view.

## [3.3.1] - 2026-10-02

### Fixed

- A long prompt is all reachable in the new-task box and the chat's reply box: the box grows
  to its height, then scrolls inside (the wheel, or the caret kept in view), with a thin bar
  showing there is more. Dictated words land at the end, in view.

## [3.3.0] - 2026-10-02

### Added

- Voice dictation into a chat: with the chat's reply box open and the pointer over the chat,
  the voice shortcut writes what you say into the reply instead of starting a task.
- Settings → Voice → Start on its own: how long the voice card counts down before it starts
  the task (Off, 3, 5 or 10 s). It is 5 s now, up from a fixed 3 s.

### Fixed

- The chat shows a tool's whole command, a size smaller, wrapped when it is long, instead of
  its program and first word.
- The notch's office has smooth corners: on a light desktop its edge showed white steps.
- Settings' section list scrolls when the office is too short for it (a Small office hid
  Claude Code).
- A long folder name in the chat's header no longer pushes Delete and Close out of it.
- CI's tests build about a third faster (a ci profile without LTO), and pull requests reuse
  main's build cache.

## [3.2.0] - 2026-10-02

### Added

- Claude Code as a fifth agent in the office, run as T3 Code runs it: the `claude` CLI in its
  Agent SDK mode, one hidden process per conversation in its folder. Full, Ask first, Ask
  always and Read only all hold; its questions (AskUserQuestion) show over the bot, in the chat
  and in the notch; Stop interrupts it; a reply after it went idle picks the conversation back
  up. Settings has a Claude Code page with its own models and each model's efforts.

### Changed

- The open office is lighter on Windows: its frames stay on the GPU instead of being
  copied through the CPU each frame.
- Voice's one-line notch cards are only as wide as their text.
- Releases are now the Latest release on GitHub, and the Rust app is the repo's `main`
  branch (2.x is on `dotnet`).

### Fixed

- HTTPS on Windows uses the system's certificates, so Groq, chat images, the Phonon
  download and the quota reads no longer fail with "unable to find any user-specified
  roots".
- The office overview's note no longer pushes the Close button off the panel.
- The notch says "Thinking" for a thinking agent, not "Working on Thinking".

## [3.1.1] - 2026-10-02

### Fixed

- Regular text in the chat no longer shows some letters in bold.
- Thoughts and tool runs stay open while the agent works and fold when it is done; your
  own fold or unfold is kept. The Copy button is gone from your own messages.
- A Kiro MCP server that doesn't start no longer ends the task; the chat says which one.
  The "Require MCP servers" switch is gone.
- The office is much lighter on a computer with no GPU (a VM or a remote desktop): it
  draws at half size there, and never asks for frames faster than the computer makes them.
- Hover's own icon on the exe, its shortcuts and the taskbar, instead of Windows' blank one.
- A session deleted just as its task ended could come back in the history.

## [3.1.0] - 2026-10-01

### Added

- Voice: hold `Ctrl+Alt+Space`, say a task, let go. Speech is turned into text by Groq
  (Cloud) or by Phonon on your computer (Local, English only, downloaded from Settings).
  The task is matched to a registered project or the default workspace, shown for three
  seconds, then started as a new chat.
- Settings → Projects: the folders voice may work in, their other names, and each one's
  tool access. A new project asks first; it never gets Full on its own.
- Settings → Voice: speech mode, microphone, shortcut, Groq key, the Phonon download,
  optional cleanup (Gemini, OpenAI or a custom service), and Try it.
- API keys are sealed in `secrets.dat`, never kept in plain text.
- Chat: the thinking and subagents the agent reports, diffs with line numbers, command
  output with exit codes, colour-coded code, changed files under the answer, Copy and
  Retry.
- Chat: replies sent during a run are queued, each with Cancel. Pause stops the current
  answer, and the next queued reply starts once the agent confirms.

### Changed

- Typing a reply no longer denies a pending request.
- The office's Menu button has a tooltip.
- Releases are built and published on GitHub Actions.

### Fixed

- A long model name no longer pushes Start out of the new-task box.

## [3.0.0] - 2026-09-30

### Changed

- Hover is now a native app in Rust and Slint, for Windows and Linux. It reads
  everything 2.x left: the data folder, the key, `settings.json` and the sessions.
- The Windows executable is `hoverai.exe`.

[Unreleased]: https://github.com/4regab/Hover/compare/v3.4.0...HEAD
[3.4.0]: https://github.com/4regab/Hover/compare/v3.3.1...v3.4.0
[3.3.1]: https://github.com/4regab/Hover/compare/v3.3.0...v3.3.1
[3.3.0]: https://github.com/4regab/Hover/compare/v3.2.0...v3.3.0
[3.2.0]: https://github.com/4regab/Hover/compare/v3.1.1...v3.2.0
[3.1.1]: https://github.com/4regab/Hover/compare/v3.1.0...v3.1.1
[3.1.0]: https://github.com/4regab/Hover/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/4regab/Hover/releases/tag/v3.0.0
