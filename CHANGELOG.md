# Changelog

What changed in each Hover release. Versions follow [Semantic Versioning](https://semver.org).
3.x is the native (Rust) Hover; 2.x was the .NET app.

## [Unreleased]

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

[Unreleased]: https://github.com/4regab/Hover/compare/v3.1.0...HEAD
[3.1.0]: https://github.com/4regab/Hover/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/4regab/Hover/releases/tag/v3.0.0
