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

impl Platform for Headless {
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
            ev(KiroStep { exit: Some(0), ms: Some(4100.0), output: Some(out), ..st("x1", "execute", "Run", Some("cargo test --release -p hover-notch"), "completed") });
            ev(KiroStep { added: 2, removed: 1, diff: Some("@@ -118 +118 @@\n  fn open(&mut self) {\n-     self.place();\n+     self.dpi_ready();\n+     self.place();\n  }".into()), ..st("e2", "edit", "Edit", Some("apps/hover/src/notch.rs"), "completed") });
            (a.events)(KiroEvent { credits: Some(0.12), ..Default::default() });
            done("Found it. On a second monitor the notch was placed **before** Windows knew that monitor's scale, so it got resized once on every open. That resize is the blink.\n\n### What changed\n\n- `place()` reads the monitor's DPI first, then sizes the window *once*.\n- The resize message is ignored while the notch opens.\n\n```win.rs\n// Read the scale first: placing then rescaling is the blink.\nfn monitor_dpi(m: HMONITOR) -> u32 {\n    let (mut x, mut y) = (96, 96);\n    unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) };\n    x\n}\n```\n\n| Monitor | Scale | Blinks before | After |\n|---|---|--:|--:|\n| Main | 100% | 0 | 0 |\n| Second | 150% | 1 per open | 0 |\n\n> **Note** · A monitor plugged in while the notch is open still needs one resize.\n\n- [x] Second monitor at 150%\n- [ ] A monitor plugged in while open")
        }
        1 => {
            ev(st("r1", "read", "Read", Some("apps/hover/src/win.rs"), "completed"));
            ev(think("t1", "The user wants it to hold when the taskbar is at the top too. The notch sits at the top centre, so a top taskbar pushes the work area down.\n\nTwo choices. Use `rcWork` from `GetMonitorInfoW` and start the notch under the taskbar. Or keep it at the very top and draw over the taskbar, since the window is topmost anyway.\n\nDrawing over the taskbar hides the clock on some setups. Starting under it is safer, and it matches what NotchOwl does on a Mac with the menu bar.\n\nThere are three monitors to check, and Linux may have the same bug.", None));
            wait();
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
}

/// A voice preview as Voice makes one, for the shots.
fn preview(folder: &str, target: &str, note: Option<&str>, task: &str, countdown: Option<f32>, access: &str) -> hover_app::voice::Preview {
    hover_app::voice::Preview {
        id: 1, heard: "go to hover and fix the notch blink on the second monitor when the taskbar is at the top".into(), cleanup_note: None,
        task: task.into(), folder: folder.into(), target_name: target.into(), note: note.map(Into::into), tool: AgentTool::Codex,
        model: String::new(), access: access.into(), countdown, trial: false,
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
    }
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
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let run: RunTask = Arc::new(move |a: RunArgs| {
        use hover_agents::stream::KiroEvent;
        use hover_core::model::KiroStep;
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (a.events)(KiroEvent { session_id: Some(format!("s{k}")), ..Default::default() });
        (a.progress)(hover_agents::stream::KiroPhase::Reading);
        if let Some(r) = chat_fixture(&a, &hc) { return r; }
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
    voice_shots(&app, &hover, dir, &data);

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
    app.open_panel(None);
    app.notch.global::<Office>().invoke_fab_main();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-pick.png"));
    app.notch.global::<Office>().invoke_pick_tool(0);
    app.notch.global::<Office>().set_new_draft("Add a dark mode to the settings page".into());
    app.office_widgets();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-fab-open.png"));
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
    chat_shots(&app, &hover, dir, &folder, &hold, &hold_c);
    app.show_settings_in(0, Section::General);
    save(&notch, full, 1.0, desk, &dir.join("notch-open-settings.png"));

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
    settings_voice_shots(&app, &hover, dir, &data);
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
    hover.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}
