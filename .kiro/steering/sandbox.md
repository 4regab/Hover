# Run code out of the user's way, and know Hover's own sandbox

The user's Mac is in daily use (Microsoft Office and other apps). Earlier agent work there
got in the way: a near-invisible test window that swallowed clicks, Hover launched for
smoke tests, apps registered with LaunchServices, builds using every core. These rules keep
that from happening again while Hover gains macOS support in Rust.

## Hover's sandbox (the feature)

`crates/hover-agents/src/sandbox.rs` starts every agent tool under Anthropic's
sandbox-runtime (`srt`, version `sandbox::VERSION`): sandbox-exec with a generated profile
on a Mac, bubblewrap on Linux. It is on by default (Settings → Integrations → Sandbox) and
off on Windows, with the note "The sandbox needs macOS or Linux." Inside it:

- Writes go only to the folders the tool's sessions work in, the tool's own state and
  caches, and temp. The folders are fixed when the tool starts, so a session in another
  folder gets the tool started again when nothing of it runs.
- Keys, keychains, mail, messages, browsers' data and other apps' data (Office's included)
  can't be read, nor Hover's own.
- No window server and no Apple Events: nothing can open a window, take focus, or launch
  or script an app.
- The network goes only through srt's proxy, to the tool's own service, package registries
  and GitHub, plus the hosts in `<data>/sandbox/allowed-domains.txt`.
- Computer use still works: Cua Driver's daemon runs outside the sandbox, and the agent's
  `cua-driver` reaches it over one Unix socket.

If `srt` or `rg` (on Linux also `bwrap` and `socat`) is missing, or Hover is already inside
a sandbox (`HOVER_SANDBOXED=1`), the tool starts unsandboxed and `hover.log` says why; the
settings page shows what is missing. The settings text and the argument list are built by
plain functions (`config`, `srt_args`, `plan`) that the tests run on every OS
(`crates/hover-agents/tests/sandbox.rs`). A change there needs a test there.

## Working on this repo on the user's Mac

- Edit, read and search files, read-only git (`status`, `diff`, `log`), and run builds and
  tests that need no screen: `cargo check`, `cargo clippy`, `cargo test`. If `srt` is
  installed, run them under it with the settings `sandbox::config` writes, at low priority
  (`nice`), and ask before raising the limits.
- Never on the host: open a window (even transparent or off screen), run or `open`
  `hoverai` or `Hover.app`, start a browser, take focus, run `lsregister`, `tccutil`,
  `defaults write` or `osascript`, kill anything you didn't start, or install software.
  Ask the user first if one is truly needed.
- `hoverai --shots` draws with the software renderer and opens no window, but it is for
  Windows and Linux; on a Mac ask first.
- Anything that needs a screen (the notch, the menu bar, the WKWebView, the permission
  prompts) can't be checked in a sandbox. Say that it was not run, and ask the user before
  running it outside one.
- Installing the user's own copy (`/Applications/Hover.app`) is for them to ask for: a build
  stays in the checkout (`dist/Hover.app`).
- If `srt` is missing, say so. Don't fall back to running things on the host that the
  rules above forbid.
