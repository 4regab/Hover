# Security

Hover runs coding agents that can change files and run commands, and it keeps API keys
and encrypted session history on your computer. Please report security problems
privately.

## Report a vulnerability

Use [GitHub's private vulnerability reporting](https://github.com/4regab/Hover/security/advisories/new).
Don't open a public issue for it.

Include:

- what an attacker could do, and what they would need first;
- the steps to reproduce it;
- the Hover version (`hoverai --version`), the OS and the agent involved.

Never send a real API key or a real session. Use made-up values.

You should get a first answer within a week.

## Supported versions

| Version | Supported |
|---|---|
| 3.x (native) | Yes, the latest release |
| 2.x (.NET) | No |

## In scope

- Tool access: an agent doing something its access setting (Full, Ask first, Ask
  always, Read only) should have stopped or asked about.
- Keys and history: `secrets.dat`, `note.key` or the `agents/` history readable by
  another user, or written in plain text.
- Voice: audio or text sent somewhere other than the service the user chose.
- The local OpenCode server or the Phonon helper reachable from another machine or user.
- The installers and the Phonon download (files not checked against their pinned hashes).

## Out of scope

- What an agent does inside its folder with Full access. That is what Full allows.
- Problems in the agents themselves (Kiro, Codex, Cursor, OpenCode, Claude Code) or in Groq,
  Gemini or OpenAI. Report those to their makers.
