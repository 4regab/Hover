# Testing

Three layers, from fastest to closest to what users run, then a manual voice check.

## 1. Unit and golden tests

```powershell
cargo test --manifest-path native/Cargo.toml --release --workspace
```

`native/golden/` holds fixtures and outputs made from the 2.x page (Markdown, flowcharts,
the chat's copy and layout). `hover-agents`' tests drive the ACP host and the OpenCode host
against in-process fakes. The three layout goldens that compare text widths were measured
with DejaVu Sans on Linux, so they skip on Windows.

Voice, Phonon, routing and Pause have their own tests in the same run. To run one part:

```powershell
cargo test --manifest-path native/Cargo.toml --release -p hover --lib voice::    # the voice flow, capture, WAV, Groq, cleanup
cargo test --manifest-path native/Cargo.toml --release -p hover --lib phonon::   # setup, checks, repair, remove (a fake Python)
cargo test --manifest-path native/Cargo.toml --release -p hover-agents --lib route::
cargo test --manifest-path native/Cargo.toml --release -p hover-agents --test sessions   # Pause, the queue, an unconfirmed stop
cargo test --manifest-path native/Cargo.toml --release -p hover-core                     # projects, secrets, the new settings keys
```

- `voice::tests` drives `Voice` with a fake engine, microphone and agent: the countdown
  against Start (one dispatch), edits, cancel, a failed cleanup, Local not ready (never
  Groq), the ten-minute cap, an unavailable agent, a busy press, Try it.
- `voice::groq` and `voice::cleanup` talk to a local fake HTTP server; no key or network.
- `phonon::tests` run setup against small fake pins and a fake Python.
- `route::tests` cover the word rules, the agent's checked answer, and the routing turn's
  access `"none"` in a folder of its own.
- `sessions.rs`: `pause_sends_the_next_queued_reply_once_the_stop_is_confirmed` and
  `an_unconfirmed_stop_sends_nothing_and_queued_replies_can_be_cancelled`.

Two voice tests are ignored because they need real things:

```powershell
# A real microphone, for two seconds
cargo test --manifest-path native/Cargo.toml --release -p hover --lib voice::audio -- --ignored
# Phonon for real: downloads, installs and checks it, transcribes the sample, then a cancel
$env:PHONON_LIVE_DIR = "$env:TEMP\hover-phonon-live"   # the default; a Ready install there is reused
cargo test --manifest-path native/Cargo.toml --release -p hover --lib phonon::tests::live_install -- --ignored --nocapture
```

`live_install` downloads the pinned runtime, wheels and model (about 420 MB on Windows,
523 MB on Linux x64) and prints each state, the sizes and the times. `PHONON_WAV=FILE`
transcribes another 16 kHz mono WAV.

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
.\native\tools\hover-measure\run-memory.ps1 -Exe native\target\release\hoverai.exe -Out out\oc -Runs 1 -Script opencode.hms -Env "FAKE_OPENCODE_LOG=$PWD\out\oc.log"
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

Voice adds two sets. `voice-<size>-<stage>.png` is the notch's voice card in every stage
(listening, loading, transcribing, resolving, preview, preview-default-workspace, editing,
starting, choose-agent, started, cancelled, error, error-setup) at each office size, with a
`-2x` copy at Default, and `voice-extra-large-busy.png`. The Settings set is
`settings-<name>.png`, each with a `-narrow` copy: `projects-registered`, `project-page`,
`voice-cloud`, `voice-local-<state>` (each Phonon card state) and `voice-try-done` /
`voice-try-listening`. The stages are drawn as `Voice` hands them over; no voice flow runs.

Real pointer and keyboard input (`move`, `click`, `press`, `type`, `click-name`) goes through
`SendInput` and UI Automation. It needs a logged-on, unlocked desktop session: on a machine
reached only over SSH or SSM, the input desktop is the logon screen and the input never
reaches Hover. On Linux, `hover --selftest DIR` drives the notch on a real X display. It
doesn't exist in the Windows build.

## 4. Voice live smoke (Windows)

This checks the whole flow with a real microphone and Local speech, but fake agents. It
needs a logged-on desktop, a microphone and speakers, and a Ready Phonon install (run
`live_install` first).

1. Use a data folder of its own: `$env:HOVER_DATA_DIR = "$env:TEMP\hover-int-live"`. Link
   its `phonon` folder to the live install so nothing downloads:
   `New-Item -ItemType Junction "$env:HOVER_DATA_DIR\phonon" -Target "$env:TEMP\hover-phonon-live\phonon"`.
2. Put `fake-agent.exe` (from `hover-measure`) first on `PATH` as `kiro-cli.exe`,
   `codex-acp.exe`, `codex.exe` and `cursor-agent.exe`, and set `FAKEACP_LOG=FILE`.
3. Start `hoverai.exe`. In Settings → Voice: voice on, Local (Phonon). Set the default
   workspace to a test folder with Ask first access.
4. Hold Ctrl+Alt+Space for about 2 s in silence. Expected: Listening, then "Nothing was
   heard…" with Retry, and no session. Esc closes the card.
5. Hold it while `native/apps/hover/assets/phonon/check.wav` plays from the speakers.
   Expected: Listening, Loading (about 25 s, Phonon's start), Resolving, the preview with
   "Open the notes folder and add a list of the open tasks.", the default workspace, Kiro
   and Ask first, a 3 s countdown, then Started.
6. Check `FAKEACP_LOG`: one `session/new` with the workspace as `cwd` and one
   `session/prompt` with the transcript. `hover.log` has `voice: run N started (kiro,
   access risky)`.

Not covered by this recipe: Cloud (Groq) and cleanup with real keys, choosing another
agent, and giving the keyboard back to the previous app. Results of the last run:
`evidence/voice-chat/integration.md`.

## Linux

The same tests run on Linux (`make test`). For voice on Linux (the build, X11
hold-to-talk, the microphone, Phonon), see `evidence/voice-chat/linux.md`.
