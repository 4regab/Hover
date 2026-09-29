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
fn save(w: &Rc<MinimalSoftwareWindow>, size: (u32, u32), scale: f32, backdrop: [u8; 3], file: &Path) {
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
    img.save(file).expect("the shot is written");
    println!("{}", file.display());
}

fn run_for(ms: u64) {
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(5));
    }
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
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let run: RunTask = Arc::new(move |a: RunArgs| {
        use hover_agents::stream::KiroEvent;
        use hover_core::model::KiroStep;
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (a.events)(KiroEvent { session_id: Some(format!("s{k}")), ..Default::default() });
        (a.progress)(hover_agents::stream::KiroPhase::Reading);
        // The steps a real turn reports: reads, an edit with its change, a command with its output.
        let step = |id: &str, kind: &str, title: &str, target: &str| KiroStep::new(id, kind, title, Some(target.into()), "completed");
        (a.events)(KiroEvent { step: Some(step("r1", "read", "Read", "src/app/imports.ts")), ..Default::default() });
        (a.events)(KiroEvent { step: Some(step("r2", "read", "Read", "src/app/sort.ts")), ..Default::default() });
        (a.events)(KiroEvent { step: Some(KiroStep { added: 3, removed: 1, ms: Some(1400.0),
            diff: Some("  export function tidy(files) {\n- return files;\n+ return files\n+   .map(sortImports)\n+   .filter(Boolean);".into()), ..step("e1", "edit", "Edit", "src/app/imports.ts") }), ..Default::default() });
        (a.events)(KiroEvent { step: Some(KiroStep { exit: Some(0), ms: Some(8200.0), output: Some("✓ 14 files sorted\nTests: 42 passed, 42 total".into()), ..step("x1", "execute", "Run", "npm test") }), ..Default::default() });
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
    let hover = hover_app::app::Hover::with(settings, None, vec![], Some(run), Some(reader));
    let app = App::new(hover.clone(), Box::new(Plain), Look { dark: true, animations: true }, true);
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
    // The finished chat, its timeline open, the edit's change and the command's output.
    if let Some(id) = first { app.open_session(id); }
    settle(600);
    let turns = app.page_turns();
    if let Some(mut c) = app.page_thread() { c.toggle_steps(&turns, 0); c.toggle_step(&turns, 0, 2, false); c.toggle_step(&turns, 0, 3, false); }
    app.office_widgets();
    settle(600);
    save(&notch, full, 1.0, desk, &dir.join("office-drawer-timeline.png"));
    app.close_drawer();
    *hold3.lock().unwrap() = false;
    app.show_settings_in(0, Section::General);
    save(&notch, full, 1.0, desk, &dir.join("notch-open-settings.png"));

    // Settings in the app window, every section, dark and light.
    app.open_dashboard(true);
    let dash = adapter(1);
    for (dark, tag) in [(true, "dark"), (false, "light")] {
        hover.settings.set_theme(None);
        hover.settings.set_appearance(if dark { Appearance::Dark } else { Appearance::Light });
        view::Host::theme_changed(&*app);
        for s in Section::ALL {
            app.show_settings_in(1, s);
            let name = format!("settings-{}-{tag}.png", s.title().to_lowercase());
            save(&dash, (1200, 620), 1.0, [0, 0, 0], &dir.join(name));
        }
    }
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
