//! `HOVER_BENCH=1`: a line channel on stdin for tools/hover-measure (the external
//! sampler and scenario runner, tools/hover-measure). Each command takes the
//! path a user's action takes (toggle is the shortcut's, open is the history panel's);
//! answers are `bench <what> <value>` lines on stdout. The measurements themselves are
//! taken from outside; the app only says when something happened.
//!
//! The runner's real-input steps drive the same app with the system's pointer and
//! keyboard; these commands are for setting up a scenario and reading state back.

use slint::ComponentHandle;
use std::io::BufRead;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static FRAMES: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
static WAIT: Mutex<Option<(Instant, &'static str)>> = Mutex::new(None);

pub fn active() -> bool { std::env::var_os("HOVER_BENCH").is_some() }

/// An office frame reached the window: its time, and the end of a timed wait.
pub fn office_frame() {
    if !active() { return; }
    let now = Instant::now();
    let mut f = FRAMES.lock().unwrap();
    f.push(now);
    if f.len() > 4000 { f.drain(..2000); }
    if let Some((t, what)) = WAIT.lock().unwrap().take() { println!("bench {what} {:.1}", (now - t).as_secs_f64() * 1000.0); }
}

/// The notch is on screen (S1's end).
pub fn visible() { if active() { println!("bench visible"); } }

/// The office was dropped after its time hidden (its thread and GPU memory gone).
pub fn dropped() { if active() { println!("bench dropped"); } }

fn pct(v: &mut [f64], p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(f64::total_cmp); v[((v.len() - 1) as f64 * p).round() as usize] }

/// `--features profiling`: every heap allocation counted (live bytes, the peak, how
/// many and how much in all). Not in release builds: the counting costs a few atomics
/// per allocation.
#[cfg(feature = "profiling")]
pub mod heap {
    use std::alloc::{GlobalAlloc, Layout};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub static LIVE: AtomicU64 = AtomicU64::new(0);
    pub static PEAK: AtomicU64 = AtomicU64::new(0);
    pub static COUNT: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);

    pub struct Counting<A>(pub A);

    fn add(n: usize) {
        let live = LIVE.fetch_add(n as u64, Relaxed) + n as u64;
        PEAK.fetch_max(live, Relaxed);
        COUNT.fetch_add(1, Relaxed);
        BYTES.fetch_add(n as u64, Relaxed);
        trace(n);
    }

    /// `heap-trace N`: every allocation of N bytes or more is written to stderr with the
    /// stack that made it (0: off). For finding what makes a peak; symbols need a build
    /// with debug info (docs/development/profiling.md).
    pub static TRACE: AtomicU64 = AtomicU64::new(0);
    std::thread_local! { static IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

    fn trace(n: usize) {
        let min = TRACE.load(Relaxed);
        if min == 0 || (n as u64) < min { return; }
        IN.with(|g| {
            if g.replace(true) { return; }
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            // The functions past the allocator's own, short: the first of Hover's and
            // its crates' frames (std's, core's and alloc's left out).
            let frames: Vec<String> = bt.lines().map(str::trim).filter(|l| !l.starts_with("at ") && l.contains(": "))
                .map(|l| l.split_once(": ").map_or(l, |x| x.1).to_owned())
                .filter(|l| !l.starts_with("std::") && !l.starts_with("core::") && !l.starts_with("alloc::") && !l.contains("bench::heap") && !l.starts_with("hashbrown::"))
                .take(10).collect();
            eprintln!("heap-trace {n} {}", frames.join(" | "));
            g.set(false);
        });
    }

    unsafe impl<A: GlobalAlloc> GlobalAlloc for Counting<A> {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 { let p = unsafe { self.0.alloc(l) }; if !p.is_null() { add(l.size()); } p }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 { let p = unsafe { self.0.alloc_zeroed(l) }; if !p.is_null() { add(l.size()); } p }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { self.0.dealloc(p, l) }; LIVE.fetch_sub(l.size() as u64, Relaxed); }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            let q = unsafe { self.0.realloc(p, l, n) };
            if !q.is_null() { LIVE.fetch_sub(l.size() as u64, Relaxed); add(n); }
            q
        }
    }

    pub fn line() -> String {
        format!("bench heap live {} peak {} allocs {} bytes {}", LIVE.load(Relaxed), PEAK.load(Relaxed), COUNT.load(Relaxed), BYTES.load(Relaxed))
    }
    pub fn reset_peak() { PEAK.store(LIVE.load(Relaxed), Relaxed); }
}

/// Prints `bench <what>` once `done` holds, checked every 100 ms, or `bench <what>-timeout`.
/// Each check is a one-shot timer that schedules the next, so nothing outlives the wait
/// (a repeated timer holding itself was never freed, in the process being measured).
fn until(what: &'static str, secs: f64, done: impl Fn(&std::rc::Rc<crate::App>) -> Option<String> + 'static) {
    type Done = std::rc::Rc<dyn Fn(&std::rc::Rc<crate::App>) -> Option<String>>;
    fn check(what: &'static str, t0: Instant, secs: f64, done: Done) {
        slint::Timer::single_shot(Duration::from_millis(100), move || {
            let Some(a) = crate::APP.with(|a| a.borrow().clone()) else { return };
            if let Some(extra) = done(&a) { println!("bench {what} {extra}"); }
            else if t0.elapsed().as_secs_f64() > secs { println!("bench {what}-timeout"); }
            else { check(what, t0, secs, done); }
        });
    }
    check(what, Instant::now(), secs, std::rc::Rc::new(done));
}

/// `bench gpu allocated A reserved R blocks N sizes S,S,…`, or `bench gpu none` where no
/// device is shared (Linux, the headless shots) or the backend keeps no report.
fn gpu_line(full: bool) -> String {
    let Some((device, _)) = hover_office::render::shared() else { return "bench gpu none".into() };
    let _ = device.poll(slint::wgpu_30::wgpu::PollType::Poll);
    match device.generate_allocator_report() {
        Some(r) => {
            let mut line = format!("bench gpu allocated {} reserved {} blocks {} sizes {}", r.total_allocated_bytes, r.total_reserved_bytes, r.blocks.len(),
                r.blocks.iter().map(|b| b.size.to_string()).collect::<Vec<_>>().join(","));
            // `gpu full`: every live allocation too, by its label, largest first.
            if full {
                let mut by: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
                for a in &r.allocations { let e = by.entry(if a.name.is_empty() { "--".into() } else { a.name.clone() }).or_default(); e.0 += a.size; e.1 += 1; }
                let mut v: Vec<_> = by.into_iter().collect();
                v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
                line += &format!(" allocations {}", v.iter().map(|(n, (s, c))| format!("{}={s}x{c}", n.replace(' ', "_"))).collect::<Vec<_>>().join(";"));
            }
            line
        }
        None => "bench gpu none".into(),
    }
}

fn tool_of(s: &str) -> hover_core::model::AgentTool {
    hover_core::model::AgentTool::ALL.into_iter().find(|t| t.id() == s).unwrap_or(hover_core::model::AgentTool::Kiro)
}

pub fn listen() {
    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { return };
            let parts: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
            let Some(cmd) = parts.first().cloned() else { continue };
            crate::ui_do(move |a| match cmd.as_str() {
                "toggle" => {
                    let opening = a.n.borrow().hover.state == hover_notch::State::Rest;
                    if opening { *WAIT.lock().unwrap() = Some((Instant::now(), "reopen")); }
                    a.toggle();
                    println!("bench toggled {}", if opening { "open" } else { "rest" });
                }
                // The notch to rest, or open, whatever it was (toggle's two halves).
                "fold" => { a.collapse(); println!("bench folded"); }
                "unfold" => {
                    if a.n.borrow().hover.state == hover_notch::State::Rest { *WAIT.lock().unwrap() = Some((Instant::now(), "reopen")); a.expand(false, true); println!("bench unfolded"); }
                    else { println!("bench unfolded already"); }
                }
                // The next office frame, timed from now.
                "frame" => { *WAIT.lock().unwrap() = Some((Instant::now(), "frame")); }
                "history" => { a.open_panel(Some("history")); println!("bench ok"); }
                "open" => {
                    let key = parts.get(1).cloned().unwrap_or_default();
                    *WAIT.lock().unwrap() = Some((Instant::now(), "opened"));
                    match a.hover.sessions.all().into_iter().find(|s| s.key == key) {
                        Some(s) => a.open_session(s.id),
                        None => { let s = a.hover.sessions.saved(&key); println!("bench saved {}", s.is_some()); if let Some(s) = a.hover.sessions.wake(&key) { a.office_changed(); a.open_session(s.id); } }
                    }
                }
                "chat" => { if let Some(id) = parts.get(1).and_then(|v| v.parse().ok()) { a.open_session(id); } else { a.close_drawer(); } println!("bench ok"); }
                // chat-last: the newest session's chat; chat-nth N: the Nth kept one's (from
                // the oldest, wrapping), for scripts that can't know the ids.
                "chat-last" | "chat-nth" => {
                    let all = a.hover.sessions.all_light();
                    let n: usize = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                    let pick = if cmd == "chat-last" { all.last() } else if all.is_empty() { None } else { all.get(n % all.len()) };
                    if let Some(s) = pick { a.open_session(s.id); }
                    println!("bench ok");
                }
                "scroll" => { let dy: f32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0); a.notch.global::<crate::ui::Office>().invoke_d_wheel(dy); println!("bench ok"); }
                "start" => {
                    let folder = parts.get(1).cloned().unwrap_or_default();
                    let n: usize = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(1);
                    let mut ids = vec![];
                    for i in 0..n { if let Some(s) = a.hover.sessions.start(hover_core::model::AgentTool::Kiro, &folder, &format!("bench task {i}"), vec![]) { ids.push(s.id); } }
                    a.office_changed();
                    if let Some(id) = ids.first() { a.open_session(*id); }
                    println!("bench started {}", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
                }
                // task TOOL FOLDER [ACCESS|-] PROMPT…: one task, as the new-task box starts it.
                "task" => {
                    let tool = tool_of(parts.get(1).map_or("kiro", String::as_str));
                    let folder = parts.get(2).cloned().unwrap_or_default();
                    // "@": the folder in Settings (a path with spaces can't be a word here).
                    let folder = if folder == "@" { a.hover.settings.kiro_folder().unwrap_or_default() } else { folder };
                    let access = parts.get(3).filter(|x| *x != "-").cloned();
                    let prompt = parts.get(4..).map(|p| p.join(" ")).unwrap_or_default();
                    let s = a.hover.sessions.start_as(tool, &folder, &prompt, vec![], access.as_deref());
                    a.office_changed();
                    println!("bench task {}", s.map_or("none".into(), |s| s.id.to_string()));
                }
                "reply" => {
                    let id: i32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(-1);
                    let ok = a.hover.sessions.reply(id, &parts.get(2..).map(|p| p.join(" ")).unwrap_or_default(), vec![]);
                    a.office_changed();
                    println!("bench reply {ok}");
                }
                "stop" => { if let Some(id) = parts.get(1).and_then(|v| v.parse().ok()) { a.hover.sessions.stop(id); } println!("bench ok"); }
                "stopall" => { a.hover.sessions.stop_all(); println!("bench ok"); }
                "running" => println!("bench running {}", a.hover.sessions.running()),
                // Each session: id, tool, state, turns, answer bytes, asks waiting.
                "sessions" => {
                    let all = a.hover.sessions.all();
                    let rows: Vec<String> = all.iter().map(|s| format!("{}:{}:{}:{}:{}:{}", s.id, s.tool.id(), s.state.name(), s.turns.len(),
                        s.turns.iter().map(|t| t.result.as_ref().map_or(0, |r| r.text.len())).sum::<usize>(), s.asks.len())).collect();
                    println!("bench sessions {} {}", all.len(), rows.join(" "));
                }
                // said ID: the end of that session's last answer, on one line (a run's check
                // of what the agent was told, e.g. the choice a question got).
                "said" => {
                    let id: i32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(-1);
                    let text = a.hover.sessions.get(id).and_then(|s| s.turns.last().and_then(|t| t.result.as_ref().map(|r| r.text.clone()))).unwrap_or_default();
                    let head: String = text.chars().take(160).collect();
                    // Quotes as ' : a scenario's quoted words can't hold a ".
                    println!("bench said {id} {}", head.replace(['\r', '\n'], " ").replace('"', "'"));
                }
                // drag X0 Y0 X1 Y1 [N]: a press (N clicks, default 1), a move and a release in
                // the open chat's thread, through the pointer's own callbacks; prints what is
                // selected. copy: Ctrl+C's, then what the clipboard holds.
                "drag" => {
                    let v: Vec<f32> = parts[1..].iter().filter_map(|x| x.parse().ok()).collect();
                    if v.len() >= 4 {
                        let g = a.notch.global::<crate::ui::Office>();
                        for _ in 1..(v.get(4).copied().unwrap_or(1.0) as i32).max(1) { g.invoke_d_pointer(0, v[0], v[1], false); g.invoke_d_pointer(2, v[0], v[1], false); }
                        g.invoke_d_pointer(0, v[0], v[1], false);
                        g.invoke_d_pointer(1, v[2], v[3], false);
                        g.invoke_d_pointer(2, v[2], v[3], false);
                    }
                    let t = a.page_thread().map(|t| t.selected_text()).unwrap_or_default();
                    println!("bench selected {}", t.replace(['\r', '\n'], " "));
                }
                "copy" => {
                    let ok = a.notch.global::<crate::ui::Office>().invoke_d_copy();
                    let got = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
                    println!("bench copy {ok} {}", got.replace(['\r', '\n'], " "));
                }
                "until-idle" => {
                    let secs: f64 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(60.0);
                    until("idle", secs, |a| (a.hover.sessions.running() == 0).then(String::new));
                }
                "until-ask" => {
                    let secs: f64 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(60.0);
                    until("asking", secs, |a| a.hover.sessions.all().iter().find_map(|s| s.asking().map(|q| format!("{} {} {}", s.id, q.id, q.kind))));
                }
                // answer ID allow|trust|trustAll|deny: the question in front of that session.
                // answer-front HOW: the one in front of whichever session waits first.
                "answer" | "answer-front" => {
                    let (id, how) = if cmd == "answer-front" {
                        (a.hover.sessions.asking_now().first().map_or(-1, |x| x.0), parts.get(1))
                    } else { (parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(-1), parts.get(2)) };
                    let how = match how.map_or("deny", String::as_str) { "allow" => hover_agents::ask::AskAnswer::Allow, "trust" => hover_agents::ask::AskAnswer::Trust,
                        "trustAll" => hover_agents::ask::AskAnswer::TrustAll, _ => hover_agents::ask::AskAnswer::Deny };
                    let q = a.hover.sessions.get(id).and_then(|s| s.asking().map(|q| q.id.clone()));
                    let ok = q.is_some_and(|q| a.hover.sessions.answer(id, &q, how));
                    a.office_changed();
                    println!("bench answer {ok}");
                }
                // pick ID LABEL…: the question in front of that session, answered with one
                // label (its choice, or one's own words), as a click on a choice does.
                "pick" => {
                    let id: i32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(-1);
                    let label = parts.get(2..).map(|p| p.join(" ")).unwrap_or_default();
                    let q = a.hover.sessions.get(id).and_then(|s| s.asking().filter(|q| q.is_question()).map(|q| (q.id.clone(), q.questions.as_ref().map_or(0, Vec::len))));
                    let ok = q.is_some_and(|(q, n)| a.hover.sessions.answer_question(id, &q, vec![vec![label.clone()]; n]));
                    a.office_changed();
                    println!("bench pick {ok}");
                }
                "settings" => { a.show_settings_in(0, hover_app::pages::Section::ALL[parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0)]); println!("bench ok"); }
                "back" => { a.notch_settings.set(false); a.notch.set_in_settings(false); println!("bench ok"); }
                // "draft new|reply TEXT…": a prompt in the new-task box (opened with the first
                // tool) or in the open chat's reply box, the caret after it.
                "draft" => {
                    let g = a.notch.global::<crate::ui::Office>();
                    let text: slint::SharedString = parts.get(2..).map(|p| p.join(" ")).unwrap_or_default().into();
                    if parts.get(1).is_some_and(|w| w == "new") {
                        if g.get_fab() != 2 { if g.get_fab() == 0 { g.invoke_fab_main(); } g.invoke_pick_tool(0); }
                        g.set_new_draft(text);
                    } else {
                        g.set_d_compose(true);
                        g.set_d_draft(text);
                        g.set_d_draft_to_end(g.get_d_draft_to_end().wrapping_add(1));
                    }
                    a.office_widgets();
                    println!("bench ok");
                }
                // "key [ctrl+]home|end": as the keyboard sends it to the focused box.
                "key" => {
                    use slint::platform::{Key, WindowEvent as E};
                    let k = parts.get(1).map_or("", String::as_str);
                    let ctrl = k.starts_with("ctrl+");
                    let key = if k.ends_with("home") { Key::Home } else { Key::End };
                    let w = a.notch.window();
                    if ctrl { w.dispatch_event(E::KeyPressed { text: Key::Control.into() }); }
                    w.dispatch_event(E::KeyPressed { text: key.into() });
                    w.dispatch_event(E::KeyReleased { text: key.into() });
                    if ctrl { w.dispatch_event(E::KeyReleased { text: Key::Control.into() }); }
                    println!("bench ok");
                }
                "dash" => { a.open_dashboard(false); println!("bench ok"); }
                "dash-close" => { if let Some(d) = a.dash.borrow_mut().take() { let _ = d.hide(); } a.dash_settings.set(false); a.watching_changed(); println!("bench ok"); }
                "dash-min" => { if let Some(d) = &*a.dash.borrow() { crate::hold_gpu(); d.window().set_minimized(true); } println!("bench ok"); }
                "dash-restore" => { if let Some(d) = &*a.dash.borrow() { crate::hold_gpu(); d.window().set_minimized(false); let _ = d.show(); } println!("bench ok"); }
                // quota ID on|off: a notch item, as its switch in Settings does.
                "quota" => {
                    let id = parts.get(1).cloned().unwrap_or_default();
                    let on = parts.get(2).map(String::as_str) == Some("on");
                    a.hover.settings.set_notch_item(&id, on);
                    a.update_rest();
                    a.hover.refresh_quotas(true);
                    println!("bench ok");
                }
                "office" => println!("bench office live {} shown {:?}", a.page.live.borrow().is_some(), a.page.open.get()),
                // until-quota ID SECS: waits for that quota's reading; prints its level and words.
                "until-quota" => {
                    let id = parts.get(1).cloned().unwrap_or_default();
                    let secs: f64 = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(30.0);
                    until("quota", secs, move |a| a.hover.quotas.reading(&id).map(|r| format!("{id} {:?} {}", r.used, r.detail)));
                }
                // size 0..3: Settings → Office size (Small, Default, Large, Extra large).
                "size" => {
                    let i: usize = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(1).min(3);
                    a.hover.settings.set_workspace_size(hover_app::pages::SIZES[i].0);
                    crate::view::Host::settings_changed(&**a);
                    let n = a.n.borrow();
                    println!("bench size {} {}x{}", hover_app::pages::SIZES[i].1.replace(' ', "-"), n.open_size.0, n.open_size.1);
                }
                "state" => {
                    let n = a.n.borrow();
                    println!("bench state {:?} {:.2} view {} kind {} card {} settings {} dash {}", n.hover.state, n.openness(), a.notch.get_view_visible(), a.notch.get_rest_kind(), a.card.get(),
                        a.notch_settings.get(), a.dash.borrow().is_some());
                }
                "frames" => {
                    // Presented-frame intervals over the last N seconds (default 10).
                    let secs: f64 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(10.0);
                    let f = FRAMES.lock().unwrap();
                    let now = Instant::now();
                    let recent: Vec<&Instant> = f.iter().filter(|t| (now - **t).as_secs_f64() <= secs).collect();
                    let mut iv: Vec<f64> = recent.windows(2).map(|w| (*w[1] - *w[0]).as_secs_f64() * 1000.0).collect();
                    let n = recent.len();
                    println!("bench frames {n} {:.1} {:.1} {:.1} {}", pct(&mut iv, 0.5), pct(&mut iv, 0.95), pct(&mut iv, 0.99), crate::FRAMES.load(std::sync::atomic::Ordering::Relaxed));
                }
                #[cfg(feature = "profiling")]
                "heap" => println!("{}", heap::line()),
                #[cfg(feature = "profiling")]
                "heap-reset" => { heap::reset_peak(); println!("bench ok"); }
                #[cfg(feature = "profiling")]
                "heap-trace" => { heap::TRACE.store(parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0), std::sync::atomic::Ordering::Relaxed); println!("bench ok"); }
                // The shared GPU device's own allocator (Windows): what it has handed out
                // and what it holds from the driver, and its blocks (bytes each).
                "gpu" => println!("{}", gpu_line(parts.get(1).is_some_and(|x| x == "full"))),
                // The notch window as its own renderer draws it (the GPU one, unlike
                // --shots), into a PNG: a check of the pixels on a machine with no screen.
                "snap" => {
                    let file = parts.get(1).cloned().unwrap_or_else(|| "notch.png".into());
                    let r = a.notch.window().take_snapshot().map_err(|e| e.to_string()).and_then(|b| {
                        image::RgbaImage::from_raw(b.width(), b.height(), b.as_bytes().to_vec()).ok_or_else(|| "size".to_string())?
                            .save(&file).map_err(|e| e.to_string())
                    });
                    println!("bench snap {}", r.err().unwrap_or_else(|| "ok".into()));
                }
                "quit" => { let _ = slint::quit_event_loop(); }
                _ => println!("bench unknown"),
            });
        }
    });
}
