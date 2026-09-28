# Hover native port: working area

The candidate is Rust + Slint + wgpu. It replaces nothing until its gates pass and the
cutover has been reviewed; the C# app under `src/` is still the product.

| Where | What |
|---|---|
| `phase0/` | The baseline (`BASELINE.md`), the feature and screen checklists, what md.js/diagram.js really support, the frozen benchmark procedure and thresholds, and baseline screenshots (`baseline/`). |
| `phase1/REPORT.md` | The Phase 1 feasibility report: evidence, gate status, gaps, costs. |
| `phase1/RUN-ON-WINDOWS.md` | How to run the prototypes and gates on a real desktop. |
| `HANDOFF.md`, `NEXT-PROMPT.md` | The state for the next agent, and the prompt that sets its work (all remaining phases, Windows and Linux). |
| `bench/` | `Measure-Hover.ps1` (the benchmark) and `capture-office.mjs` (baseline captures). |
| `tools/` | `FakeAcp` (a stand-in ACP agent) and `HoverFixture` (a sealed history written with Hover's own code). |
| `../native/` | The Rust workspace: `hover-md`, `hover-diagram`, `hover-chat`, `hover-notch`, `apps/chat-proto`, `apps/notch-proto`. |

## Build (Linux dev VM or Windows)

```sh
cd native
cargo test --workspace --release        # goldens, engine and notch-logic tests, headless
cargo run --release -p chat-proto -- --screenshot shot.png --select
node golden/gen.mjs                     # re-derive the goldens from the office's own JS
```

On Linux, `notch-proto` is only a plain dev window. Its notch behaviour is Windows-only
(`apps/notch-proto/src/win.rs`).
