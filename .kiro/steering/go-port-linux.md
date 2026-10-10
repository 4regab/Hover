---
inclusion: always
---

# Go port: Linux targets

- Wayland only. Do not build for X11 or XWayland; they are legacy.
- Audio and screen capture go through PipeWire (the desktop portal's screen cast for capture). Do not use PulseAudio.
- In the Rust code (git tag `rust-final`), `app/src/x11.rs` and the PulseAudio path are references for behaviour only, not for what to build.
