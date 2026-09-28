# Hover native port: working area

The candidate is Rust + Slint + wgpu. It replaces nothing until its gates pass and the
cutover has been reviewed; the C# app under `src/` is still the product.

| Where | What |
|---|---|
| `phase0/` | The baseline (`BASELINE.md`), the feature and screen checklists, what md.js/diagram.js really support, the frozen benchmark procedure and thresholds, and baseline screenshots (`baseline/`). |
| `phase1/REPORT.md` | The Phase 1 feasibility report: evidence, gate status, gaps, costs. |
| `phase1/RUN-ON-WINDOWS.md` | How to run the prototypes and gates on a real desktop. |
| `phase3/REPORT.md`, `phase3/RUN-ON-WINDOWS.md` | Phase 3 (persistence, sessions, product): evidence, differences, questions; the Windows checks. |
| `HANDOFF.md`, `NEXT-PROMPT.md` | The state for the next agent, and the prompt that sets its work (all remaining phases, Windows and Linux). |
| `bench/` | `Measure-Hover.ps1` (the benchmark) and `capture-office.mjs` (baseline captures). |
| `tools/` | `FakeAcp` (a stand-in ACP agent) and `HoverFixture` (a sealed history written with Hover's own code). |
| `../native/` | The Rust workspace: `hover-core` (paths, settings, sealing, history, images, single instance), `hover-md`, `hover-diagram`, `hover-chat`, `hover-notch`, `apps/chat-proto`, `apps/notch-proto`. |

## Build (Linux dev VM or Windows)

```sh
cd native
cargo test --workspace --release        # goldens, engine and notch-logic tests, headless
cargo run --release -p chat-proto -- --screenshot shot.png --select
node golden/gen.mjs                     # re-derive the goldens from the office's own JS
```

On Linux, `notch-proto` is only a plain dev window so far. Its notch behaviour is
Windows-only (`apps/notch-proto/src/win.rs`) until Phase 3C.

## Platform layout

Windows and Linux are both targets. The logic is shared; what differs sits in a
`platform` module per crate, `platform/windows.rs` and `platform/linux.rs`, chosen by
`cfg(windows)`, behind the same function names and small traits (`crypto::KeyGuard`,
`platform::Autostart`), so tests can swap them. Where a whole feature is small
(`hover-core::single`), the two halves sit in one file as `mod imp`.

| Area | Windows | Linux | Where |
|---|---|---|---|
| Data folder | `%APPDATA%\Hover` (known folder) | `$XDG_DATA_HOME/Hover` (`~/.local/share/Hover`) | `hover-core::paths`, `platform::app_data` |
| `note.key` | DPAPI, current user | Secret Service item, else the key in a 0600 file | `platform::SystemKeyGuard` |
| Launch at login | HKCU `…\Run\Hover` | `~/.config/autostart/hover.desktop` | `platform::SystemAutostart` |
| Single instance | `Local\HoverRunningInstance` mutex, `Local\HoverShowApp` event | lock on `$XDG_RUNTIME_DIR/hover.lock`, `hover.sock` | `hover-core::single` |

`cargo check --release --workspace --target x86_64-pc-windows-msvc` must stay green.
