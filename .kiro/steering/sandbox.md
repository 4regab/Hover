# Run code in the sandbox, out of the user's way

The user's Mac is in daily use (Microsoft Office and other apps). Earlier agent work
there got in the way: a near-invisible test window that swallowed clicks, Hover
launched for smoke tests, apps registered with LaunchServices, builds using every core.
So code runs only inside Anthropic's sandbox-runtime (srt), through
`scripts/sandbox.sh`.

## The sandbox

`scripts/sandbox.sh -- <command> [args...]` runs a command on this checkout under srt,
at background priority (efficiency cores, throttled disk I/O). Inside it:

- Writes go only to this checkout (never its `.git` or `.kiro`), srt's temp folder
  `/private/tmp/claude/hover` and the user's own macOS temp folder (Apple's tools
  ignore TMPDIR). Caches and Hover's data folder live in `.sandbox/` (git-ignored,
  not indexed by Spotlight).
- Unix sockets only inside that temp folder. MSBuild's worker nodes can't connect,
  so `scripts/sandbox.sh -- dotnet build …` adds `-m:1` itself; scripts that call
  dotnet pass `$HOVER_DOTNET_ARGS`.
- SSH and cloud keys, keychains, mail, other apps' data and the user's documents are
  unreadable (only this checkout is readable in `~/Documents`).
- No window server and no Apple Events: nothing can open a window, take focus, or
  launch or script an app.
- The network goes through srt's proxy, to package registries and GitHub only.
  `HOVER_SANDBOX_DOMAINS="a.com,*.b.com"` adds hosts; `--offline` allows none.

`scripts/sandbox.sh check` proves each of those holds. Run it once per session before
other work; if any line says FAIL, stop and tell the user.

## Rules

- Run every build, test, script, package install and code check through
  `scripts/sandbox.sh -- …`: `dotnet build`, `dotnet test`, `npm ci`,
  `node web/office/build.mjs`, `scripts/build-macos.sh`, `scripts/test-macos.sh`
  (it sees `HOVER_SRT=1` and skips its on-screen app smokes), and anything else that
  executes code.
- On the host itself, only: reading and searching files, editing with the editor
  tools, read-only git (`status`, `diff`, `log`), `scripts/sandbox.sh`, and web lookups.
- Never on the host: open a window (even transparent or off screen), launch or
  `open` an app, start a browser, take focus, run `lsregister`, `tccutil`,
  `defaults write`, `osascript`, or kill anything you didn't start, or install
  software. Ask the user first if one of these is truly needed.
- Checks that need a screen (rendering the office, Hover's UI smoke tests) can't run
  in the sandbox. Say so and ask the user before running one outside it.
- If srt is missing or `check` fails, stop and tell the user. Don't fall back to the host.
- Installing the user's own copy of Hover (`/Applications/Hover.app`) is for them to
  ask for; a build stays in the checkout for them.
