//! `hover --shots DIR`: every view of the product rendered headless with Slint's
//! software renderer, from the real App (its settings, its sessions, its quotas, in a
//! data folder of its own), for the reports. No display, no tools, no network: the
//! sessions run a stand-in and the quotas read fixed values.

use crate::notch::Plat;
use crate::ui::*;
use crate::{App, view};
use hover_agents::session::{RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_app::pages::Section;
use hover_core::model::{AgentTool, Appearance, KiroState};
use hover_core::platform::Look;
use hover_notch::{Openness, Rect};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// No platform at all: a 1920 × 1080 display at 100 %.
pub struct Plain;

impl Plat for Plain {
    fn primary(&self) -> (Rect, f64) { (Rect { left: 0, top: 0, right: 1920, bottom: 1080 }, 1.0) }
    fn signature(&self) -> String { String::new() }
    fn cursor(&self) -> (i32, i32) { (-100, -100) }
    fn buttons(&self) -> bool { false }
    fn place(&self, _r: Rect) {}
    fn raise(&self) {}
    fn set_accepts_keys(&self, _on: bool) {}
    fn remember_foreground(&self) {}
    fn restore_foreground(&self) {}
    fn focus(&self) {}
    fn set_hit(&self, _over: bool, _s: (f64, f64, f64, f64), _scale: f64) {}
    fn foreground_is_ours(&self) -> bool { false }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(vec![]) };
}

struct Headless;

/// What `ui_do` was asked to run on the UI thread. The shots have no event loop, so this waits
/// until `pump` runs it (only the checks that need an answer from a thread call it).
static QUEUED: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>> = std::sync::Mutex::new(Vec::new());

struct Proxy;

impl slint::platform::EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> { Ok(()) }
    fn invoke_from_event_loop(&self, event: Box<dyn FnOnce() + Send>) -> Result<(), slint::EventLoopError> { QUEUED.lock().unwrap().push(event); Ok(()) }
}

/// Runs what threads asked the UI thread to do.
fn pump() {
    let todo = std::mem::take(&mut *QUEUED.lock().unwrap());
    for f in todo { f(); }
}

impl Platform for Headless {
    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> { Some(Box::new(Proxy)) }
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOWS.with(|v| v.borrow_mut().push(w.clone()));
        Ok(w)
    }
}

fn adapter(i: usize) -> Rc<MinimalSoftwareWindow> { WINDOWS.with(|v| v.borrow()[i].clone()) }

/// One frame of a window, over a backdrop (the notch is see-through), saved as PNG.
fn save(w: &Rc<MinimalSoftwareWindow>, size: (u32, u32), scale: f32, backdrop: [u8; 3], file: &Path) { save_in(w, size, scale, backdrop, file, false) }

/// The open notch's office only (its shape), at its logical size: what the mockup's frame shows.
fn save_office(w: &Rc<MinimalSoftwareWindow>, size: (u32, u32), file: &Path) { save_in(w, size, 1.0, [0x3a, 0x4a, 0x5e], file, true) }

fn save_in(w: &Rc<MinimalSoftwareWindow>, size: (u32, u32), scale: f32, backdrop: [u8; 3], file: &Path, office: bool) {
    w.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
    w.set_size(slint::PhysicalSize::new((size.0 as f32 * scale) as u32, (size.1 as f32 * scale) as u32));
    let (pw, ph) = ((size.0 as f32 * scale) as usize, (size.1 as f32 * scale) as usize);
    let mut buf = vec![PremultipliedRgbaColor::default(); pw * ph];
    // A frame, then the poll (which reads what that frame laid out), then the shot.
    w.request_redraw();
    w.draw_if_needed(|r| { r.render(&mut buf, pw); });
    run_for(120);
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        w.request_redraw();
        w.draw_if_needed(|r| { r.render(&mut buf, pw); });
    }
    let mut img = image::RgbImage::new(pw as u32, ph as u32);
    for (i, p) in buf.iter().enumerate() {
        let a = p.alpha as u32;
        let px = [0, 1, 2].map(|c| {
            let (s, b) = ([p.red, p.green, p.blue][c] as u32, backdrop[c] as u32);
            (s + b * (255 - a) / 255).min(255) as u8
        });
        img.put_pixel((i % pw) as u32, (i / pw) as u32, image::Rgb(px));
    }
    // The notch clips its open view to the shape's rounded bottom corners, which the
    // software renderer can't: the corners are cut here as the GPU renderers cut them.
    let notch = Rc::ptr_eq(w, &adapter(0));
    let cut = crate::APP.with(|a| a.borrow().as_ref().filter(|a| notch && a.notch.get_view_visible())
        .map(|a| (a.notch.get_shape_x(), a.notch.get_shape_w(), a.notch.get_shape_h(), a.notch.get_shape_r())));
    if let Some((sx, sw, sh, r)) = cut {
        let k = scale;
        let (x0, x1, y1, r) = (sx * k, (sx + sw) * k, sh * k, r * k);
        for y in 0..ph { for x in 0..pw {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            if py < y1 - r || py > y1 || px < x0 || px > x1 { continue; }
            let cx = if px < x0 + r { x0 + r } else if px > x1 - r { x1 - r } else { continue };
            let d = (px - cx).hypot(py - (y1 - r)) - r;
            let a = (d + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                let o = *img.get_pixel(x as u32, y as u32);
                img.put_pixel(x as u32, y as u32, image::Rgb([0, 1, 2].map(|c| (o[c] as f32 * (1.0 - a) + backdrop[c] as f32 * a).round() as u8)));
            }
        } }
    }
    if let (true, Some((sx, sw, sh, _))) = (office, cut) {
        let (x, w, h) = ((sx * scale) as u32, ((sw * scale) as u32).min(img.width()), ((sh * scale) as u32).min(img.height()));
        img = image::imageops::crop_imm(&img, x, 0, w.min(img.width() - x), h).to_image();
    }
    img.save(file).expect("the shot is written");
    println!("{}", file.display());
}

/// The chat's fixtures, by what the prompt asks (the mockup's states): a finished turn
/// with every kind of step and answer, thinking that streams, ten subagents, a question
/// to answer, a long answer. Only what a tool reports: the steps as hover-agents makes
/// them, the reasoning as text, the subagents as OpenCode's task tool reports them.
fn chat_fixture(a: &RunArgs, hold: &Arc<std::sync::Mutex<bool>>) -> Option<KiroResult> {
    use hover_agents::stream::KiroEvent;
    use hover_core::model::KiroStep;
    let p = a.prompt.as_str();
    let kind = ["second monitor", "taskbar is at the top", "every monitor setup", "release build", "whole notch"].iter().position(|k| p.contains(k))?;
    let ev = |s: KiroStep| (a.events)(KiroEvent { step: Some(s), ..Default::default() });
    let st = |id: &str, kind: &str, title: &str, target: Option<&str>, status: &str| KiroStep::new(id, kind, title, target.map(Into::into), status);
    let think = |id: &str, text: &str, ms: Option<f64>| KiroStep { output: Some(text.into()), ms, ..st(id, "thought", "Thinking", None, if ms.is_some() { "completed" } else { "in_progress" }) };
    let wait = || while *hold.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); };
    let done = |text: &str| Some(KiroResult::new(KiroState::Completed, text));
    match kind {
        0 => {
            ev(think("t1", "The notch blinks only on the second monitor. So it is not the animation itself, something about where the window is placed.\n\nHover keeps one full-size window and never resizes it, except when the office size changes. But on a second monitor `SetWindowPos` gets the main display's size first, then the DPI message arrives and Windows resizes it. That is one resize per open.\n\nI'll read `win.rs` to see where the window is first placed, then search for every `SetWindowPos` call.", Some(14200.0)));
            ev(st("r1", "read", "Read", Some("apps/hover/src/win.rs"), "completed"));
            ev(KiroStep { output: Some("6 results".into()), ..st("s1", "search", "Search", Some("SetWindowPos"), "completed") });
            ev(think("t2", "Found it. `place()` runs before the monitor's DPI is known. If I read the DPI with `GetDpiForMonitor` first and place the window once, the extra resize goes away.", Some(4100.0)));
            let diff = "@@ -41 +41 @@\n  fn place(hwnd: HWND, m: HMONITOR) {\n-     let r = main_rect();\n+     let dpi = monitor_dpi(m);\n+     let r = scaled(monitor_rect(m), dpi);\n      SetWindowPos(hwnd, HWND_TOPMOST, r.x, r.y, r.w, r.h,\n-         SWP_NOACTIVATE);\n+         SWP_NOACTIVATE | SWP_NOSENDCHANGING);\n  }\n  \n+ // Read the scale first: placing then rescaling is the blink.\n+ fn monitor_dpi(m: HMONITOR) -> u32 {\n+     let (mut x, mut y) = (96, 96);\n+     unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };\n+     x\n+ }";
            let (add, del) = (diff.lines().filter(|l| l.starts_with('+')).count() as i32, diff.lines().filter(|l| l.starts_with('-')).count() as i32);
            ev(KiroStep { added: add, removed: del, ms: Some(900.0), diff: Some(diff.into()), ..st("e1", "edit", "Edit", Some("apps/hover/src/win.rs"), "completed") });
            let out = std::iter::once("… 74 earlier lines not kept".to_string()).chain((0..10).map(|i| format!("test geometry::case_{i:02} ... ok"))).chain(["".into(), "test result: ok. 81 passed; 0 failed; 0 ignored".into()]).collect::<Vec<_>>().join("\n");
            ev(KiroStep { exit: Some(0), ms: Some(4100.0), output: Some(out), ..st("x1", "execute", "Run", Some("cargo test --release -p hover-notch --test geometry -- --test-threads=1 second_monitor_places_once_at_its_own_dpi"), "completed") });
            ev(KiroStep { added: 2, removed: 1, diff: Some("@@ -118 +118 @@\n  fn open(&mut self) {\n-     self.place();\n+     self.dpi_ready();\n+     self.place();\n  }".into()), ..st("e2", "edit", "Edit", Some("apps/hover/src/notch.rs"), "completed") });
            (a.events)(KiroEvent { credits: Some(0.12), ..Default::default() });
            done("Found it. On a second monitor the notch was placed **before** Windows knew that monitor's scale, so it got resized once on every open. That resize is the blink.\n\n### What changed\n\n- `place()` reads the monitor's DPI first, then sizes the window *once*.\n- The resize message is ignored while the notch opens.\n\n```win.rs\n// Read the scale first: placing then rescaling is the blink.\nfn monitor_dpi(m: HMONITOR) -> u32 {\n    let (mut x, mut y) = (96, 96);\n    unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };\n    x\n}\n```\n\n| Monitor | Scale | Blinks before | After |\n|---|---|--:|--:|\n| Main | 100% | 0 | 0 |\n| Second | 150% | 1 per open | 0 |\n\n> **Note** · A monitor plugged in while the notch is open still needs one resize.\n\n- [x] Second monitor at 150%\n- [ ] A monitor plugged in while open")
        }
        1 => {
            ev(st("r1", "read", "Read", Some("apps/hover/src/win.rs"), "completed"));
            ev(think("t1", "The user wants it to hold when the taskbar is at the top too. The notch sits at the top centre, so a top taskbar pushes the work area down.\n\nTwo choices. Use `rcWork` from `GetMonitorInfoW` and start the notch under the taskbar. Or keep it at the very top and draw over the taskbar, since the window is topmost anyway.\n\nDrawing over the taskbar hides the clock on some setups. Starting under it is safer, and it matches what NotchOwl does on a Mac with the menu bar.\n\nThere are three monitors to check, and Linux may have the same bug.", None));
            wait();
            // A stop ends it as a real agent's does: cancelled, not finished.
            if a.ct.is_cancelled() { return Some(KiroResult::new(KiroState::Cancelled, "Stopped.")); }
            done("The notch now starts under a top taskbar on every monitor.")
        }
        2 => {
            ev(st("r1", "read", "Read", Some("apps/hover/src/win.rs"), "completed"));
            ev(think("t1", "That splits well: one subagent per monitor setup, one for Linux, one for the docs, while I write the change.", Some(6300.0)));
            let subs = [("Read how 3 monitors report their work area", "explore", "completed", Some("Monitor 3 is left of the main one. Its x is negative."), Some(48000.0)),
                ("Run the notch tests on monitor 2", "general", "running", None, None), ("Check x11.rs for the same bug", "explore", "running", None, None),
                ("List every SetWindowPos call", "explore", "completed", Some("Found 6 calls. Two are in place()."), Some(12000.0)),
                ("Read the Win32 docs on rcWork", "general", "completed", Some("rcWork leaves out the taskbar. rcMonitor is the whole screen."), Some(20000.0)),
                ("Build for the 32-bit target", "general", "failed", Some("error: linker `link.exe` not found"), Some(9000.0)),
                ("Run the notch tests on monitor 3", "general", "running", None, None), ("Check the DPI on the laptop screen", "explore", "running", None, None),
                ("Review the change to place()", "general", "running", None, None), ("Write the release note", "general", "running", None, None)];
            for (i, (title, ty, status, out, ms)) in subs.iter().enumerate() {
                let status = if *status == "running" { "in_progress" } else { status };
                ev(KiroStep { output: out.map(Into::into), ms: *ms, ..st(&format!("a{i}"), "agent", title, Some(ty), status) });
            }
            wait();
            done("All ten subagents are back.")
        }
        3 => {
            ev(st("r1", "read", "Read", Some("Cargo.toml"), "completed"));
            wait();
            done("Built.")
        }
        _ => done(&format!("## The whole notch, start to end\n\n{}\n\nThe one path that matters: https://example.com/a/very/long/link/that/does/not/break/anywhere/because/it/is/one/word/{}\n\n```rust\nlet placed = place(hwnd, monitor, scale, work_area, taskbar_edge, auto_hide, animations_on, reduced_motion, office_size);\n```\n\n{}",
            "The notch is one window, as wide as the main display, that never resizes while it opens: the shape grows from its resting size to the office by animating one openness value. ".repeat(4),
            "x".repeat(60), "Every step is drawn by the same painter, so a long answer scrolls as one thread and a selection runs across all of it. ".repeat(3))),
    }
}

/// A key, with Ctrl held or not, as the keyboard sends it to the focused box.
fn key(w: &Rc<MinimalSoftwareWindow>, ctrl: bool, k: slint::platform::Key) {
    use slint::platform::{Key, WindowEvent as E};
    use slint::platform::WindowAdapter as _;
    if ctrl { w.window().dispatch_event(E::KeyPressed { text: Key::Control.into() }); }
    w.window().dispatch_event(E::KeyPressed { text: k.into() });
    w.window().dispatch_event(E::KeyReleased { text: k.into() });
    if ctrl { w.window().dispatch_event(E::KeyReleased { text: Key::Control.into() }); }
}

/// Characters typed into whatever has the keyboard focus in the window.
fn type_text(w: &Rc<MinimalSoftwareWindow>, text: &str) {
    use slint::platform::{WindowAdapter as _, WindowEvent as E};
    for c in text.chars() {
        let s = slint::SharedString::from(c.to_string());
        w.window().dispatch_event(E::KeyPressed { text: s.clone() });
        w.window().dispatch_event(E::KeyReleased { text: s });
    }
}

/// A prompt of a dozen lines, its first and last words marked, for the boxes that must scroll.
const LONG_PROMPT: &str = concat!("FIRST LINE: the notch blinks when it opens on my second monitor. Steps: plug in a 150 % monitor, open the office, close it, open it again. ",
    "Expected: no blink. Seen: one blink per open, only on that monitor. Look at src/win.rs where the window is placed and at the DPI change handler, ",
    "and at notch.rs where the openness animates. Keep the resting island's size. Add a test that opens the notch twice on a scaled monitor and ",
    "counts the resizes. Don't touch the office's renderer. When done, run the tests and tell me what changed and why. LAST WORDS HERE");

/// `HOVER_SHOTS_SKIP=chat,voice` leaves those sets out, for a quick run of the rest.
fn skip(what: &str) -> bool { std::env::var("HOVER_SHOTS_SKIP").is_ok_and(|v| v.split(',').any(|w| w.trim() == what)) }

fn run_for(ms: u64) {
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The chat as the mockup draws it, at the Small and Default office sizes (840 × 340 and
/// 1120 × 440): closed and open, thinking (streaming, folded, opened), ten subagents, a
/// question waiting, a finished turn with its change, output, code and files, the
/// history, the reply circle with a draft, the box open, a queued reply, Pause, a long
/// answer. Each file is the office alone, chat-<size>-<state>.png.
fn chat_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, folder: &str, hold: &Arc<std::sync::Mutex<bool>>, hold_c: &Arc<std::sync::Mutex<bool>>) {
    let notch = adapter(0);
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    let until = |f: &dyn Fn() -> bool| { let t = std::time::Instant::now(); while t.elapsed() < Duration::from_secs(5) && !f() { std::thread::sleep(Duration::from_millis(10)); } };
    // Desks for these: the earlier tasks end.
    *hold.lock().unwrap() = false;
    run_for(600);
    let start = |tool: AgentTool, prompt: &str| hover.sessions.start(tool, folder, prompt, vec![]).map(|s| s.id);
    let idle = |id: Option<i32>| if let Some(id) = id { until(&|| hover.sessions.get(id).is_some_and(|s| !s.busy())); };
    let rich = start(AgentTool::Codex, "The notch blinks when it opens on my second monitor. Can you find out why?");
    idle(rich);
    let long = start(AgentTool::Cursor, "Explain the whole notch to me, start to end, in one long answer.");
    idle(long);
    let think = start(AgentTool::Kiro, "Now make it hold when the taskbar is at the top too, and check all three monitors.");
    let agents = start(AgentTool::OpenCode, "Check every monitor setup and run the notch tests on each.");
    let asker = start(AgentTool::Codex, "Make a release build and run it.");
    for id in [think, agents, asker].into_iter().flatten() { until(&|| hover.sessions.get(id).is_some_and(|s| s.kiro_id.is_some() && !s.turns.is_empty() && !s.turns[0].steps.is_empty())); }
    if let Some(id) = asker {
        let sid = hover.sessions.get(id).and_then(|s| s.kiro_id).unwrap_or_default();
        let ask = hover_agents::ask::AgentAsk { id: "c1".into(), kind: "execute".into(), title: "Run".into(), command: Some("cargo build --release -p hover".into()), path: None,
            preview: None, added: 0, removed: 0, reason: "Runs a command".into(), danger: false, questions: None };
        hover.sessions.ask(AgentTool::Codex, &sid, ask, &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
    }
    let g = app.notch.global::<Office>();
    for (ws, tag) in [(hover_core::model::WorkspaceSize::Small, "small"), (hover_core::model::WorkspaceSize::Default, "default")] {
        hover.settings.set_workspace_size(ws);
        view::Host::settings_changed(&**app);
        app.office_follow();
        app.office_push();
        settle(1500);
        let full = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
        let shot = |name: &str| save_office(&notch, full, &dir.join(format!("chat-{tag}-{name}.png")));
        let open = |id: Option<i32>| { if let Some(id) = id { app.open_session(id); } settle(1200); };
        let top = || { g.invoke_d_wheel(100000.0); settle(300); };
        let with = |f: &dyn Fn(&mut hover_chat::Thread, &[hover_chat::Turn])| { let turns = app.page_turns(); if let Some(mut c) = app.page_thread() { f(&mut c, &turns); } app.office_widgets(); settle(400); };
        app.close_drawer();
        settle(600);
        shot("closed");
        // Finished: the end (answer, files, stamp), then the top, then the timeline open
        // on its change, its output and a thought.
        open(rich);
        shot("done");

        // The question Restore and Try again ask before they touch the folder.
        g.set_confirm_title("Restore to here?".into());
        g.set_confirm_ok("Restore".into());
        g.set_confirm_text("The files in “project” go back to how they were after this answer, and the 2 messages after it leave this chat. Changes made since, by the agent or by you, are undone.".into());
        g.set_confirm(true);
        settle(300);
        shot("rewind-confirm");
        g.set_confirm(false);
        g.set_confirm_title("Delete this session?".into());
        g.set_confirm_ok("Delete".into());        // A long folder name: its chip gives way, the header's Delete and Close stay whole.
        g.set_d_folder("a-really-long-project-folder-name-that-goes-on-and-on-and-on".into());
        settle(200);
        shot("long-folder");
        app.office_widgets();
        top();
        shot("done-top");
        with(&|c, t| { c.toggle_steps(t, 0); c.toggle_step(t, 0, 4, false); });
        top();
        shot("done-timeline-diff");
        with(&|c, t| { c.toggle_step(t, 0, 4, false); c.toggle_step(t, 0, 5, false); c.toggle_step(t, 0, 0, false); });
        top();
        shot("done-thought-output");
        with(&|c, t| { c.toggle_step(t, 0, 0, false); c.toggle_step(t, 0, 5, false); c.toggle_step(t, 0, 4, false); c.toggle_flag(t, 0, 4, 0); });
        top();
        shot("done-diff-full");
        // The long command alone: its row and its output's header wrap it.
        with(&|c, t| { c.toggle_flag(t, 0, 4, 0); c.toggle_step(t, 0, 4, false); c.toggle_step(t, 0, 5, false); });
        top();
        shot("done-command");
        open(long);
        shot("long");
        top();
        shot("long-top");
        // Thinking as it streams, a reply queued behind it, the reply dock.
        open(think);
        shot("thinking-live");
        if tag == "small" { if let Some(id) = think { hover.sessions.reply(id, "Use rcWork, and check monitor 3 too.", vec![]); } }
        app.office_changed();
        app.office_widgets();
        settle(600);
        shot("queued");
        g.set_d_draft("use rcWork for the top bar".into());
        app.office_widgets();
        settle(300);
        shot("reply-draft");
        g.set_d_compose(true);
        settle(300);
        shot("reply-open");
        // Voice over the open reply box writes into it (dictation), and only there.
        if tag == "default" {
            g.set_d_hover(true);
            assert!(app.dictation_here(), "an open reply box under the pointer takes dictation");
            app.dictation_shot(&hover_app::voice::Stage::Recording { level: 0.4, secs: 1.0 });
            settle(300);
            shot("dictating");
            app.dictation_shot(&hover_app::voice::Stage::Dictated("and check monitor 3 too".into()));
            settle(300);
            shot("dictated");
            assert_eq!(g.get_d_draft().as_str(), "use rcWork for the top bar and check monitor 3 too", "written after the draft");
            assert_eq!(g.get_d_voice().as_str(), "");
            g.set_d_hover(false);
            assert!(!app.dictation_here(), "not with the pointer elsewhere");
            g.set_d_compose(false);
            g.set_d_hover(true);
            assert!(!app.dictation_here(), "not with the reply box closed");
            g.set_d_hover(false);
            g.set_d_compose(true);
        }
        // A prompt longer than the box: it scrolls inside, the caret kept in view (put in
        // from outside, the caret goes to its end; Ctrl+Home goes back to the top).
        g.set_d_draft(LONG_PROMPT.into());
        g.set_d_draft_to_end(g.get_d_draft_to_end() + 1);
        settle(300);
        shot("reply-long-end");
        key(&notch, true, slint::platform::Key::Home);
        settle(300);
        shot("reply-long-top");
        g.set_d_draft("".into());
        settle(300);
        shot("reply-pause");
        g.set_d_compose(false);
        // Ten subagents: four, then the rest; one's result.
        open(agents);
        shot("subagents");
        with(&|c, t| { c.toggle_flag(t, 0, 2, 1); c.toggle_flag(t, 0, 2, 2); });
        shot("subagents-all");
        with(&|c, t| c.toggle_steps(t, 0));
        top();
        shot("subagents-timeline");
        // A question waiting: its card over the reply circle.
        open(asker);
        shot("ask");
        app.open_panel(Some("history"));
        settle(800);
        shot("history");
        app.open_panel(None);
        app.close_drawer();
    }
    *hold_c.lock().unwrap() = false;
    run_for(600);
    // A Kiro Web chat, made last so the shots above keep their office: its cloud chip
    // beside the context ring. Kiro is the only tool that runs there.
    if let Some(key) = long.and_then(|id| hover.sessions.get(id)).map(|s| s.key) { hover.sessions.delete(&key); }
    let cloud = hover.sessions.start_in(AgentTool::Kiro, folder, "Add a dark mode with a theme switch to the site.", vec![], Some("full"), Some(vec!["4regab/hoverweb".into()])).map(|s| s.id);
    idle(cloud);
    for (ws, tag) in [(hover_core::model::WorkspaceSize::Small, "small"), (hover_core::model::WorkspaceSize::Default, "default")] {
        hover.settings.set_workspace_size(ws);
        view::Host::settings_changed(&**app);
        app.office_follow();
        app.office_push();
        settle(1500);
        let full = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
        if let Some(id) = cloud { app.open_session(id); }
        settle(1200);
        save_office(&notch, full, &dir.join(format!("chat-{tag}-cloud-chip.png")));
        app.close_drawer();
    }
}

/// Expand chat (#41): each kind of chat in the small drawer in the app window (before), then
/// the same chat expanded (after); with the files and changes beside it, the session list
/// hidden, and the narrowest window. Checks that going there and back keeps the session, the
/// draft and the place in the thread.
fn expand_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, folder: &str, hold_c: &Arc<std::sync::Mutex<bool>>) {
    let dash = adapter(1);
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    if let Some(d) = &*app.dash.borrow() { d.set_in_settings(false); }
    app.dash_settings.set(false);
    app.refresh_page(false);
    // One that is still streaming, and one that waits for an approval, held open as the chat shots hold theirs.
    *hold_c.lock().unwrap() = true;
    let live = hover.sessions.start(AgentTool::Kiro, folder, "Now make it hold when the taskbar is at the top too, and check all three monitors.", vec![]).map(|s| s.id);
    let asker = hover.sessions.start(AgentTool::Codex, folder, "Make a release build and run it.", vec![]).map(|s| s.id);
    for id in [live, asker].into_iter().flatten() {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(5) && !hover.sessions.get(id).is_some_and(|s| s.kiro_id.is_some() && !s.turns.is_empty() && !s.turns[0].steps.is_empty()) { std::thread::sleep(Duration::from_millis(10)); }
    }
    if let Some(id) = asker {
        let sid = hover.sessions.get(id).and_then(|s| s.kiro_id).unwrap_or_default();
        let ask = hover_agents::ask::AgentAsk { id: "c1".into(), kind: "execute".into(), title: "Run".into(), command: Some("cargo build --release -p hover".into()), path: None,
            preview: None, added: 0, removed: 0, reason: "Runs a command".into(), danger: false, questions: None };
        hover.sessions.ask(AgentTool::Codex, &sid, ask, &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
    }
    let all = hover.sessions.all();
    let done = all.iter().find(|s| !s.busy() && !s.waiting() && !s.turns.is_empty()).map(|s| s.id);
    let picks = [("done", done), ("live", live), ("ask", asker)];
    macro_rules! g { () => { app.dash.borrow().as_ref().expect("the app window").global::<Office>() } }
    let top_turn = || { let sc = app.page_scroll(); app.page_thread().and_then(|t| t.sections.iter().position(|x| x.y + x.h > sc)) };
    for (name, sess) in picks {
        let Some(id) = sess else { println!("no {name} chat for the expanded shots"); continue };
        app.open_session(id);
        // A draft written in the small drawer comes along.
        g!().set_d_draft(format!("a draft for the {name} chat").into());
        settle(1200);
        save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join(format!("expand-{name}-before.png")));
        g!().invoke_d_wheel(60.0);
        settle(300);
        let (turn, before) = (top_turn(), app.page_scroll());
        g!().invoke_d_expand();
        settle(1500);
        assert!(g!().get_d_wide(), "{name}: the chat is expanded");
        assert_eq!(app.page.open.get(), Some(id), "{name}: the same session");
        assert_eq!(g!().get_d_draft().as_str(), format!("a draft for the {name} chat"), "{name}: the draft came along");
        assert_eq!(top_turn(), turn, "{name}: the reader stays at the same turn ({before} before)");
        save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join(format!("expand-{name}-after.png")));
        if name == "done" {
            app.desk_details(id);
            settle(1500);
            assert!(app.page.desk.panel.get().is_some(), "the files and changes open beside the chat");
            assert!(g!().get_d_wide(), "the chat stays expanded beside them");
            save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("expand-done-details.png"));
            app.desk_details(id);
            settle(600);
            g!().set_list_open(false);
            settle(600);
            save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("expand-done-no-list.png"));
            g!().set_list_open(true);
            save(&dash, (880, 560), 1.0, [0, 0, 0], &dir.join("expand-done-narrow.png"));
            save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("expand-done-after-wide-again.png"));
        }
        g!().invoke_d_collapse();
        settle(1200);
        assert!(!g!().get_d_wide(), "{name}: back to the small drawer");
        save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join(format!("expand-{name}-collapsed.png")));
        assert_eq!(app.page.open.get(), Some(id), "{name}: still the same session");
        assert_eq!(g!().get_d_draft().as_str(), format!("a draft for the {name} chat"), "{name}: the draft came back");
        g!().set_d_draft("".into());
        app.close_drawer();
    }
    // Memory and time: ten trips there and back on the finished chat. Hover's own resident memory
    // (the agents run as other processes), and how long one switch takes until the next frame.
    if let Some(id) = done {
        let rss = || std::fs::read_to_string("/proc/self/status").ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("VmRSS:").map(|v| v.trim().to_owned()))).unwrap_or_default();
        app.open_session(id);
        settle(600);
        let start = rss();
        let mut worst = Duration::ZERO;
        for _ in 0..10 {
            let t = std::time::Instant::now();
            g!().invoke_d_expand();
            settle(300);
            app.office_frame();
            worst = worst.max(t.elapsed().saturating_sub(Duration::from_millis(300)));
            g!().invoke_d_collapse();
            settle(300);
        }
        println!("expand chat memory: {start} before, {} after 10 round trips; slowest switch {} ms beyond the 300 ms wait", rss(), worst.as_millis());
        app.close_drawer();
    }
    *hold_c.lock().unwrap() = false;
}

/// The chat view in place of the office, through its switch: the start screen, a chat with its
/// reply bar at rest and grown, the list hidden, a narrow window, and the notch. It is kept in
/// the settings, so it is switched off again at the end and the office shots after it are as before.
fn chat_view_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path) {
    use slint::Model as _;
    let dash = adapter(1);
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    macro_rules! g { () => { app.dash.borrow().as_ref().expect("the app window").global::<Office>() } }
    let shot = |name: &str| save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join(format!("chat-view-{name}.png")));
    app.close_drawer();
    settle(600);
    shot("off");
    // The switch, as a click on it: the office goes, the start screen comes, and the choice is kept.
    g!().invoke_toggle_view();
    settle(900);
    assert!(hover.settings.chat_view() && g!().get_d_wide(), "the switch turned the chat view on, and it is kept");
    assert_eq!(app.page.shown.get(), Some(false), "the office draws nothing under the chat view");
    shot("home");
    g!().set_new_draft("Fix the login redirect: after signing in it should go back to the page you were on, not the home page.".into());
    settle(400);
    shot("home-typed");
    g!().set_new_draft("".into());
    // A chat: the reply bar is one slim line at rest, and grows with what is written.
    let done = hover.sessions.all().into_iter().find(|s| !s.busy() && !s.waiting() && !s.turns.is_empty()).map(|s| s.id);
    if let Some(id) = done {
        app.open_session(id);
        settle(1200);
        assert!(g!().get_d_wide(), "a chat opens in the chat view");
        shot("chat");
        // The header: the title being renamed, ⋯ open with its lists, and a name typed in.
        let before = hover.sessions.get(id).expect("the chat").title();
        g!().set_d_renaming(true);
        settle(300);
        shot("header-renaming");
        g!().set_d_renaming(false);
        g!().invoke_d_menu_open();
        g!().set_d_menu(true);
        settle(300);
        shot("header-menu");
        g!().set_d_fly(1);
        settle(300);
        shot("header-menu-open-in");
        g!().set_d_fly(2);
        settle(300);
        shot("header-menu-switch");
        g!().set_d_menu(false);
        g!().invoke_d_rename("".into());
        assert_eq!(hover.sessions.get(id).expect("the chat").title(), before, "an empty name keeps the title");
        g!().invoke_d_rename("A name I typed".into());
        settle(400);
        assert_eq!(hover.sessions.get(id).expect("the chat").title(), "A name I typed", "the typed name is the title");
        assert_eq!(g!().get_d_title().as_str(), "A name I typed", "the header shows it");
        assert!(g!().get_list().iter().any(|r| r.text.as_str() == "A name I typed"), "the sidebar row shows it");
        // It is kept in the history too, so the saved row and a later wake-up carry it.
        let key = hover.sessions.get(id).expect("the chat").key;
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(3) && !hover.history.as_ref().is_some_and(|h| h.entries().iter().any(|e| e.key == key && e.title == "A name I typed")) { settle(100); }
        assert!(hover.history.as_ref().is_some_and(|h| h.entries().iter().any(|e| e.key == key && e.title == "A name I typed")), "the typed name is saved in the history");
        shot("header-renamed");
        // The branch and the context chips (the fixture's folder has no branch, and no context yet).
        g!().set_d_branch("main".into());
        g!().set_d_ctx(11.0);
        settle(200);
        shot("header-chips");
        // A folder folds its chats, and unfolds them.
        let rows = g!().get_list().row_count();
        g!().invoke_list_fold(0);
        settle(300);
        assert!(g!().get_list().row_count() < rows && slint::Model::row_data(&g!().get_list(), 0).is_some_and(|r| r.head && r.shut), "a folded folder shows its row only");
        shot("sidebar-folded");
        g!().invoke_list_fold(0);
        settle(300);
        assert_eq!(g!().get_list().row_count(), rows, "unfolded, the chats are back");
        // The title bar's menus.
        for (n, name) in [(1, "file"), (2, "settings"), (3, "help")] {
            app.dash.borrow().as_ref().expect("the app window").set_bar(n);
            settle(300);
            shot(&format!("bar-{name}"));
        }
        app.dash.borrow().as_ref().expect("the app window").set_bar(0);
        g!().set_d_draft("First line of a longer reply.\nA second line.\nAnd a third, so the bar grows to fit what is written.".into());
        settle(400);
        shot("chat-long-draft");
        g!().set_d_draft("".into());
        g!().set_list_open(false);
        settle(500);
        shot("chat-no-list");
        g!().set_list_open(true);
        save(&dash, (880, 560), 1.0, [0, 0, 0], &dir.join("chat-view-chat-narrow.png"));
        // Closing the chat goes to the start screen, still in the chat view.
        g!().invoke_d_close();
        settle(500);
        assert!(hover.settings.chat_view() && app.page.open.get().is_none(), "closed: the start screen, not the office");
        // Esc doesn't leave it either.
        app.open_session(id);
        settle(500);
    }
    // The notch opens on the chat view too.
    {
        let mut n = app.n.borrow_mut();
        n.hover.opened(false);
        n.open = Openness::at(1.0);
    }
    app.notch.set_view_visible(true);
    app.notch_settings.set(false);
    app.notch.set_in_settings(false);
    app.watching_changed();
    settle(1200);
    let notch = adapter(0);
    let full = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
    assert!(app.notch.global::<Office>().get_d_wide(), "the notch shows the chat view as well");
    save_office(&notch, full, &dir.join("chat-view-notch.png"));
    app.close_drawer();
    settle(500);
    save_office(&notch, full, &dir.join("chat-view-notch-home.png"));
    app.collapse();
    // Back to the office through the switch: drawn again.
    g!().invoke_toggle_view();
    settle(900);
    assert!(!hover.settings.chat_view() && !g!().get_d_wide(), "the switch turned it off again");
    assert_eq!(app.page.shown.get(), Some(true), "the office draws again");
    shot("back-to-office");
}

/// A task started from the new-task box with the Helpers switch on works in the project folder itself, and may ask others for help.
fn new_task_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path) {
    let dash = adapter(1);
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            pump();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    hover_agents::agents::seed(AgentTool::Kiro, hover_agents::agents::AgentReady { installed: true, signed_in: true, hint: String::new() });
    let repo = std::env::temp_dir().join(format!("hover-new-task-shot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).expect("the project folder");
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    let folder = repo.to_string_lossy().into_owned();
    app.notch.global::<Office>().invoke_toggle_helpers();
    app.shot_new_task(&folder, "The whole notch, explained once more.", false);
    settle(500);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("new-task-box.png"));
    app.shot_new_task(&folder, "The whole notch, explained once more.", true);
    let t = std::time::Instant::now();
    let mine = || hover.sessions.all().into_iter().find(|s| s.folder == folder);
    while t.elapsed() < Duration::from_secs(20) && mine().is_none() { settle(100); }
    let s = mine().unwrap_or_else(|| panic!("the task started in the project folder; sessions: {:?}; running {}; can start {}; toast {:?}", hover.sessions.all().iter().map(|s| (s.folder.clone(), s.busy())).collect::<Vec<_>>(), hover.sessions.running(), hover.sessions.can_start(), app.dash.borrow().as_ref().map(|d| d.global::<Office>().get_toast().to_string())));
    // The Helpers switch is offered only where the host can serve helpers (orch::mcp_supported: not on Windows yet).
    assert_eq!(s.ext.orch.as_ref().is_some_and(|l| l.delegation), hover_agents::orch::mcp_supported(), "the Helpers switch was on where it is offered: the task may ask others for help");
    assert!(s.ext.workspace.is_none(), "no worktree or other workspace was made for it");
    println!("new task: runs in {}", s.folder);
    app.close_drawer();
    let _ = std::fs::remove_dir_all(&repo);
}

/// The chat's note strip and its More menu (#35, #36, #39), pressed through the buttons' own callbacks: a
/// replies held after Stop (send them), and continue with another agent, fork, and bring findings back.
fn chat_action_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, folder: &str, hold_c: &Arc<std::sync::Mutex<bool>>) {
    use slint::Model;
    let dash = adapter(1);
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            pump();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    macro_rules! g { () => { app.dash.borrow().as_ref().expect("the app window").global::<Office>() } }
    let until = |f: &dyn Fn() -> bool| { let t = std::time::Instant::now(); while t.elapsed() < Duration::from_secs(8) && !f() { settle(20); } assert!(f(), "waited for the state"); };
    for t in [AgentTool::Kiro, AgentTool::Codex, AgentTool::Cursor] { hover_agents::agents::seed(t, hover_agents::agents::AgentReady { installed: true, signed_in: true, hint: String::new() }); }
    if let Some(d) = &*app.dash.borrow() { d.set_in_settings(false); }
    app.dash_settings.set(false);
    app.refresh_page(false);

    // Replies held after Stop: the note offers to send them.
    *hold_c.lock().unwrap() = true;
    let live = hover.sessions.start(AgentTool::Kiro, folder, "Now make it hold when the taskbar is at the top too, and check all three monitors.", vec![]).map(|s| s.id).expect("the live task starts");
    until(&|| hover.sessions.get(live).is_some_and(|s| !s.turns.is_empty() && !s.turns[0].steps.is_empty()));
    assert!(hover.sessions.reply(live, "Use rcWork, and check monitor 3 too.", vec![]), "queued behind the run");
    hover.sessions.stop(live);
    until(&|| hover.sessions.get(live).is_some_and(|s| !s.busy()));
    app.open_session(live);
    settle(600);
    assert!(hover.sessions.get(live).unwrap().held, "Stop held the reply");
    assert!(g!().get_d_note().contains("held"), "the note: {}", g!().get_d_note());
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("chat-held-note.png"));
    *hold_c.lock().unwrap() = false;
    g!().invoke_d_note_act(0);
    settle(500);
    assert!(!hover.sessions.get(live).unwrap().held, "the held reply was let go");
    until(&|| hover.sessions.get(live).is_some_and(|s| !s.busy()));

    // The More menu on a finished chat.
    let done = hover.sessions.all().into_iter().find(|s| !s.busy() && s.cloud.is_none() && s.tool == AgentTool::Codex && s.turns.iter().any(|t| t.result.is_some())).expect("a finished Codex chat");
    app.open_session(done.id);
    settle(600);
    let at = |label: &str| g!().get_d_more_items().iter().position(|m| m.label == label).unwrap_or_else(|| panic!("{label} in the menu")) as i32;
    g!().set_d_more(true);
    settle(300);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("chat-more-menu.png"));
    let before = hover_agents::session::provider_id(&hover.sessions.get(done.id).unwrap());
    g!().invoke_d_more_act(at("Continue with Cursor"));
    settle(500);
    let after = hover_agents::session::provider_id(&hover.sessions.get(done.id).unwrap());
    assert_eq!((before.as_str(), after.as_str()), ("codex", "cursor"), "the chat moved to Cursor");
    let n = hover.sessions.all().len();
    g!().invoke_d_more_act(at("Fork this chat"));
    until(&|| hover.sessions.all().len() > n || hover.sessions.all().iter().any(|s| s.ext.lineage.as_ref().is_some_and(|l| l.fork.is_some())));
    let fork = hover.sessions.all().into_iter().find(|s| s.ext.lineage.as_ref().is_some_and(|l| l.fork.is_some())).expect("the fork");
    assert_eq!(app.page.open.get(), Some(fork.id), "the fork is the chat in front");
    settle(600);
    g!().set_d_more(true);
    settle(300);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("chat-more-menu-fork.png"));
    g!().invoke_d_more_act(at("Bring findings back to the original"));
    settle(400);
    // The More menu's helpers switch, on and off (only where helpers are offered).
    if hover_agents::orch::mcp_supported() {
        let hchat = hover.sessions.all().into_iter().find(|s| !s.busy() && s.cloud.is_none() && s.id != done.id && s.ext.orch.is_none() && Path::new(&s.folder).is_dir()).expect("a chat for the helpers switch");
        app.open_session(hchat.id);
        settle(500);
        g!().invoke_d_more_act(at("Let it ask other agents for help"));
        settle(300);
        assert!(hover.sessions.get(hchat.id).unwrap().ext.orch.is_some_and(|l| l.delegation), "helpers on");
        g!().invoke_d_more_act(at("Stop letting it ask other agents for help"));
        settle(300);
        assert!(!hover.sessions.get(hchat.id).unwrap().ext.orch.is_some_and(|l| l.delegation), "helpers off");
    }
    // Context chips: in the reply box with their ×, taken off one by one, and sent with the reply.
    let chat = hover.sessions.all().into_iter().find(|s| !s.busy() && s.cloud.is_none() && s.id != done.id && s.turns.iter().any(|t| t.result.is_some()) && Path::new(&s.folder).is_dir()).expect("a chat to reply in");
    std::fs::write(Path::new(&chat.folder).join("notes.txt"), "the rows redraw too often\n").unwrap();
    app.add_chip(chat.id, hover_agents::context::file_snapshot(&chat.folder, "notes.txt").expect("the file chip"));
    app.add_chip(chat.id, hover_agents::context::terminal("npm test", "20 passed", &chat.key, "x1").expect("the output chip"));
    app.open_session(chat.id);
    g!().set_d_draft("Explain the whole notch again, with these.".into());
    g!().set_d_compose(true);
    settle(600);
    assert_eq!(g!().get_d_chips().row_count(), 2, "both chips are in the reply box");
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("chat-chips.png"));
    g!().invoke_d_chip_remove(0);
    settle(300);
    assert_eq!((g!().get_d_chips().row_count(), app.chips_of(chat.id).len()), (1, 1), "× took one off");
    app.add_chip(chat.id, hover_agents::context::file_snapshot(&chat.folder, "notes.txt").expect("the file chip"));
    g!().invoke_d_send();
    settle(600);
    let sent = hover.sessions.get(chat.id).unwrap();
    let last = sent.turns.last().unwrap();
    assert_eq!(last.chips.len(), 2, "the reply carries its chips");
    assert!(app.chips_of(chat.id).is_empty() && g!().get_d_chips().row_count() == 0, "the box is empty after sending");
    println!("chat actions: moved to {after}, forked into {}, toast {:?}", fork.id, g!().get_toast().to_string());
    app.close_drawer();
}

/// The desk card's sessions: a turn with the steps a real one reports (commands with their
/// output, a failed one, a dev server, a page fetched, two subagents), held while it "works".
fn desk_fixture(a: &RunArgs, hold: &Arc<std::sync::Mutex<bool>>) -> Option<KiroResult> {
    use hover_agents::stream::KiroEvent;
    use hover_core::model::KiroStep;
    let p = a.prompt.as_str();
    if !p.starts_with("Desk fixture") { return None; }
    let ev = |s: KiroStep| (a.events)(KiroEvent { step: Some(s), ..Default::default() });
    let st = |id: &str, kind: &str, title: &str, target: &str, status: &str| KiroStep::new(id, kind, title, Some(target.into()), status);
    ev(KiroStep { ms: Some(900.0), ..st("r1", "read", "Read", "src/refresh.ts", "completed") });
    ev(KiroStep { added: 12, removed: 3, ms: Some(1400.0), diff: Some("  export function refresh(view) {\n-   view.draw();\n+   if (!view.dirty) return;\n+   view.draw();".into()), ..st("e1", "edit", "Edit", "src/refresh.ts", "completed") });
    ev(KiroStep { exit: Some(0), ms: Some(8200.0), output: Some("> hover@1.0.0 test\n> vitest run\n\n ✓ src/refresh.test.ts (6)\n ✓ src/view.test.ts (14)\n\n Test Files  2 passed (2)\n      Tests  20 passed (20)".into()), ..st("x1", "execute", "Run", "npm test", "completed") });
    ev(KiroStep { exit: Some(101), ms: Some(2100.0), output: Some("error[E0308]: mismatched types\n --> src/lib.rs:41:9\n  |\n41 |     let n: usize = view.rows();\n  |            -----   ^^^^^^^^^^^ expected `usize`, found `i32`\n\nerror: could not compile `hover` due to 1 previous error".into()), ..st("x2", "execute", "Run", "cargo check -p hover", "failed") });
    ev(KiroStep { output: Some("\n  VITE v5.4.0  ready in 312 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: use --host to expose".into()), ..st("x3", "execute", "Run", "npm run dev", "completed") });
    ev(KiroStep { ..st("f1", "fetch", "Fetched", "https://docs.rs/slint/latest/slint/", "completed") });
    if p.contains("busy") {
        ev(st("a1", "agent", "Subagent", "Find every caller of refresh()", "in_progress"));
        ev(st("a2", "agent", "Subagent", "Check the tests for refresh()", "in_progress"));
        ev(st("x4", "execute", "Run", "cargo build --release -p hover", "in_progress"));
        while *hold.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); }
        return Some(KiroResult::new(KiroState::Completed, "Done."));
    }
    ev(KiroStep { ms: Some(41_000.0), output: Some("Three callers: view.rs:88, panel.rs:12 and the tests. All of them pass a dirty view already.".into()), ..st("a1", "agent", "Subagent", "Find every caller of refresh()", "completed") });
    ev(KiroStep { ms: Some(9_000.0), output: Some("The tests cover refresh() with a clean view and a dirty one.".into()), ..st("a2", "agent", "Subagent", "Check the tests for refresh()", "completed") });
    Some(KiroResult::new(KiroState::Completed, "## Refresh skips clean views\n\n`refresh()` now returns early when the view isn't dirty, so the panel stops redrawing on every poll. The change is in `src/refresh.ts`, and `npm test` passes (20 tests).\n\nIt also fixes the flicker reported in https://github.com/4regab/Hover/pull/42."))
}

/// A desk's data for the panel's tabs, as git and gh would read it, with no git and no gh.
fn desk_sample(app: &Rc<App>, id: i32) {
    use crate::desk_ui::Got;
    use hover_agents::desk as d;
    let patch = "@@ -8,7 +8,9 @@ export class View {\n   private rows: Row[] = [];\n   dirty = false;\n \n-  refresh() {\n-    this.draw();\n+  refresh() {\n+    if (!this.dirty) return;\n+    this.draw();\n+    this.dirty = false;\n   }\n \n   draw() {\n@@ -40,3 +42,4 @@ export class View {\n   mark() {\n     this.dirty = true;\n+    this.rows.length = 0;\n   }";
    let file = |p: &str, st: char, add: i32, del: i32| d::FileDiff { path: p.into(), old: None, status: st, add, del, binary: false, patch: patch.into() };
    let diff = d::Diff { git: true, partial: false, branch: Some("feat/refresh".into()), truncated: false, error: None, files: vec![
        file("src/refresh.ts", 'M', 5, 2), file("src/view.test.ts", 'A', 24, 0),
        d::FileDiff { path: "assets/logo.png".into(), old: None, status: 'A', add: 0, del: 0, binary: true, patch: String::new() }] };
    let changed = |p: &str, st: char, add: i32, del: i32| d::ChangedFile { path: p.into(), status: st, old: None, add, del };
    let files = d::Files { git: true, branch: Some("feat/refresh".into()), changed: vec![changed("src/refresh.ts", 'M', 5, 2), changed("src/view.test.ts", 'A', 24, 0), changed("notes/old.md", 'D', 0, 9)],
        touched: vec![d::Touched { path: "src/refresh.ts".into(), read: 2, edit: 1 }, d::Touched { path: "src/view.ts".into(), read: 1, edit: 0 }, d::Touched { path: "package.json".into(), read: 1, edit: 0 }],
        tree: ["README.md", "package.json", "src/app.ts", "src/refresh.ts", "src/view.ts", "src/view.test.ts", "src/ui/panel.ts", "src/ui/theme.ts", "notes/old.md", "assets/logo.png"].iter().map(|s| s.to_string()).collect(), more: false, error: None };
    let probe = d::Probe { folder: true, git_installed: true, git: true, branch: Some("feat/refresh".into()), changed: 3, add: 29, del: 11, gh: true, gh_auth: true, gh_user: Some("arz".into()),
        commands: 4, agents: 2, running: 2, pages: 2, linked: 1, ..Default::default() };
    let linked = d::Linked { gh: true, prs: vec![
        d::LinkedPr { url: "https://github.com/4regab/Hover/pull/42".into(), repo: "4regab/Hover".into(), number: 42, title: Some("Fix the flicker on refresh".into()), state: Some("merged".into()), is_draft: false, additions: 31, deletions: 7, head: Some("fix/flicker".into()), error: None },
        d::LinkedPr { url: "https://github.com/4regab/Hover/pull/57".into(), repo: "4regab/Hover".into(), number: 57, title: Some("Redraw the panel only when it changed".into()), state: Some("open".into()), is_draft: true, additions: 12, deletions: 3, head: Some("feat/refresh".into()), error: None },
        d::LinkedPr { url: "https://github.com/trycua/cua/pull/9".into(), repo: "trycua/cua".into(), number: 9, title: None, state: None, is_draft: false, additions: 0, deletions: 0, head: None, error: Some("gh can’t see it".into()) }] };
    app.desk_put(id, "probe", Got::Probe(probe));
    app.desk_put(id, "files", Got::Files(files));
    app.desk_put(id, "diff", Got::Diff(diff));
    app.desk_put(id, "linked", Got::Linked(linked));
}

/// The desk card and the desk panel: a busy desk with two helpers out, the card on it and on
/// a finished one, every tab with sample data (no git, no gh, no network), the pull request
/// tab's setup and form, and the tip over a desk. Files: desk-*.png.
fn desk_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, folder: &str, hold: &Arc<std::sync::Mutex<bool>>) {
    use crate::desk_ui::Got;
    use hover_agents::desk as d;
    let notch = adapter(0);
    let desk = [0x3a, 0x4a, 0x5e];
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    let until = |f: &dyn Fn() -> bool| { let t = std::time::Instant::now(); while t.elapsed() < Duration::from_secs(5) && !f() { std::thread::sleep(Duration::from_millis(10)); } };
    // A large office, for the wide panel; the sessions of the earlier shots go.
    hover.settings.set_workspace_size(hover_core::model::WorkspaceSize::Large);
    view::Host::settings_changed(&**app);
    for s in hover.sessions.all() { hover.sessions.delete(&s.key); }
    run_for(500);
    app.close_drawer();
    app.open_panel(None);
    let busy = hover.sessions.start(AgentTool::Kiro, folder, "Desk fixture busy: make refresh() skip views that are clean", vec![]).map(|s| s.id);
    let done = hover.sessions.start(AgentTool::Codex, folder, "Desk fixture done: open a pull request for the refresh change", vec![]).map(|s| s.id);
    for id in [busy, done].into_iter().flatten() { until(&|| hover.sessions.get(id).is_some_and(|s| s.kiro_id.is_some() && s.turns.first().is_some_and(|t| t.steps.len() >= 6))); }
    let (busy, done) = (busy.unwrap_or(0), done.unwrap_or(0));
    app.office_follow();
    app.office_push();
    // The bots walk in and sit down; the busy one's subagents come out as helpers.
    settle(6000);
    let full = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
    let shot = |name: &str| save_office(&notch, full, &dir.join(name));
    shot("desk-helpers.png");
    let at = |id: i32| app.desk_tag_at(id).unwrap_or((400.0, 200.0));
    // The tip over a bot, and over a desk with a session at it.
    let g = app.notch.global::<Office>();
    let (bx, by) = at(busy);
    g.set_tip_x(bx + 30.0);
    g.set_tip_y(by + 60.0);
    g.set_over_name("Pip".into());
    g.set_over_color(slint::Color::from_rgb_u8(0x9b, 0x6b, 0xff));
    g.set_over_kind(1);
    shot("desk-tip-bot.png");
    g.set_over_kind(2);
    shot("desk-tip-desk.png");
    g.set_over_kind(0);
    // The card, on the busy desk: its live steps, the helpers out, the eight tiles; on the
    // finished one; asking; with a question. First at the Large office (desk-card-<state>),
    // then at every other size Settings offers (desk-card-<size>-<state>): Small is the
    // shortest, and a card must fit each. The last shot of each is a click in the far corner,
    // where the card is pushed back into the office.
    desk_sample(app, busy);
    desk_sample(app, done);
    let sid = hover.sessions.get(busy).and_then(|s| s.kiro_id).unwrap_or_default();
    let ask = hover_agents::ask::AgentAsk { id: "d1".into(), kind: "execute".into(), title: "Run".into(), command: Some("cargo build --release -p hover".into()), path: None,
        preview: None, added: 0, removed: 0, reason: "Builds the project".into(), danger: false, questions: None };
    // A question with choices (OpenCode, Claude Code): Skip and Answer… in place of the three.
    let question = hover_agents::ask::AgentAsk { id: "d2".into(), kind: "question".into(), title: "Question".into(), command: None, path: None, preview: None, added: 0, removed: 0,
        reason: String::new(), danger: false, questions: Some(vec![hover_agents::ask::AgentQuestion { header: "Scope".into(), question: "Should refresh() also skip views that are hidden, or only clean ones?".into(),
            options: vec![("Only clean ones".into(), "Keep the change small".into()), ("Hidden too".into(), "Also check visibility".into())], multiple: false, custom: true }]) };
    use hover_core::model::WorkspaceSize as W;
    for (ws, tag) in [(W::Large, ""), (W::Small, "small-"), (W::Default, "default-"), (W::ExtraLarge, "extra-large-")] {
        if !tag.is_empty() {
            hover.settings.set_workspace_size(ws);
            view::Host::settings_changed(&**app);
            app.office_follow();
            app.office_push();
            settle(1500);
        }
        let full = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
        let shot = |name: &str| save_office(&notch, full, &dir.join(format!("desk-card-{tag}{name}.png")));
        let (bx, by) = at(busy);
        let (dx, dy) = at(done);
        app.desk_close_card();
        app.desk_shot_card(busy, bx + 40.0, by + 20.0);
        settle(700);
        shot("working");
        // The same with a reply typed.
        app.notch.global::<Desk>().set_c_draft("Also keep the dirty flag in the tests".into());
        settle(300);
        shot("working-reply");
        app.notch.global::<Desk>().set_c_draft("".into());
        // The finished desk: the answer it gave, in a line or so.
        app.desk_close_card();
        app.desk_shot_card(done, dx + 40.0, dy + 20.0);
        settle(700);
        shot("done");
        // A permission the agent waits on, in place of the steps.
        hover.sessions.ask(AgentTool::Kiro, &sid, ask.clone(), &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
        app.desk_close_card();
        app.desk_shot_card(busy, bx + 40.0, by + 20.0);
        settle(500);
        shot("asking");
        // A click far past the office's corner: the card is held inside it.
        app.desk_close_card();
        app.desk_shot_card(busy, 5000.0, 5000.0);
        settle(500);
        shot("asking-corner");
        hover.sessions.answer(busy, "d1", hover_agents::ask::AskAnswer::Deny);
        app.desk_close_card();
        hover.sessions.ask(AgentTool::Kiro, &sid, question.clone(), &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
        app.desk_shot_card(busy, bx + 40.0, by + 20.0);
        settle(500);
        shot("question");
        hover.sessions.answer(busy, "d2", hover_agents::ask::AskAnswer::Deny);
        app.desk_close_card();
    }
    hover.settings.set_workspace_size(W::Large);
    view::Host::settings_changed(&**app);
    app.office_follow();
    app.office_push();
    settle(1500);    // The panel, tab by tab, on the finished desk (its session is idle, so Create is open).
    let tab = |name: &str, file: &str| { app.desk_shot_open(done, name); settle(700); shot(file); };
    tab("terminal", "desk-tab-terminal.png");
    // Attach a command's output to the chat: a chip in that chat's reply box.
    let term = app.desk_terminal_ids(done);
    let with_out = term.iter().find(|_| true).cloned();
    if let Some(t) = with_out {
        app.notch.global::<Desk>().invoke_act(format!("chip-term:{t}").into());
        settle(300);
        let got = app.chips_of(done);
        assert!(got.iter().any(|c| c.kind == "terminal" && c.text.as_deref().is_some_and(|t| !t.is_empty())), "the output became a chip: {got:?}");
    }
    tab("files", "desk-tab-files.png");
    app.notch.global::<Desk>().invoke_act("dir:src".into());
    app.notch.global::<Desk>().invoke_act("dir:src/ui".into());
    settle(300);
    shot("desk-tab-files-tree.png");
    app.notch.global::<Desk>().invoke_find_edited("view".into());
    settle(300);
    shot("desk-tab-files-find.png");
    app.notch.global::<Desk>().invoke_find_edited("".into());
    // A file opens, with its numbers (the sample's text is a long one, to scroll).
    let text: String = (1..=300).map(|i| if i % 7 == 0 { format!("  // line {i}: refresh() draws the rows\n") } else { format!("export const row{i} = (view: View) => view.rows[{i}];\n") }).collect();
    app.notch.global::<Desk>().invoke_act("file:src/refresh.ts".into());
    app.desk_put(done, "file", Got::File("src/refresh.ts".into(), d::FileView::Text { path: "src/refresh.ts".into(), text, truncated: false, size: 14_900 }));
    settle(500);
    shot("desk-tab-file.png");
    // The pointer over a line: Open at this line shows at its end.
    notch.dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(app.notch.get_shape_x() + 1180.0, 338.0) });
    settle(300);
    shot("desk-tab-file-hover.png");
    notch.dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(5.0, 5.0) });
    app.notch.global::<Desk>().invoke_scrolled(2000.0, 300.0);
    settle(300);
    shot("desk-tab-file-scrolled.png");
    // Attach the open file: a copy as it is now, and a reference. (The sample's file is made in the task's folder for it.)
    let task_folder = hover.sessions.get(done).map(|s| s.folder).unwrap_or_default();
    std::fs::create_dir_all(Path::new(&task_folder).join("src")).unwrap();
    std::fs::write(Path::new(&task_folder).join("src/refresh.ts"), "export const a = 1;\n").unwrap();
    app.notch.global::<Desk>().invoke_act("chip-file:src/refresh.ts".into());
    app.notch.global::<Desk>().invoke_act("chip-ref:src/refresh.ts".into());
    settle(300);
    let kinds: Vec<(String, bool)> = app.chips_of(done).iter().filter(|c| c.kind == "file").map(|c| (c.source.clone(), c.live)).collect();
    assert_eq!(kinds, [("src/refresh.ts".to_owned(), false), ("src/refresh.ts".to_owned(), true)], "a copy, then a reference");
    app.notch.global::<Desk>().invoke_act("fback".into());
    tab("diff", "desk-tab-diff.png");
    app.notch.global::<Desk>().invoke_act("chip-diff:src/refresh.ts".into());
    settle(300);
    assert!(app.chips_of(done).iter().any(|c| c.kind == "diff" && c.source == "src/refresh.ts"), "the file's change became a chip");
    // Pull request: the branch's own, with its checks.
    let pr = d::PrDetail { number: 57, title: "Redraw the panel only when it changed".into(), state: "open".into(), is_draft: true, url: "https://github.com/4regab/Hover/pull/57".into(), head: "feat/refresh".into(), base: "main".into(),
        additions: 29, deletions: 11, changed_files: 3, body: "## What changed\n\n`refresh()` returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\n- **Skips** clean views\n- Keeps the dirty flag in `View`\n- Covers a clean view and a dirty one in the tests\n\n```rust\nif !view.dirty { return; }\n```\n\nFixes the flicker from [#42](https://github.com/4regab/Hover/pull/42).".into(),
        author: Some("arz".into()), review: Some("REVIEW_REQUIRED".into()), updated_at: None, comments: 2, pass: 3, fail: 1, pending: 1, skip: 0,
        checks: vec![d::Check { name: "build (ubuntu)".into(), state: "pass".into(), url: Some("https://github.com/x".into()) }, d::Check { name: "build (windows)".into(), state: "pass".into(), url: Some("https://github.com/x".into()) },
            d::Check { name: "test".into(), state: "fail".into(), url: Some("https://github.com/x".into()) }, d::Check { name: "lint".into(), state: "pass".into(), url: None }, d::Check { name: "deploy preview".into(), state: "pending".into(), url: None }] };
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::Open(Box::new(pr.clone()))));
    tab("pr", "desk-tab-pr.png");
    // The same without its checks, so the description (headings, a list, bold, inline code, a code block, a link) is in view.
    let pr_text = d::PrDetail { checks: vec![], pass: 0, fail: 0, pending: 0, ..pr.clone() };
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::Open(Box::new(pr_text.clone()))));
    tab("pr", "desk-tab-pr-description.png");
    // The GitHub CLI's setup: one button; then its one-time code, with Copy and Open.
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::Setup { need: d::Setup::Install, message: "Set up the GitHub CLI.".into() }));
    settle(300);
    let g = app.notch.global::<Desk>();
    g.set_gh_can_start(true);
    g.set_gh_hint("".into());
    settle(300);
    shot("desk-tab-pr-setup.png");
    g.set_gh_title("Sign in to GitHub".into());
    g.set_gh_text("gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones.".into());
    g.set_gh_code("A1B2-C3D4".into());
    g.set_gh_busy(true);
    g.set_gh_line("Waiting for you to approve it on github.com…".into());
    g.set_gh_url("https://github.com/login/device".into());
    settle(300);
    shot("desk-tab-pr-code.png");
    g.set_gh_busy(false);
    g.set_gh_code("".into());
    g.set_gh_error("Sign-in didn’t finish: the code expired. Try again.".into());
    g.set_gh_button("Sign in with GitHub".into());
    settle(300);
    shot("desk-tab-pr-setup-failed.png");
    // Create pull request, on a finished desk: the form from the session's title and answer.
    let create = d::CreateInfo { branch: Some("main".into()), base: "main".into(), on_default: true, suggest: Some("hover/refresh-skips-clean-views".into()), ahead: 0, changed: 3,
        title: "Refresh skips views that are clean".into(), body: "`refresh()` now returns early when the view isn't dirty, so the panel stops redrawing on every poll.\n\nChanges: src/refresh.ts, src/view.test.ts.".into(), busy: false };
    let create_small = create.clone();
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::NoPr { message: "This branch has no pull request yet.".into(), create: create.clone() }));
    settle(500);
    shot("desk-tab-pr-create.png");
    app.desk_shot_result(done, true, None);
    settle(300);
    shot("desk-tab-pr-creating.png");
    app.desk_shot_result(done, false, Some(d::CreatePrResult { ok: false, url: None, error: Some("Couldn’t push: the remote rejected it (protected branch).".into()), steps: vec!["made the branch hover/refresh-skips-clean-views".into(), "committed 3 files".into()] }));
    settle(300);
    shot("desk-tab-pr-create-failed.png");
    app.desk_shot_result(done, false, Some(d::CreatePrResult { ok: true, url: Some("https://github.com/4regab/Hover/pull/58".into()), error: None, steps: vec![] }));
    settle(300);
    shot("desk-tab-pr-create-done.png");
    app.desk_shot_result(done, false, None);
    // The same form while the agent works in the folder: Create is off, and says why.
    app.desk_put(busy, "pr", Got::Pr(d::PrPanel::NoPr { message: "This branch has no pull request yet.".into(), create: d::CreateInfo { busy: true, ..create } }));
    app.desk_shot_open(busy, "pr");
    settle(600);
    shot("desk-tab-pr-create-busy.png");
    tab("linked", "desk-tab-linked.png");
    tab("agents", "desk-tab-agents.png");
    app.notch.global::<Desk>().invoke_act(format!("sa:{}", "a1").into());
    settle(400);
    shot("desk-tab-agents-open.png");
    tab("browser", "desk-tab-browser.png");
    // Screen: the desktop with the agent's apps, as a Mac shows it with its grant.
    let frame = image::RgbaImage::from_fn(1280, 800, |x, y| {
        let inside = (260..1020).contains(&x) && (120..700).contains(&y);
        let bar = inside && y < 150;
        if bar { image::Rgba([0x2a, 0x2a, 0x30, 255]) } else if inside { image::Rgba([0xf4, 0xf1, 0xea, 255]) } else { image::Rgba([(0x30 + y / 20) as u8, (0x40 + x / 30) as u8, 0x7a, 255]) }
    });
    app.desk_shot_open(done, "screen");
    app.desk_shot_frame(frame.clone(), false);
    settle(500);
    shot("desk-tab-screen.png");
    // The panel at Small, the shortest office (840 x 340): every tab must fit it, with the
    // description as Markdown and the pull request form (desk-tab-small-<tab>.png).
    hover.settings.set_workspace_size(W::Small);
    view::Host::settings_changed(&**app);
    app.office_follow();
    app.office_push();
    settle(1500);
    let small = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
    let tab_small = |name: &str, file: &str| { app.desk_shot_open(done, name); settle(700); save_office(&notch, small, &dir.join(format!("desk-tab-small-{file}.png"))); };
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::Open(Box::new(pr))));
    for name in ["terminal", "files", "diff", "pr", "linked", "agents", "browser"] { tab_small(name, name); }
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::Open(Box::new(pr_text))));
    tab_small("pr", "pr-description");
    app.desk_put(done, "pr", Got::Pr(d::PrPanel::NoPr { message: "This branch has no pull request yet.".into(), create: create_small }));
    tab_small("pr", "pr-create");
    tab_small("screen", "screen");
    hover.settings.set_workspace_size(W::Large);
    view::Host::settings_changed(&**app);
    app.office_follow();
    app.office_push();
    settle(1000);
    // Everything off: Create's state cleared, the panel put away, the sessions let go.
    app.desk_close_panel();
    *hold.lock().unwrap() = false;
    run_for(400);
    hover.settings.set_workspace_size(hover_core::model::WorkspaceSize::Default);
    view::Host::settings_changed(&**app);
    settle(600);
    let _ = desk;
}

/// Settings → Integrations with Computer use on, in each state of Cua Driver, and as a Mac
/// would show it (every switch on); and an agent's page with its one-click setup. Headless
/// there is no Cua Driver to ask, so each state is handed in.
fn settings_integrations_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path) {
    use hover_app::pages::{Caps, Cua, Integ, SetupCard};
    let dash = adapter(1);
    let mac = Caps { sandbox: true, browser: true, setup: true, computer_use: true, mac: true };
    // Computer use is a Mac's: its own states are shown with it on, wherever the shots run.
    let cu_on = Caps { computer_use: true, ..Caps::here() };
    hover.settings.set_theme(None);
    hover.settings.set_appearance(Appearance::Dark);
    view::Host::theme_changed(&**app);
    let show = |name: &str, integ: Integ| {
        app.pane.borrow_mut().live.integ = integ;
        app.show_settings_in(1, Section::Integrations);
        save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join(name));
    };
    hover.settings.set_computer_use(true);
    let cua = |c: Cua| Integ { caps: cu_on, cua: Some(c), ..Default::default() };
    // What this system shows: off with its note where it isn't a Mac.
    show("settings-integrations-computer-use-here.png", Integ::default());
    show("settings-integrations-cua-checking.png", Integ { caps: cu_on, ..Default::default() });
    show("settings-integrations-cua-missing.png", cua(Cua { hint: "Install Cua Driver: /bin/bash -c \"$(curl -fsSL https://cua.ai/driver/install.sh)\"".into(), ..Default::default() }));
    show("settings-integrations-cua-installing.png", cua(Cua { busy: true, line: "Installing Cua Driver…".into(), ..Default::default() }));
    show("settings-integrations-cua-ready.png", cua(Cua { installed: true, version: "0.3.1".into(), permissions: "granted".into(), ..Default::default() }));
    // As a Mac shows it: Computer use needs its grants, the sandbox lacks srt, the browser is on.
    hover.settings.set_sandbox(true);
    show("settings-integrations-as-on-a-mac.png", Integ { caps: mac, cua: Some(Cua { installed: true, version: "0.3.1".into(), permissions: "partial".into(),
        hint: "Screen Recording isn’t granted to CuaDriver, so agents can read and act on windows but not see them.".into(), ..Default::default() }),
        sandbox_missing: Some("Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78, then brew install ripgrep. (Or turn the sandbox off in Settings.)".into()), setup: vec![] });
    hover.settings.set_computer_use(false);
    // An agent's page: its setup off here with the note, and as a Mac shows it, going.
    app.pane.borrow_mut().live.integ = Integ::default();
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("settings-kiro-setup-off.png"));
    app.pane.borrow_mut().live.integ = Integ { caps: mac, setup: vec![(AgentTool::Kiro, SetupCard { busy: true, line: "Installing kiro-cli…".into(), error: None })], ..Default::default() };
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("settings-kiro-setup-going.png"));
    app.pane.borrow_mut().live.integ = Integ { caps: mac, setup: vec![(AgentTool::Kiro, SetupCard { busy: false, line: String::new(), error: Some("Couldn’t install kiro-cli: the installer exited with 1.".into()) })], ..Default::default() };
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 720), 1.0, [0, 0, 0], &dir.join("settings-kiro-setup-failed.png"));
    app.pane.borrow_mut().live.integ = Integ::default();
    // Kiro's auto compact: off (the switch alone), then on at 70 % with its choice.
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 1400), 1.0, [0, 0, 0], &dir.join("settings-kiro-compact-off.png"));
    // Through the page's own callbacks, as a click on the switches and on 70 % does.
    if let Some(d) = &*app.dash.borrow() {
        let page = d.global::<crate::ui::Page>();
        page.invoke_toggled("KiroAutoCompact".into(), true);
        page.invoke_toggled("KiroRetryBusy".into(), true);
    }
    assert!(hover.settings.kiro_auto_compact() && hover.settings.kiro_retry_busy(), "the switches took the clicks");
    hover.settings.set_kiro_compact_at(70);
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 1400), 1.0, [0, 0, 0], &dir.join("settings-kiro-compact-on.png"));
    // The slider's release goes out through the page's callback as a percent: 35 is kept; 10 is held at the floor, 20.
    if let Some(d) = &*app.dash.borrow() { d.global::<crate::ui::Page>().invoke_picked_seg("KiroCompactAt".into(), 35); }
    assert_eq!(hover.settings.kiro_compact_at(), 35, "the slider's 35 % was taken");
    if let Some(d) = &*app.dash.borrow() { d.global::<crate::ui::Page>().invoke_picked_seg("KiroCompactAt".into(), 10); }
    assert_eq!(hover.settings.kiro_compact_at(), 20, "below 20 % is held at 20");
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 1400), 1.0, [0, 0, 0], &dir.join("settings-kiro-compact-min.png"));
    // With Kiro ready the rows are live. A real press, drag and release on the slider.
    hover_agents::agents::seed(AgentTool::Kiro, hover_agents::agents::AgentReady { installed: true, signed_in: true, hint: String::new() });
    app.show_settings_in(1, Section::Kiro);
    save(&dash, (1200, 1400), 1.0, [0, 0, 0], &dir.join("settings-kiro-compact-ready.png"));
    {
        use slint::platform::{PointerEventButton, WindowEvent};
        use slint::LogicalPosition as P;
        // The track runs x 918 to 1102 at this size (the knob is centred on its ends), under
        // the credits card: half way is 60 %.
        let at = |x: f32| P::new(x, 1097.0);
        dash.dispatch_event(WindowEvent::PointerMoved { position: at(930.0) });
        dash.dispatch_event(WindowEvent::PointerPressed { position: at(930.0), button: PointerEventButton::Left });
        run_for(60);
        dash.dispatch_event(WindowEvent::PointerMoved { position: at(1010.0) });
        run_for(60);
        assert_eq!(hover.settings.kiro_compact_at(), 20, "nothing is saved while the knob is still down");
        dash.dispatch_event(WindowEvent::PointerReleased { position: at(1010.0), button: PointerEventButton::Left });
        run_for(200);
        let got = hover.settings.kiro_compact_at();
        assert!((59..=61).contains(&got), "a drag to the middle gave {got} %");
        println!("compact slider: dragged to {got} %");
        app.show_settings_in(1, Section::Kiro);
        save(&dash, (1200, 1400), 1.0, [0, 0, 0], &dir.join("settings-kiro-compact-dragged.png"));
        // Pulled left of the track, it is held at 20.
        dash.dispatch_event(WindowEvent::PointerMoved { position: at(1010.0) });
        dash.dispatch_event(WindowEvent::PointerPressed { position: at(1010.0), button: PointerEventButton::Left });
        dash.dispatch_event(WindowEvent::PointerMoved { position: at(300.0) });
        dash.dispatch_event(WindowEvent::PointerReleased { position: at(300.0), button: PointerEventButton::Left });
        run_for(200);
        assert_eq!(hover.settings.kiro_compact_at(), 20, "dragged past the left end: 20 %");
    }
    hover.settings.set_kiro_auto_compact(false);
    hover.settings.set_kiro_retry_busy(false);
}
/// Settings → Kiro's credits, from 30 made-up days to Oct 6 2026: a monthly reset on
/// Sep 20, two days Hover wasn't running (Oct 2 and 3, so Oct 4 is partial), today's
/// three sessions. Then the range at 30 days, a bar under the pointer, the quota off, and
/// no data at all.
fn settings_credits_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path) {
    use chrono::NaiveDate;
    use hover_core::ledger::{DayA, SessionCredits};
    use hover_quota::daily::Day;
    let dash = adapter(1);
    let today = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
    // Kiro's total and Hover's share, oldest first.
    let spend: [(f64, f64); 30] = [(1.9, 1.2), (2.6, 2.0), (0.4, 0.0), (0.0, 0.0), (3.1, 1.4), (2.2, 2.2), (1.7, 0.6), (2.9, 2.1), (1.1, 1.1), (0.6, 0.0),
        (3.4, 1.8), (2.4, 2.0), (1.3, 0.9), (2.8, 1.6), (3.6, 2.4), (0.9, 0.3), (0.2, 0.0), (2.7, 1.9), (2.2, 1.5), (3.3, 2.6),
        (1.8, 1.0), (2.5, 2.1), (4.4, 2.0), (1.6, 1.2), (2.1, 1.7), (0.0, 0.0), (0.0, 0.0), (3.9, 1.6), (2.3, 1.4), (3.24, 2.10)];
    let (mut used, mut days, mut a) = (31.0, vec![], std::collections::BTreeMap::new());
    for (k, (t, hv)) in spend.iter().enumerate() {
        let date = today - chrono::Duration::days(29 - k as i64);
        let reset_day = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        if date == reset_day { used = 0.0; }
        let before = used;
        used += t;
        a.insert(date, DayA { credits: *hv, turns: 1, sessions: vec![] });
        // No reading on Oct 2 and 3; Oct 4's first came part way through it.
        if (3..=4).contains(&(today - date).num_days()) { continue; }
        let first = if (today - date).num_days() == 2 { used - 2.6 } else { before };
        days.push(Day { date, first, used, limit: 50.0, reset: Some(if date < reset_day { "09/20" } else { "10/20" }.into()), plan: Some("KIRO PRO".into()), at: String::new() });
    }
    let s = |t: &str, f: &str, c: f64| SessionCredits { key: t.into(), title: t.into(), folder: f.into(), credits: c };
    a.get_mut(&today).unwrap().sessions = vec![s("Fix login redirect", "C:\\work\\Hover\\app", 1.20), s("Add CSV export", "/home/me/billing-svc", 0.64), s("Tidy the imports", "/home/me/project", 0.26)];
    hover.credits.pin(hover_quota::credits::combine(&a, &days, today));
    hover.settings.set_theme(None);
    let shot = |name: &str| { app.show_settings_in(1, Section::Kiro); save(&dash, (1200, 1000), 1.0, [0, 0, 0], &dir.join(name)); };
    for (dark, tag) in [(true, "dark"), (false, "light")] {
        hover.settings.set_appearance(if dark { Appearance::Dark } else { Appearance::Light });
        view::Host::theme_changed(&**app);
        shot(&format!("settings-kiro-credits-{tag}.png"));
    }
    hover.settings.set_appearance(Appearance::Dark);
    view::Host::theme_changed(&**app);
    if let Some(d) = &*app.dash.borrow() { d.global::<crate::ui::Page>().invoke_picked_seg(hover_app::pages::CREDITS_RANGE.into(), 1); }
    assert_eq!(app.pane.borrow().live.credits_range, 1, "the range took the pick");
    shot("settings-kiro-credits-30-days.png");
    // The pointer over a bar: its day in place of the legend.
    {
        use slint::platform::WindowEvent;
        let (x, y) = std::env::var("HOVER_CREDITS_AT").ok().and_then(|v| { let (x, y) = v.split_once(',')?; Some((x.parse().ok()?, y.parse().ok()?)) }).unwrap_or((1080.0f32, 300.0f32));
        dash.dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(x, y) });
        run_for(60);
        save(&dash, (1200, 1000), 1.0, [0, 0, 0], &dir.join("settings-kiro-credits-hover.png"));
        dash.dispatch_event(WindowEvent::PointerExited);
    }
    app.pane.borrow_mut().live.credits_range = 0;
    // The quota off: Hover's numbers, Kiro's own as dashes and why.
    hover.settings.set_notch_item("kiro", false);
    shot("settings-kiro-credits-quota-off.png");
    hover.settings.set_notch_item("kiro", true);
    hover.credits.pin(hover_quota::credits::combine(&Default::default(), &[], today));
    shot("settings-kiro-credits-empty.png");
}

/// A voice preview as Voice makes one, for the shots.
fn preview(folder: &str, target: &str, note: Option<&str>, task: &str, countdown: Option<f32>, access: &str) -> hover_app::voice::Preview {
    hover_app::voice::Preview {
        id: 1, heard: "go to hover and fix the notch blink on the second monitor when the taskbar is at the top".into(), cleanup_note: None,
        task: task.into(), folder: folder.into(), target_name: target.into(), note: note.map(Into::into), tool: AgentTool::Codex,
        model: String::new(), access: access.into(), countdown, trial: false, cloud: false, repo: Default::default(),
    }
}

/// Voice's card in the resting notch, every stage, at every office size (the window is
/// as wide as the office; the card is the same in each), 1x and at Default 2x too.
/// The stages are drawn as Voice would hand them over (voice_ui's shot override): no
/// microphone, no speech service, no agent.
fn voice_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, data: &Path) {
    use hover_app::voice::{Pending, Stage};
    use hover_core::model::WorkspaceSize as W;
    let notch = adapter(0);
    let desk = [0x3a, 0x4a, 0x5e];
    let proj = data.join("Hover");
    std::fs::create_dir_all(&proj).unwrap();
    let pf = proj.to_string_lossy().into_owned();
    if let Ok(mut p) = hover.settings.add_project(&pf) {
        p.aliases = vec!["hover".into(), "the notch app".into()];
        p.voice = true;
        p.access = "full".into();
        let _ = hover.settings.update_project(p);
    }
    let home = data.join("Workspace").to_string_lossy().into_owned();
    let session = hover.sessions.all().first().map_or(0, |s| s.id);
    let task = "Fix the notch blink on the second monitor with a top taskbar";
    let long = "Fix the notch blink on the second monitor with a top taskbar. Then check it on all three monitors, with the taskbar on each edge, at 100, 125 and 150 % scale, and write down which ones still blink and why, so we can decide whether the DPI fix is enough or the window has to be placed again after every display change.";
    let states: Vec<(&str, Stage)> = vec![
        ("listening", Stage::Recording { level: 0.6, secs: 4.2 }),
        ("loading", Stage::Loading),
        ("transcribing", Stage::Transcribing),
        ("resolving", Stage::Resolving),
        ("preview", Stage::Preview(preview(&pf, "Hover", None, task, Some(2.1), "full"))),
        ("preview-default-workspace", Stage::Preview(preview(&home, "Default workspace", Some("Using default workspace: no clear project match. It will be made when the task starts."), task, Some(0.9), "risky"))),
        ("preview-kiro-cloud", Stage::Preview(hover_app::voice::Preview { tool: AgentTool::Kiro, cloud: true, ..preview(&pf, "Hover", None, task, Some(2.1), "risky") })),
        ("editing", Stage::Editing(preview(&pf, "Hover", None, long, None, "full"))),
        ("starting", Stage::Starting(preview(&pf, "Hover", None, task, None, "full"))),
        ("choose-agent", Stage::ChooseAgent(Pending { id: 1, text: task.into(), tools: vec![AgentTool::Codex, AgentTool::OpenCode] })),
        ("started", Stage::Started { session, folder: pf.clone() }),
        ("cancelled", Stage::Cancelled),
        ("error", Stage::Error { message: "Groq couldn’t be reached. Check the connection, then try again.".into(), retry: true, transcript: Some(task.into()) }),
        ("error-setup", Stage::Error { message: "Set up local speech in Settings → Voice.".into(), retry: false, transcript: None }),
    ];
    let draw = |st: &Stage| {
        *app.voice_ui.shot.borrow_mut() = Some(st.clone());
        app.update_rest();
        // A frame makes the card's new rows, so its height is known (on screen the next
        // frame does this, and the poll springs the shape to it).
        let sz = notch.size();
        let mut buf = vec![PremultipliedRgbaColor::default(); (sz.width * sz.height) as usize];
        notch.request_redraw();
        notch.draw_if_needed(|r| { r.render(&mut buf, sz.width as usize); });
        run_for(100);
        app.notch.global::<Clock>().set_t(0.35);
        // The card's height is known once Slint has laid it out (the poll reads it again).
        app.update_rest();
        run_for(700);
    };
    for (ws, tag) in [(W::Small, "small"), (W::Default, "default"), (W::Large, "large"), (W::ExtraLarge, "extra-large")] {
        hover.settings.set_workspace_size(ws);
        view::Host::settings_changed(&**app);
        run_for(300);
        let w = app.n.borrow().win.width() as u32;
        for (name, st) in &states {
            draw(st);
            save(&notch, (w, 200), 1.0, desk, &dir.join(format!("voice-{tag}-{name}.png")));
            if ws == W::Default { save(&notch, (w, 200), 2.0, desk, &dir.join(format!("voice-{tag}-{name}-2x.png"))); }
        }
        if ws == W::Default {
            // "Take a screenshot" while listening: the flash, then the note; then the preview with the
            // pictures it will send, each with its ×. Two made-up screens stand in for real ones.
            let pics: Vec<String> = [([0x1e, 0x29, 0x3b], [0x4a, 0xde, 0x80]), ([0xf6, 0xf2, 0xff], [0x6b, 0xa8, 0xff])].iter().enumerate().map(|(i, (bg, bar))| {
                let mut img = image::RgbaImage::from_pixel(1600, 1000, image::Rgba([bg[0], bg[1], bg[2], 255]));
                for y in 0..60 { for x in 0..1600 { img.put_pixel(x, y, image::Rgba([bar[0], bar[1], bar[2], 255])); } }
                for y in 200..700 { for x in 200..1000 { img.put_pixel(x, y, image::Rgba([0x80, 0x80, 0x90, 255])); } }
                let f = data.join(format!("voice-shot-{i}.png"));
                img.save(&f).unwrap();
                f.to_string_lossy().into_owned()
            }).collect();
            let listening = Stage::Recording { level: 0.6, secs: 6.8 };
            *app.voice_ui.shot_pics.borrow_mut() = Some(pics[..1].to_vec());
            draw(&listening);
            app.shot_feedback(hover_app::voice::SHOT_TAKEN);
            run_for(40);
            save(&notch, (w, 200), 2.0, desk, &dir.join(format!("voice-{tag}-screenshot-flash-2x.png")));
            run_for(700);
            save(&notch, (w, 200), 2.0, desk, &dir.join(format!("voice-{tag}-screenshot-note-2x.png")));
            *app.voice_ui.shot_pics.borrow_mut() = Some(pics.clone());
            draw(&Stage::Preview(preview(&pf, "Hover", None, "Fix the footer: it overlaps the menu on narrow screens.", Some(2.1), "full")));
            let h = app.n.borrow().win.height() as u32;
            save(&notch, (w, h), 2.0, desk, &dir.join(format!("voice-{tag}-preview-screenshots-2x.png")));
            app.voice_ui.shot_pics.borrow_mut().take();
            app.notch.set_voice_note("".into());
        }
        // The tallest the preview gets: the agent menu open over a long task with a note.
        // It stays inside the window (Small's is the shortest), Start and Cancel in view.
        draw(&Stage::Preview(preview(&home, "Default workspace", Some("Using default workspace: no project named. Cleanup failed; using the original."), long, None, "full")));
        *app.voice_ui.ready.borrow_mut() = Some((1, vec![AgentTool::Kiro, AgentTool::Codex, AgentTool::Cursor, AgentTool::OpenCode, AgentTool::Claude]));
        app.notch.invoke_voice_open_menu(1);
        let sz = notch.size();
        let mut buf = vec![PremultipliedRgbaColor::default(); (sz.width * sz.height) as usize];
        notch.request_redraw();
        notch.draw_if_needed(|r| { r.render(&mut buf, sz.width as usize); });
        run_for(100);
        app.update_rest();
        run_for(700);
        let h = app.n.borrow().win.height() as u32;
        save(&notch, (w, h), 1.0, desk, &dir.join(format!("voice-{tag}-menu-long.png")));
        app.notch.invoke_voice_open_menu(0);
        app.voice_ui.ready.borrow_mut().take();
        // Kiro Web: no folder pick, and the repo menu with its search box.
        if ws == W::Default {
            draw(&states[6].1);
            save(&notch, (w, 200), 1.0, desk, &dir.join("voice-default-cloud-no-folder.png"));
            app.cloud_shot(None, vec!["4regab/hoverweb".into(), "4regab/Hover".into(), "4regab/tasksync-mcp".into()]);
            for (q, name) in [("", "voice-default-cloud-repos.png"), ("HOV", "voice-default-cloud-repos-search.png")] {
                if q.is_empty() { app.notch.invoke_voice_open_menu(4); } else { type_text(&notch, q); }
                let sz = notch.size();
                let mut buf = vec![PremultipliedRgbaColor::default(); (sz.width * sz.height) as usize];
                notch.request_redraw();
                notch.draw_if_needed(|r| { r.render(&mut buf, sz.width as usize); });
                run_for(100);
                app.update_rest();
                run_for(700);
                let h = app.n.borrow().win.height() as u32;
                save(&notch, (w, h), 1.0, desk, &dir.join(name));
            }
            app.notch.invoke_voice_open_menu(0);
        }
    }
    // The aura in a colour picked in Settings → Voice.
    hover.settings.set_voice(hover_core::projects::VoiceSettings { aura_color: Some("#C4A2FF".into()), ..hover.settings.voice() });
    draw(&states[0].1);
    let w = app.n.borrow().win.width() as u32;
    save(&notch, (w, 200), 2.0, desk, &dir.join("voice-listening-violet-2x.png"));
    hover.settings.set_voice(hover_core::projects::VoiceSettings { aura_color: None, ..hover.settings.voice() });
    // A press while one is in progress: the card glows amber a moment.
    draw(&states[4].1);
    app.notch.set_voice_busy(true);
    app.update_rest();
    run_for(300);
    let w = app.n.borrow().win.width() as u32;
    save(&notch, (w, 200), 1.0, desk, &dir.join("voice-extra-large-busy.png"));
    app.notch.set_voice_busy(false);
    *app.voice_ui.shot.borrow_mut() = None;
    hover.settings.set_workspace_size(W::Default);
    view::Host::settings_changed(&**app);
    app.update_rest();
    run_for(700);
}

/// Settings' new pages as the app fills them: Projects with registered projects, one
/// project, Voice in Cloud and in Local with the Phonon card in each state, and Try it's
/// answer. The card's states are set as Phonon reports them; its facts are the real pins.
fn settings_voice_shots(app: &Rc<App>, hover: &Arc<hover_app::app::Hover>, dir: &Path, data: &Path) {
    use hover_app::pages::PhononAction as A;
    use hover_core::projects::{SpeechMode, VoiceSettings};
    let dash = adapter(1);
    // Dark, as the mockup is.
    hover.settings.set_theme(None);
    hover.settings.set_appearance(Appearance::Dark);
    view::Host::theme_changed(&**app);
    let blog = data.join("blog");
    std::fs::create_dir_all(&blog).unwrap();
    if let Ok(mut p) = hover.settings.add_project(&blog.to_string_lossy()) {
        p.name = "Blog".into();
        p.aliases = vec!["the blog".into()];
        p.access = "always".into();
        let _ = hover.settings.update_project(p);
    }
    let shot = |name: &str| {
        app.refresh_page(false);
        // Voice's page is long: tall enough to show it all (the page has no scroll from here).
        let h = if name.starts_with("voice") { 1500 } else { 620 };
        save(&dash, (1200, h), 1.0, [0, 0, 0], &dir.join(format!("settings-{name}.png")));
        save(&dash, (840, h), 1.0, [0, 0, 0], &dir.join(format!("settings-{name}-narrow.png")));
    };
    app.show_settings_in(1, Section::Projects);
    shot("projects-registered");
    let first = hover.settings.projects().first().map(|p| p.id.clone());
    app.pane.borrow_mut().project = first;
    shot("project-page");
    app.pane.borrow_mut().project = None;
    hover.settings.set_voice(VoiceSettings { enabled: true, speech: SpeechMode::Cloud, ..hover.settings.voice() });
    app.show_settings_in(1, Section::Voice);
    shot("voice-cloud");
    hover.settings.set_voice(VoiceSettings { speech: SpeechMode::Local, ..hover.settings.voice() });
    let real = crate::voice_ui::phonon_card(&app.phonon, None);
    let card = |state: &str, progress: Option<(u64, Option<u64>)>, actions: Vec<A>, error: Option<&str>| hover_app::pages::PhononCard {
        state: state.into(), progress, actions, error: error.map(Into::into), ..real.clone()
    };
    let total = app.phonon.facts().download_bytes;
    for (name, c) in [
        ("not-installed", card("Not installed", None, vec![A::Download], None)),
        ("downloading", card("Downloading", Some((total * 41 / 100, Some(total))), vec![A::Cancel], None)),
        ("verifying", card("Verifying…", None, vec![A::Cancel], None)),
        ("installing", card("Installing…", None, vec![A::Cancel], None)),
        ("ready", card("Ready", None, vec![A::Repair, A::Remove], None)),
        ("cancelled", card("Cancelled", None, vec![A::Retry], None)),
        ("failed", card("Failed", None, vec![A::Retry, A::Remove], Some("A download didn’t match its checksum (torch-2.8.0+cpu). Nothing was kept; the install you had still works."))),
        ("unsupported-vcredist", card("Can’t run on this computer", None, vec![A::Download],
            Some("Phonon needs the Microsoft Visual C++ Redistributable (x64). Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe, then press Download again."))),
    ] {
        app.pane.borrow_mut().live.phonon = Some(c);
        shot(&format!("voice-local-{name}"));
    }
    // Try it: what voice would start, without starting it.
    app.pane.borrow_mut().live.phonon = Some(real.clone());
    let p = hover_app::voice::Preview { trial: true, ..preview(&data.join("Hover").to_string_lossy(), "Hover", None, "Fix the notch blink on the second monitor with a top taskbar", None, "full") };
    app.pane.borrow_mut().live.voice_try = crate::voice_ui::try_card(&hover_app::voice::Stage::Preview(p), &hover.settings);
    shot("voice-try-done");
    app.pane.borrow_mut().live.voice_try = crate::voice_ui::try_card(&hover_app::voice::Stage::Recording { level: 0.5, secs: 2.4 }, &hover.settings);
    shot("voice-try-listening");
    app.pane.borrow_mut().live.voice_try = None;
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let data = std::env::temp_dir().join(format!("hover-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(data.join("project")).unwrap();
    std::env::set_var("HOVER_DATA_DIR", &data);
    slint::platform::set_platform(Box::new(Headless)).unwrap();

    let settings = hover_core::settings::Settings::load(hover_core::paths::settings_file());
    for id in ["claude", "kiro", "codex", "cursor"] { settings.set_notch_item(id, true); }
    settings.set_kiro_folder(Some(&data.join("project").to_string_lossy()));
    // Sessions that work until told otherwise, and quotas at every level.
    let hold = Arc::new(std::sync::Mutex::new(true));
    let h2 = hold.clone();
    // A third task holds on its own until the question in it has been shot.
    let hold3 = Arc::new(std::sync::Mutex::new(true));
    let h3 = hold3.clone();
    // The chat's own fixtures (thinking, subagents, a question) hold until shot.
    let hold_c = Arc::new(std::sync::Mutex::new(true));
    let hc = hold_c.clone();
    // The desk card's sessions work until told otherwise.
    let hold_d = Arc::new(std::sync::Mutex::new(true));
    let hd = hold_d.clone();
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let run: RunTask = Arc::new(move |a: RunArgs| {
        use hover_agents::stream::KiroEvent;
        use hover_core::model::KiroStep;
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (a.events)(KiroEvent { session_id: Some(format!("s{k}")), ..Default::default() });
        (a.progress)(hover_agents::stream::KiroPhase::Reading);
        if let Some(r) = chat_fixture(&a, &hc) { return r; }
        if let Some(r) = desk_fixture(&a, &hd) { return r; }
        // The steps a real turn reports: reads, an edit with its change, a command with its output.
        let step = |id: &str, kind: &str, title: &str, target: &str| KiroStep::new(id, kind, title, Some(target.into()), "completed");
        (a.events)(KiroEvent { step: Some(step("r1", "read", "Read", "src/app/imports.ts")), ..Default::default() });
        (a.events)(KiroEvent { step: Some(step("r2", "read", "Read", "src/app/sort.ts")), ..Default::default() });
        (a.events)(KiroEvent { step: Some(KiroStep { added: 3, removed: 1, ms: Some(1400.0),
            diff: Some("  export function tidy(files) {\n- return files;\n+ return files\n+   .map(sortImports)\n+   .filter(Boolean);".into()), ..step("e1", "edit", "Edit", "src/app/imports.ts") }), ..Default::default() });
        (a.events)(KiroEvent { step: Some(KiroStep { exit: Some(0), ms: Some(8200.0), output: Some("✓ 14 files sorted\nTests: 42 passed, 42 total".into()), ..step("x1", "execute", "Run", "npm test") }), ..Default::default() });
        // What the turn cost, as Kiro says at its end (the chat shows it under the answer).
        (a.events)(KiroEvent { credits: Some(0.087), ..Default::default() });
        // A picture's answer: an image from the session's own folder, under its words.
        if a.prompt.contains("mock-up") {
            return KiroResult::new(KiroState::Completed, "## Chart restyled\n\nThe bars follow your mock-up now:\n\n![The new chart](chart.png)\n\nColours come from the theme.");
        }
        let hold = if a.prompt.contains("three") { &h3 } else { &h2 };
        while *hold.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); }
        KiroResult::new(KiroState::Completed, "## Imports tidied\n\nAll 14 files now sort their imports.\n\n```ts\nexport const tidy = (f) => f.map(sortImports);\n```")
    });
    let reader = Arc::new(|id: &str| match id {
        "claude" => hover_quota::Reading { used: Some(37.5), detail: "Max · 5h 18% · week 38% · resets 14:00".into() },
        "kiro" => hover_quota::Reading { used: Some(82.0), detail: "KIRO PRO · 41 of 50 credits · resets 10/01".into() },
        "codex" => hover_quota::Reading::fail("Codex hasn’t recorded any limits yet — use it once."),
        _ => hover_quota::Reading { used: Some(95.0), detail: "Pro · 95% of plan · resets 3 Oct".into() },
    });
    // Kept in the history, so its panel has rows (with their dates) to show.
    let history = hover_core::crypto::global().map(|c| Arc::new(hover_core::history::AgentHistory::new(hover_core::paths::agents(), c)));
    let hover = hover_app::app::Hover::with(settings, history, vec![], Some(run), Some(reader));
    // Kiro's credits card is as tall in every Kiro shot (the compact slider's drag hits
    // it where it is): no days, until the credits shots pin their own.
    hover.credits.pin(hover_quota::credits::combine(&Default::default(), &[], chrono::Local::now().date_naive()));
    let app = App::new(hover.clone(), Box::new(Plain), Look { dark: true, animations: true }, true);
    // The software renderer doesn't clip to rounded corners: the glass's blurred copy
    // of the scene would show as a square behind each rounded panel.
    app.notch.global::<Backdrop>().set_live(false);
    let notch = adapter(0);
    let size = |a: &App| { let n = a.n.borrow(); ((n.win.width()) as u32, (n.win.height()) as u32) };
    let desk = [0x3a, 0x4a, 0x5e];

    hover.refresh_quotas(true);
    run_for(300);
    let folder = data.join("project").to_string_lossy().into_owned();
    hover.sessions.start(AgentTool::Kiro, &folder, "Tidy the imports", vec![]);
    hover.sessions.start(AgentTool::Codex, &folder, "Look for dead code", vec![]);
    run_for(300);
    app.update_rest();
    app.notch.global::<Clock>().set_t(1.3);
    let full = size(&app);
    let crop = (full.0, 60);
    save(&notch, crop, 2.0, desk, &dir.join("notch-rest-pill-2x.png"));

    // Ends nobody saw: the tool's logo with its badge, and the task. (Headless there is
    // no event loop for the hooks to post to, so what they would do is done here.)
    *hold.lock().unwrap() = false;
    run_for(400);
    app.update_rest();
    run_for(500);
    save(&notch, crop, 2.0, desk, &dir.join("notch-rest-done-2x.png"));
    // Two agents at work and a question: the amber island, then its card.
    *hold.lock().unwrap() = true;
    hover.sessions.start(AgentTool::Kiro, &folder, "Tidy the imports again", vec![]);
    hover.sessions.start(AgentTool::Cursor, &folder, "Look for dead code again", vec![]);
    run_for(300);
    app.hover.seen();
    app.update_rest();
    run_for(500);
    app.notch.global::<Clock>().set_t(0.4);
    save(&notch, crop, 2.0, desk, &dir.join("notch-rest-working-2x.png"));
    let asker = hover.sessions.all().into_iter().find(|s| s.busy()).unwrap();
    let ask = hover_agents::ask::AgentAsk { id: "n1".into(), kind: "execute".into(), title: "Run".into(), command: Some("npm install three@0.171.0".into()), path: None,
        preview: None, added: 0, removed: 0, reason: "Installs packages or uses the network".into(), danger: false, questions: None };
    hover.sessions.ask(asker.tool, asker.kiro_id.as_deref().unwrap_or(""), ask, &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
    app.update_rest();
    run_for(700);
    save(&notch, (full.0, 60), 2.0, desk, &dir.join("notch-rest-ask-2x.png"));
    app.open_card();
    run_for(100);
    // The card's height is known once Slint has laid it out (the poll reads it again).
    app.update_rest();
    run_for(700);
    save(&notch, (full.0, 220), 2.0, desk, &dir.join("notch-rest-card-2x.png"));
    app.answer_asked(hover_agents::ask::AskAnswer::Deny);
    run_for(700);
    if !skip("voice") { voice_shots(&app, &hover, dir, &data); }

    { let mut n = app.n.borrow_mut(); n.hover.opened(false); n.open = Openness::at(1.0); }
    app.hover.seen();
    app.update_rest();
    // The office: its thread renders, the frames are taken here (no event loop to post to).
    app.office_follow();
    let settle = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            slint::platform::update_timers_and_animations();
            app.office_frame();
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    app.office_push();
    settle(3000);
    // The note before the first task, in place of the office.
    save(&notch, full, 1.0, desk, &dir.join("office-notice.png"));
    hover.settings.set_kiro_notice_seen(true);
    app.office_widgets();
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("notch-open-office.png"));
    let first = app.hover.sessions.all().first().map(|s| s.id);
    if let Some(id) = first { app.open_session(id); }
    settle(1500);
    save(&notch, full, 1.0, desk, &dir.join("office-drawer.png"));
    app.close_drawer();
    app.open_panel(Some("board"));
    settle(1200);
    save(&notch, full, 1.0, desk, &dir.join("office-panel-board.png"));
    app.open_panel(Some("tv"));
    settle(1200);
    save(&notch, full, 1.0, desk, &dir.join("office-panel-tv.png"));
    app.open_panel(Some("history"));
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-panel-history.png"));
    // Kiro Web sessions made elsewhere, in the same list by date, with the cloud mark.
    {
        use hover_agents::acp::CloudSession;
        let ago = |h: f64| Some(hover_core::time::Stamp::now().add_secs(-h * 3600.0));
        app.web_shot(vec![], "Kiro listed 4 for Kiro Web and 4 for this computer. They are the same, so Hover can’t tell which are Kiro Web’s. It offers sessionSources: local/remote.");
        settle(400);
        save(&notch, full, 1.0, desk, &dir.join("office-panel-history-web-none.png"));
        app.web_shot(vec![CloudSession { id: "w1".into(), title: "Fix the checkout total on mobile".into(), updated: ago(0.5) },
            CloudSession { id: "w2".into(), title: "Write the release notes for 3.7".into(), updated: ago(30.0) }, CloudSession { id: "w3".into(), title: String::new(), updated: None }], "");
        settle(600);
        save(&notch, full, 1.0, desk, &dir.join("office-panel-history-web.png"));
        app.web_shot(vec![], "");
    }
    // Long titles wrap to two lines in the board's cards; the next card must start below.
    *hold.lock().unwrap() = false;
    run_for(600);
    for x in hover.sessions.all() { hover.sessions.dismiss(x.id); }
    hover.sessions.start(AgentTool::Kiro, &folder, "is cloudflare good replacement for vercel since we cant use the free plan for a team project anymore", vec![]);
    run_for(600);
    hover.sessions.start(AgentTool::Kiro, &folder, "Can you work on the Checker Project again on KiroWeb?", vec![]);
    run_for(900);
    app.open_panel(Some("board"));
    settle(1200);
    save(&notch, full, 1.0, desk, &dir.join("office-panel-board-long.png"));
    app.open_panel(None);
    app.notch.global::<Office>().invoke_fab_main();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-pick.png"));
    app.notch.global::<Office>().invoke_pick_tool(0);
    app.notch.global::<Office>().set_new_draft("Add a dark mode to the settings page".into());
    app.office_widgets();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-open.png"));
    // Kiro Web: the cloud switch on, the repo in place of the folder, then the repo menu.
    app.notch.global::<Office>().invoke_toggle_cloud();
    app.cloud_shot(Some("4regab/hoverweb"), vec!["4regab/hoverweb".into(), "4regab/Hover".into(), "4regab/tasksync-mcp".into()]);
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-cloud.png"));
    app.notch.global::<Office>().invoke_open_repos();
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-cloud-repos.png"));
    // The search box: typing keeps the repositories that match.
    type_text(&notch, "HOV");
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-cloud-repos-search.png"));
    app.notch.global::<Office>().invoke_open_repos();
    app.notch.global::<Office>().invoke_toggle_cloud();
    settle(300);
    // A long task: the box grows to its cap, then scrolls with the caret (Ctrl+End).
    app.notch.global::<Office>().set_new_draft(LONG_PROMPT.into());
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-long-prompt-top.png"));
    key(&notch, true, slint::platform::Key::End);
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-long-prompt-end.png"));
    app.notch.global::<Office>().set_new_draft("Add a dark mode to the settings page".into());
    settle(300);
    // A long model name with its effort: Start must stay inside the box.
    let kiro = hover.settings.agent_options(AgentTool::Kiro);
    hover.settings.set_agent_options(AgentTool::Kiro, hover_core::model::AgentOptions { model: Some("claude-sonnet-4.6".into()), effort: Some("high".into()), ..kiro.clone() });
    app.office_widgets();
    // The folder label as long as the 40 % it may take (as "project dir Aü" in a deep path).
    app.notch.global::<Office>().set_new_folder("…/work/clients/project dir Aü".into());
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-open-long-model.png"));
    hover.settings.set_agent_options(AgentTool::Kiro, kiro);
    app.office_widgets();
    app.notch.global::<Office>().invoke_new_fold();
    app.toast("In Hover this opens a folder picker.");
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-toast.png"));
    // The menu (time of day, music, history, Settings), and the new task's access menu.
    app.notch.global::<Office>().invoke_toggle_menu();
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-menu.png"));
    app.notch.global::<Office>().invoke_toggle_menu();
    app.notch.global::<Office>().invoke_fab_main();
    app.notch.global::<Office>().invoke_pick_tool(1);
    app.notch.global::<Office>().invoke_open_access();
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-access-menu.png"));
    app.notch.global::<Office>().invoke_open_access();
    app.notch.global::<Office>().invoke_new_fold();
    // A question: an agent under Ask first wants to run a command.
    let s3 = hover.sessions.start_as(AgentTool::Codex, &folder, "Upgrade three.js to 0.171", vec![], Some("risky"));
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_secs(3) && s3.as_ref().and_then(|s| hover.sessions.get(s.id)).is_none_or(|s| s.kiro_id.is_none()) { std::thread::sleep(Duration::from_millis(10)); }
    if let Some(s3) = &s3 {
        let sid = hover.sessions.get(s3.id).and_then(|s| s.kiro_id).unwrap_or_default();
        let ask = hover_agents::ask::AgentAsk { id: "a1".into(), kind: "execute".into(), title: "Run".into(), command: Some("npm install three@0.171.0".into()), path: None,
            preview: None, added: 0, removed: 0, reason: "Installs packages or uses the network".into(), danger: false, questions: None };
        hover.sessions.ask(AgentTool::Codex, &sid, ask, &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
    }
    app.office_push();
    settle(2500);
    save(&notch, full, 1.0, desk, &dir.join("office-ask-over.png"));
    if let Some(s3) = &s3 { app.open_session(s3.id); }
    settle(1500);
    save(&notch, full, 1.0, desk, &dir.join("office-ask-chat.png"));
    // OpenCode: its models with their own variants, and a question with its choices.
    if let Some(s3) = &s3 { if let Some(q) = hover.sessions.get(s3.id).and_then(|s| s.asking().map(|q| q.id.clone())) { hover.sessions.answer(s3.id, &q, hover_agents::ask::AskAnswer::Deny); } }
    app.close_drawer();
    // Three run at most: the question's task ends, so OpenCode's can start.
    *hold3.lock().unwrap() = false;
    run_for(400);
    let offers = hover_agents::opencode::offers(
        &hover_core::json::parse(r#"{"providers":[{"id":"anthropic","name":"Anthropic","models":{"claude-sonnet-5":{"name":"Claude Sonnet 5","variants":{"high":{},"max":{}}},"claude-haiku-4.5":{"name":"Claude Haiku 4.5"}}},{"id":"opencode","name":"OpenCode Zen","models":{"big-pickle":{"name":"Big Pickle"}}}]}"#).unwrap(),
        &hover_core::json::parse(r#"[{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"}]"#).unwrap());
    hover.settings.set_agent_offers(AgentTool::OpenCode, &offers);
    hover.settings.set_agent_options(AgentTool::OpenCode, hover_core::model::AgentOptions { model: Some("anthropic/claude-sonnet-5".into()), effort: Some("max".into()), ..Default::default() });
    let g = app.notch.global::<Office>();
    g.invoke_fab_main();
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-pick-opencode.png"));
    g.invoke_pick_tool(3);
    settle(300);
    g.invoke_open_model(2, 330.0, (full.1 as f32) - 60.0);
    settle(400);
    save(&notch, full, 1.0, desk, &dir.join("office-model-menu-opencode.png"));
    g.invoke_open_model(0, 0.0, 0.0);
    // A long model list (Codex's model/list gives one), the last model picked: the menu scrolls to it,
    // shows a bar, and the efforts stay in view. Then the shorter list again.
    let codex: Vec<hover_core::model::AcpChoice> = ["GPT-5.6 Sol", "GPT-5.6 Terra", "GPT-5.6 Luna", "GPT-5.5", "GPT-5.5 Mini", "GPT-5.4", "GPT-5.3 Codex", "GPT-5.3 Codex Spark", "GPT-5.2", "GPT-5.1 Codex Max", "GPT-5.1 Codex", "GPT-5.1 Mini", "GPT-5", "GPT-6.1 Sol"]
        .iter().map(|n| hover_core::model::AcpChoice::new(&n.to_lowercase().replace(' ', "-"), n)).collect();
    hover.settings.set_agent_offers(AgentTool::Kiro, &[hover_core::model::AcpOption { id: "model".into(), category: Some("model".into()), current: Some("gpt-6.1-sol".into()), choices: codex.clone() },
        hover_core::model::AcpOption { id: "reasoning_effort".into(), category: Some("thought_level".into()), current: Some("medium".into()),
            choices: ["low", "medium", "high", "xhigh"].iter().map(|e| hover_core::model::AcpChoice::new(e, e)).collect() }]);
    hover.settings.set_agent_options(AgentTool::Kiro, hover_core::model::AgentOptions { model: Some("gpt-6.1-sol".into()), effort: Some("high".into()), ..Default::default() });
    g.invoke_pick_tool(0);
    settle(300);
    g.invoke_open_model(2, 330.0, (full.1 as f32) - 60.0);
    settle(400);
    save(&notch, full, 1.0, desk, &dir.join("office-model-menu-long.png"));
    g.invoke_open_model(0, 0.0, 0.0);
    hover.settings.set_agent_offers(AgentTool::Kiro, &[]);
    hover.settings.set_agent_options(AgentTool::Kiro, Default::default());
    g.invoke_pick_tool(3);
    g.invoke_new_fold();
    let s4 = hover.sessions.start(AgentTool::OpenCode, &folder, "Set up the formatter", vec![]);
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_secs(3) && s4.as_ref().and_then(|s| hover.sessions.get(s.id)).is_none_or(|s| s.kiro_id.is_none()) { std::thread::sleep(Duration::from_millis(10)); }
    if let Some(s4) = &s4 {
        use hover_agents::ask::{AgentAsk, AgentQuestion};
        let sid = hover.sessions.get(s4.id).and_then(|s| s.kiro_id).unwrap_or_default();
        let q = AgentQuestion { header: "Indent".into(), question: "Tabs or spaces?".into(), options: vec![("Tabs".into(), "Indent with tab characters".into()), ("Spaces".into(), String::new())], multiple: false, custom: true };
        let ask = AgentAsk { id: "que_1".into(), kind: "question".into(), title: "Indent".into(), command: None, path: None, preview: None, added: 0, removed: 0,
            reason: "Tabs or spaces?".into(), danger: false, questions: Some(vec![q]) };
        hover.sessions.ask_question(AgentTool::OpenCode, &sid, ask, &hover_agents::cancel::Cancel::new(), Box::new(|_| {}));
    }
    app.office_push();
    settle(2500);
    save(&notch, full, 1.0, desk, &dir.join("office-question-over.png"));
    if let Some(s4) = &s4 { app.open_session(s4.id); }
    settle(1500);
    save(&notch, full, 1.0, desk, &dir.join("office-question-chat.png"));
    if let Some(s4) = &s4 { g.invoke_q_pick(s4.id, "que_1".into(), 0, "Tabs".into()); }
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-question-picked.png"));
    app.close_drawer();
    // Claude Code: picked in the circle, its models with each one's efforts.
    let levels = |l: &[&str]| Some(l.iter().map(|x| x.to_string()).collect());
    hover.settings.set_agent_offers(AgentTool::Claude, &[hover_core::model::AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: vec![
        hover_core::model::AcpChoice { value: "default".into(), name: "Default (recommended)".into(), levels: levels(&["low", "medium", "high", "xhigh", "max"]) },
        hover_core::model::AcpChoice { value: "sonnet".into(), name: "Sonnet".into(), levels: levels(&["low", "medium", "high", "xhigh", "max"]) },
        hover_core::model::AcpChoice { value: "haiku".into(), name: "Haiku".into(), levels: levels(&[]) }] }]);
    hover.settings.set_agent_options(AgentTool::Claude, hover_core::model::AgentOptions { effort: Some("high".into()), ..Default::default() });
    g.invoke_fab_main();
    settle(300);
    g.invoke_pick_tool(4);
    settle(300);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-claude.png"));
    g.invoke_open_model(2, 330.0, (full.1 as f32) - 60.0);
    settle(400);
    save(&notch, full, 1.0, desk, &dir.join("office-model-menu-claude.png"));
    g.invoke_open_model(0, 0.0, 0.0);
    g.invoke_new_fold();
    // The finished chat, its timeline open, the edit's change and the command's output.
    if let Some(id) = first { app.open_session(id); }
    settle(600);
    let turns = app.page_turns();
    if let Some(mut c) = app.page_thread() { c.toggle_steps(&turns, 0); c.toggle_step(&turns, 0, 2, false); c.toggle_step(&turns, 0, 3, false); }
    app.office_widgets();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-drawer-timeline.png"));
    app.close_drawer();
    // Pictures in the chat: one attached to the prompt (kiro-images, as a paste or + keeps
    // it), and one in the answer from the session's folder. A desk is freed for it first.
    if let Some(s4) = &s4 { hover.sessions.stop(s4.id); }
    run_for(400);
    let png = |w: u32, h: u32, f: &dyn Fn(u32, u32) -> [u8; 3]| {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb(f(x, y)));
        let mut b = std::io::Cursor::new(vec![]);
        img.write_to(&mut b, image::ImageFormat::Png).unwrap();
        b.into_inner()
    };
    let mock = png(320, 200, &|x, y| if (40..280).contains(&x) && y > 200 - (x % 80) * 2 { [0x8f, 0x5c, 0xff] } else { [0xf4, 0xf1, 0xea] });
    std::fs::write(data.join("project").join("chart.png"), png(480, 240, &|x, y| if x % 96 > 16 && y > 240 - (x / 96 + 1) * 40 { [0x2f, 0xc9, 0xb0] } else { [0x1a, 0x12, 0x20] })).unwrap();
    let url = format!("data:image/png;base64,{}", hover_agents::http::base64(&mock));
    let pics: Vec<String> = hover_core::images::save(&[hover_core::json::Json::str(url)], &hover_core::images::folder(hover_core::paths::support()))
        .into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    if let Some(s5) = hover.sessions.start(AgentTool::Kiro, &folder, "Restyle the chart like this mock-up", pics) {
        run_for(600);
        app.open_session(s5.id);
        settle(600);
        // Headless there is no event loop for the loader's word to come through: its
        // arrivals are told here once the files have been read.
        run_for(800);
        let urls: Vec<String> = app.page_thread().map(|t| t.sections.iter().flat_map(|s| s.images.clone()).collect()).unwrap_or_default();
        app.image_arrived("");
        for u in &urls { app.image_arrived(u); }
        settle(800);
        save(&notch, full, 1.0, desk, &dir.join("office-chat-images.png"));
        // The top: the prompt with its picture.
        app.notch.global::<Office>().invoke_d_wheel(4000.0);
        settle(400);
        save(&notch, full, 1.0, desk, &dir.join("office-chat-images-prompt.png"));
        // A selection made as the pointer makes it: a triple click on the answer's first
        // paragraph selects it (drawn in the selection colour).
        let at = app.page_thread().and_then(|t| t.sections.iter().find_map(|s| s.answer_at.map(|(ti, _, _)| (s.y, s.frag.texts[ti].y))));
        let scroll = app.page_scroll();
        if let Some((sy, ty)) = at {
            let g = app.notch.global::<Office>();
            let (x, y) = (60.0, sy + ty + 6.0 - scroll);
            for k in 0..3 { g.invoke_d_pointer(0, x, y, false); if k < 2 { g.invoke_d_pointer(2, x, y, false); } }
            g.invoke_d_pointer(2, x, y, false);
            settle(300);
            save(&notch, full, 1.0, desk, &dir.join("office-chat-selection.png"));
            println!("selected: {:?}", app.page_thread().map(|t| t.selected_text()).unwrap_or_default());
        }
        app.close_drawer();
    }
    *hold3.lock().unwrap() = false;
    if !skip("chat") { chat_shots(&app, &hover, dir, &folder, &hold, &hold_c); }
    desk_shots(&app, &hover, dir, &folder, &hold_d);
    app.show_settings_in(0, Section::General);
    save(&notch, full, 1.0, desk, &dir.join("notch-open-settings.png"));
    // A Small office: the nine sections are taller than its sidebar, which scrolls.
    hover.settings.set_workspace_size(hover_core::model::WorkspaceSize::Small);
    view::Host::settings_changed(&*app);
    app.office_follow();
    run_for(800);
    app.show_settings_in(0, Section::Claude);
    run_for(400);
    let small = { let n = app.n.borrow(); (n.win.width() as u32, n.win.height() as u32) };
    save(&notch, small, 1.0, desk, &dir.join("notch-open-settings-small.png"));
    hover.settings.set_workspace_size(hover_core::model::WorkspaceSize::Default);
    view::Host::settings_changed(&*app);
    app.office_follow();
    run_for(800);

    // Settings in the app window, every section, dark and light.
    app.open_dashboard(true);
    if let Some(d) = &*app.dash.borrow() { d.global::<Backdrop>().set_live(false); }
    let dash = adapter(1);
    for (dark, tag) in [(true, "dark"), (false, "light")] {
        hover.settings.set_theme(None);
        hover.settings.set_appearance(if dark { Appearance::Dark } else { Appearance::Light });
        view::Host::theme_changed(&*app);
        for s in Section::ALL {
            app.show_settings_in(1, s);
            let name = format!("settings-{}-{tag}.png", s.title().to_lowercase().replace(' ', "-"));
            save(&dash, (1200, 620), 1.0, [0, 0, 0], &dir.join(name));
        }
    }
    // Narrow, as a small office leaves it: the long lines wrap beside the wide controls.
    for s in Section::ALL {
        app.show_settings_in(1, s);
        save(&dash, (840, 620), 1.0, [0, 0, 0], &dir.join(format!("settings-{}-narrow.png", s.title().to_lowercase().replace(' ', "-"))));
    }
    if !skip("voice") { settings_voice_shots(&app, &hover, dir, &data); }
    settings_integrations_shots(&app, &hover, dir);
    settings_credits_shots(&app, &hover, dir);
    expand_shots(&app, &hover, dir, &folder, &hold_c);
    chat_view_shots(&app, &hover, dir);
    new_task_shots(&app, &hover, dir);
    chat_action_shots(&app, &hover, dir, &folder, &hold_c);
    // A VS Code theme (Dark+ as its files say), and the model picker open.
    let t = hover_core::model::SavedTheme { name: "Dark+".into(), dark: true, colors: [("editor.background", "#1e1e1e"), ("foreground", "#cccccc"),
        ("sideBar.background", "#181818"), ("button.background", "#0e639c"), ("terminal.ansiRed", "#cd3131"), ("terminal.ansiYellow", "#e5e510"),
        ("terminal.ansiGreen", "#0dbc79"), ("terminal.ansiMagenta", "#bc3fbc"), ("terminal.ansiCyan", "#11a8cd")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    hover.settings.set_theme(Some(t));
    view::Host::theme_changed(&*app);
    app.show_settings_in(1, Section::General);
    save(&dash, (1200, 620), 1.0, [0, 0, 0], &dir.join("settings-general-vscode-dark-plus.png"));
    app.show_settings_in(1, Section::Kiro);
    app.pane.borrow_mut().menu = Some(("KiroModel".into(), view::picker_options(&app.last_blocks.borrow(), "KiroModel"), 760.0, 180.0));
    app.refresh_page(false);
    save(&dash, (1200, 620), 1.0, [0, 0, 0], &dir.join("settings-kiro-model-menu.png"));
    app.show_settings_in(1, Section::Codex);
    app.pane.borrow_mut().menu = Some(("CodexModel".into(), view::picker_options(&app.last_blocks.borrow(), "CodexModel"), 760.0, 180.0));
    app.refresh_page(false);
    save(&dash, (1200, 620), 1.0, [0, 0, 0], &dir.join("settings-codex-model-menu.png"));    hover.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}
