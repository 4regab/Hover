# Testing

Three layers, from fastest to closest to what users run.

## 1. Unit and golden tests

```powershell
cargo test --manifest-path native/Cargo.toml --release --workspace
```

`native/golden/` holds fixtures and outputs made from the 2.x page (Markdown, flowcharts,
the chat's copy and layout). `hover-agents`' tests drive the ACP host and the OpenCode host
against in-process fakes. The three layout goldens that compare text widths were measured
with DejaVu Sans on Linux, so they skip on Windows.

## 2. The release app with fake tools

`native/tools/hover-measure` (not shipped) runs the release app with `HOVER_BENCH=1`, sends it
commands on stdin (`native/apps/hover/src/bench.rs`), and samples its process tree. Each run
gets fresh app data and a project folder whose name has a space and a `ü` in it. The real
tools are replaced by stand-ins placed first on `PATH`:

- `fake-agent` answers as `kiro-cli`, `codex-acp`, `codex` and `cursor-agent`: signed in, ACP on
  stdio, and `chat --no-interactive /usage` for the Kiro quota. A prompt says what its turn
  does: `[seconds:N]`, `[bytes:N]`, `[ask:KIND:TARGET]`, `[fail]`, and so on (see the file's header).
- `fake-opencode` is `opencode serve`: loopback only, Basic auth from `OPENCODE_SERVER_PASSWORD`,
  the routes and event stream Hover uses. Its directives are `[question]`, `[ask:bash:CMD]`,
  `[drop]` (the event stream closes halfway), `[lose]` (the prompt's response is lost) and
  `[fail]`. With `FAKE_OPENCODE_LOG=FILE` it logs every request, so a run can check that no
  prompt went twice and that every call named its folder.

```powershell
cargo build --manifest-path native/Cargo.toml --release -p hover -p hover-measure
.\native\tools\hover-measure\run-memory.ps1 -Exe native\target\release\hover.exe -Out out\oc -Runs 1 -Script opencode.hms -Env "FAKE_OPENCODE_LOG=$PWD\out\oc.log"
```

| Scenario (`scenarios/`) | What it checks |
|---|---|
| `opencode.hms` | OpenCode end to end: a turn; a question picked, then skipped; a command allowed, then denied; a dropped stream; a lost prompt; a failure; a reply. Each is checked through `said ID`, which prints the answer's start. |
| `quota.hms` | Each quota on, then all of them: Kiro's read through `/usage`, the rest failing with readable messages. `FAKEACP_USAGE_MS` makes the read take as long as the real one. |
| `memory.hms` | Every memory scenario in one run (see profiling.md). |
| `stress.hms` | 100 fast cycles (office, chats, history, Settings, app window), 10 reopens at 29.5 s (just before the office is dropped) and 10 just after. An approval waits and a task runs the whole time; the end checks both are intact. |
| `soak.hms` | About 80 minutes: 60 mixed with new tasks, then 20 of a fixed load that adds nothing to the history. |
| `stress-dash.hms` | The app window minimised and restored 80 times while three agents work. |

A failed `expect` stops the run and is written as `end,failed step N` in `markers.csv`.
`quit` fails if any of Hover's child processes outlives it, and each run records
`orphans`.

Bench commands for checks: `sessions`, `said ID`, `until-idle`, `until-ask`,
`answer ID allow|trust|trustAll|deny`, `answer-front HOW`, `pick ID LABEL` (a question's
choice), `until-quota ID`, `drag X0 Y0 X1 Y1 [N]` (a text selection in the open chat, through
the pointer's own callbacks), `copy`, `office`, `state`.

## 3. Screenshots and real input

`hover --shots DIR` renders every view with the software renderer, including the chat's images
(`office-chat-images*.png`) and a selection (`office-chat-selection.png`). The 3D office isn't
the same twice (the camera may still be moving, and the clock shows the real time), so compare
the side panel between builds:

```powershell
.\native\tools\hover-measure\compare-shots.ps1 -A shots-before -B shots-after -Region 792,8,360,424 -Filter office-*.png
```

Settings shots compare whole.

Real pointer and keyboard input (`move`, `click`, `press`, `type`, `click-name`) goes through
`SendInput` and UI Automation. It needs a logged-on, unlocked desktop session: on a machine
reached only over SSH or SSM, the input desktop is the logon screen and the input never
reaches Hover. On Linux, `hover --selftest DIR` drives the notch on a real X display. It
doesn't exist in the Windows build.
