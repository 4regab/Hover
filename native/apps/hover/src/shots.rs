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
    let run: RunTask = Arc::new(move |a: RunArgs| {
        (a.progress)(hover_agents::stream::KiroPhase::Reading);
        while *h2.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); }
        KiroResult::new(KiroState::Completed, "## Imports tidied\n\nAll 14 files now sort their imports.")
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

    // Ends nobody saw: the done bot, then the alert. (Headless there is no event loop
    // for the hooks to post to, so what they would do is done here.)
    let said: Arc<std::sync::Mutex<Vec<(String, String)>>> = Default::default();
    let s2 = said.clone();
    hover.on_notify(move |t, b| s2.lock().unwrap().push((t.into(), b.into())));
    *hold.lock().unwrap() = false;
    run_for(400);
    let first = said.lock().unwrap().first().cloned().unwrap_or_default();
    app.announce(&first.0, &first.1);
    save(&notch, crop, 2.0, desk, &dir.join("notch-rest-alert-2x.png"));
    // After the alert's 8 s: the count of ends not yet seen.
    app.alert_clear();
    app.notch.global::<Clock>().set_done_since(0.45);
    save(&notch, crop, 2.0, desk, &dir.join("notch-rest-done-2x.png"));

    // Opening: the greeting's keyframes, then open.
    for (ms, name) in [(120.0, "notch-greeting-120ms.png"), (400.0, "notch-greeting-400ms.png"), (700.0, "notch-greeting-700ms.png")] {
        { let mut n = app.n.borrow_mut(); n.hover.opened(false); n.greet_from = Some(n.now() - ms); }
        app.notch.set_greet(true);
        crate::notch::shape(&app.notch, &app.n.borrow(), view::argb(app.palette.borrow().panel));
        save(&notch, full, 1.0, desk, &dir.join(name));
    }
    { let mut n = app.n.borrow_mut(); n.greet_from = None; n.open = Openness::at(1.0); }
    app.notch.set_greet(false);
    app.hover.seen();
    app.update_rest();
    save(&notch, full, 1.0, desk, &dir.join("notch-open-office.png"));
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
