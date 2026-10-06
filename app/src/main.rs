//! Hover (App.xaml.cs): one copy per user, the agent office in the notch at the top
//! centre, the app window, Settings over the office, the tray icon and the shortcut.
//!
//!   hover                 run
//!   hover --version       print the version (Cargo.toml's)
//!   hover --service      the background service: saved tasks, webhooks, watches, with no window
//!   hover --shots DIR     render every view headless (software renderer) into DIR
//!   hover --selftest DIR  run on the real display, drive it, and write report.json (X11)

// A window for the app, not a console, on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

// The Mac app is macos/ (Swift) on the hover-backend crate; this one is Windows and Linux.
#[cfg(target_os = "macos")]
compile_error!("hoverai is the Windows and Linux app; on macOS build macos/ (Swift) with scripts/build-macos.sh, on crates/hover-backend.");

mod icons;
mod bench;
mod notch;
mod desk_ui;
mod office_ui;
mod net;
mod shots;
mod view;
mod voice_ui;
#[cfg(windows)]
mod win;
// The notch on X11, and the self-test that drives it on a real display.
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
mod selftest;

pub mod ui { slint::include_modules!(); }

// See Cargo.toml: freed memory goes back to Windows.
#[cfg(all(windows, not(feature = "profiling")))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Profiling builds count every allocation (bench.rs, `heap`), over the same allocator.
#[cfg(all(windows, feature = "profiling"))]
#[global_allocator]
static ALLOC: bench::heap::Counting<mimalloc::MiMalloc> = bench::heap::Counting(mimalloc::MiMalloc);
#[cfg(all(not(windows), feature = "profiling"))]
#[global_allocator]
static ALLOC: bench::heap::Counting<std::alloc::System> = bench::heap::Counting(std::alloc::System);

use hover_app::app::Hover;
use hover_app::music::Beats;
use hover_app::pages::{self, Section};
use hover_core::palette::Palette;
use hover_core::platform::{Autostart, Look};
use hover_notch::{Action, OfficeSize, State};
use notch::Notch;
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui::*;
use view::{publish, show_page, wire_page, Pane};

/// Takes the shortcut (letting go of the last): false when the system refuses it.
type HotkeyFn = Box<dyn Fn(&hover_core::shortcut::Shortcut) -> bool>;
type NotifyFn = Box<dyn Fn(&str, &str)>;
type MenuFn = Box<dyn Fn(hover_app::rest::Menu)>;

pub struct App {
    pub hover: Arc<Hover>,
    pub notch: NotchWindow,
    pub n: RefCell<Notch>,
    pub dash: RefCell<Option<DashboardWindow>>,
    pub pane: RefCell<Pane>,
    pub last_blocks: RefCell<Vec<pages::Block>>,
    pub palette: RefCell<Palette>,
    pub look: Cell<Look>,
    pub notch_settings: Cell<bool>,
    pub dash_settings: Cell<bool>,
    pub beats: Beats,
    beats_timer: Timer,
    /// The island as last shown: its kind and items (a change cross-fades), what the
    /// words said (a change rises), how many ends were unseen (a new one glows 6 s).
    island: RefCell<(i32, String, String, usize)>,
    /// The question's card is open, and the ask it shows.
    card: Cell<bool>,
    card_ask: RefCell<Option<(i32, String)>>,
    /// NotchHost's 1 s clock: timers, and every 3 s the next agent at work speaks.
    second_timer: Timer,
    ticks: Cell<u32>,
    speaker: Cell<usize>,
    end_glow: Timer,
    anim_timer: Timer,
    clock_timer: Timer,
    clock_last: Cell<Option<Instant>>,
    poll_timer: Timer,
    quota_timer: Timer,
    had_focus: Cell<bool>,
    reported: RefCell<Option<String>>,
    warn: RefCell<Option<WarningWindow>>,
    pub hotkey: RefCell<Option<HotkeyFn>>,
    pub tray_menu: RefCell<Option<MenuFn>>,
    pub notify: RefCell<Option<NotifyFn>>,
    /// Headless: nothing is grabbed, placed or announced outside the process.
    pub headless: bool,
    pub page: office_ui::Page,
    /// Local speech's setup and engine, and the voice flow (voice_ui.rs).
    pub phonon: Arc<hover_app::phonon::Phonon>,
    pub voice: Arc<hover_app::voice::Voice>,
    pub voice_ui: voice_ui::Ui,
}

/// Set by SIGTERM or SIGINT (a logout, a kill, Ctrl+C): the poll quits cleanly, so the
/// tools are shut down and the history and settings flushed, as Quit does.
#[cfg(not(windows))]
pub static QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(not(windows))]
fn on_signals() {
    extern "C" fn on(_: libc::c_int) { QUIT.store(true, std::sync::atomic::Ordering::SeqCst); }
    unsafe {
        libc::signal(libc::SIGTERM, on as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

/// The notch's frames drawn (the self-test's idle check).
pub static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(windows)]
thread_local! {
    static GPU_HOLD: RefCell<Option<std::sync::MutexGuard<'static, ()>>> = const { RefCell::new(None) };
}

/// Keeps the office from submitting GPU work until the event loop comes round again
/// (hover_office::render::gate says why): called before anything that can set a
/// window's surface up again (a resize, a redraw, showing or minimising a window).
pub fn hold_gpu() {
    #[cfg(windows)]
    GPU_HOLD.with(|h| {
        // Only the windows' shared device has the two meet (not the headless shots).
        if h.borrow().is_some() || hover_office::render::shared().is_none() { return; }
        *h.borrow_mut() = Some(hover_office::render::gate());
        Timer::single_shot(Duration::ZERO, || GPU_HOLD.with(|h| { h.borrow_mut().take(); }));
    });
}

thread_local! {
    pub static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs on the UI thread, from any thread (the hooks of hover-app fire off it).
pub fn ui_do(f: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || APP.with(|a| if let Some(a) = a.borrow().clone() { f(&a) }));
}

fn size_of(w: hover_core::model::WorkspaceSize) -> OfficeSize {
    use hover_core::model::WorkspaceSize as W;
    match w { W::Small => OfficeSize::Small, W::Large => OfficeSize::Large, W::ExtraLarge => OfficeSize::ExtraLarge, W::Default => OfficeSize::Default }
}

impl App {
    pub fn new(hover: Arc<Hover>, plat: Box<dyn notch::Plat>, look: Look, headless: bool) -> Rc<App> {
        let notch = NotchWindow::new().expect("the notch window");
        let beats = Beats::new(local_get("beats").as_deref() == Some("on"));
        let (voice_ui, phonon, voice) = voice_ui::make(&hover);
        let app = Rc::new(App {
            n: RefCell::new(Notch::new(plat)),
            notch, dash: RefCell::new(None), pane: RefCell::new(Pane::default()), last_blocks: RefCell::new(vec![]),
            palette: RefCell::new(Palette::hover_dark()), look: Cell::new(look), notch_settings: Cell::new(false), dash_settings: Cell::new(false),
            beats, beats_timer: Timer::default(), island: RefCell::new((-1, String::new(), String::new(), 0)), card: Cell::new(false), card_ask: RefCell::new(None), second_timer: Timer::default(), ticks: Cell::new(0), speaker: Cell::new(0), end_glow: Timer::default(), anim_timer: Timer::default(),
            clock_timer: Timer::default(), clock_last: Cell::new(None), poll_timer: Timer::default(), quota_timer: Timer::default(),
            had_focus: Cell::new(false), reported: RefCell::new(None),
            warn: RefCell::new(None), hotkey: RefCell::new(None), tray_menu: RefCell::new(None), notify: RefCell::new(None),
            headless, hover, page: Default::default(),
            phonon, voice, voice_ui,
        });
        APP.with(|a| *a.borrow_mut() = Some(app.clone()));
        wire_page!(app.notch, app, 0);
        app.wire_office(app.notch.global::<Office>());
        app.desk_wire(app.notch.global::<Desk>());
        app.wire_notch();
        app.wire_voice();
        app.voice_start();
        app.theme_changed_quiet();
        {
            let mut n = app.n.borrow_mut();
            n.size = size_of(app.hover.settings.workspace_size());
            n.hover_opens = app.hover.settings.hover_opens_workspace();
        }
        app.notch.global::<Office>().set_beats(app.beats.want());
        app.update_rest();
        notch::layout(&app.notch, &mut app.n.borrow_mut(), view::argb(app.palette.borrow().panel));
        app.refresh_page(false);

        // Hooks from other threads land on the UI thread.
        app.hover.on_quotas(|| ui_do(|a| { a.update_rest(); if a.pane.borrow().section == Section::Integrations { a.refresh_page(false); } }));
        app.hover.on_sessions(|| ui_do(|a| { a.update_rest(); a.office_changed(); }));
        // The notch shows an ending as its own island (the tool's logo, a badge and the
        // task); the system gets the words.
        app.hover.on_notify(|t, b| { let (t, b) = (t.to_owned(), b.to_owned()); ui_do(move |a| a.announce(&t, &b)); });
        app.start_timers();
        app
    }

    fn wire_notch(self: &Rc<Self>) {
        let a = self.clone();
        self.notch.on_escape(move || a.collapse());
        let a = self.clone();
        self.notch.on_back(move || { a.notch_settings.set(false); a.notch.set_in_settings(false); });
        let a = self.clone();
        self.notch.on_fold(move || a.collapse());
        let a = self.clone();
        // A click inside means the user is working here: stop closing on pointer-leave.
        self.notch.on_shape_pressed(move || { let mut n = a.n.borrow_mut(); if n.hover.state == State::Peek { n.hover.opened(false); } });
        // Notch.cs does this on the shell's PreviewMouseDown: a press anywhere in it,
        // the office included. The office's own elements take the press before the
        // shape's TouchArea sees it, so it is watched before Slint gets it.
        {
            use slint::winit_030::{winit::event::{ElementState, WindowEvent}, EventResult, WinitWindowAccessor};
            let w = Rc::downgrade(self);
            self.notch.window().on_winit_window_event(move |_, e| {
                if matches!(e, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } | WindowEvent::RedrawRequested | WindowEvent::Occluded(_)) { crate::hold_gpu(); }
                if let (WindowEvent::MouseInput { state: ElementState::Pressed, .. }, Some(a)) = (e, w.upgrade()) {
                    if let Ok(mut n) = a.n.try_borrow_mut() { if n.hover.state == State::Peek { n.hover.opened(false); } }
                    // A click in voice's card when the keyboard is elsewhere brings it here.
                    let a2 = a.clone();
                    Timer::single_shot(Duration::ZERO, move || a2.voice_clicked());
                }
                EventResult::Propagate
            });
        }
        let a = self.clone();
        self.notch.on_shape_clicked(move || {
            let (state, kind) = { let n = a.n.borrow(); (n.hover.state, n.rest_kind) };
            // A click on the card does nothing; on the island (outside its buttons) it opens.
            if state == State::Rest && kind == 1 { a.expand(false, false); }
        });
        let a = self.clone();
        self.notch.on_deny(move || a.answer_asked(hover_agents::ask::AskAnswer::Deny));
        let a = self.clone();
        self.notch.on_review(move || a.open_card());
        let a = self.clone();
        self.notch.on_answer(move |how| a.answer_asked(match how.as_str() {
            "allow" => hover_agents::ask::AskAnswer::Allow, "trust" => hover_agents::ask::AskAnswer::Trust,
            "trustAll" => hover_agents::ask::AskAnswer::TrustAll, _ => hover_agents::ask::AskAnswer::Deny,
        }));
    }

    fn start_timers(self: &Rc<Self>) {
        let a = self.clone();
        // Normal priority in the C#, for the same reason as here: the pointer poll must
        // never starve. 50 ms.
        self.poll_timer.start(TimerMode::Repeated, Duration::from_millis(hover_notch::POLL_MS), move || a.poll());
        let a = self.clone();
        self.quota_timer.start(TimerMode::Repeated, hover_quota::schedule::TICK, move || a.hover.refresh_quotas(false));
        self.hover.refresh_quotas(false);
    }

    // MARK: The notch

    pub fn expand(self: &Rc<Self>, peek: bool, focus: bool) {
        let was_rest = self.n.borrow().hover.state == State::Rest;
        notch::expand(&mut self.n.borrow_mut(), peek, focus);
        if focus { self.notch.invoke_focus_view(); }
        if was_rest {
            self.had_focus.set(false);
            // The office shows the question itself.
            if self.card.get() { self.close_card(false); }
            self.notch.set_glow(slint::Color::from_argb_u8(0, 0, 0, 0));
        }
        self.notch.set_view_visible(true);
        self.watching_changed();
        self.animate();
    }

    pub fn collapse(self: &Rc<Self>) {
        notch::collapse(&mut self.n.borrow_mut());
        self.update_rest();
        self.pane.borrow_mut().menu = None;
        self.watching_changed();
        self.animate();
    }

    /// The shortcut and the tray: open the notch, or close it.
    pub fn toggle(self: &Rc<Self>) {
        if self.n.borrow().hover.state == State::Rest { self.expand(false, true) } else { self.collapse() }
    }

    fn animate(self: &Rc<Self>) {
        if self.n.borrow().anim { return; }
        self.n.borrow_mut().anim = true;
        let a = self.clone();
        self.anim_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let panel = view::argb(a.palette.borrow().panel);
            let done = {
                let mut n = a.n.borrow_mut();
                n.still = !a.look.get().animations;
                notch::shape(&a.notch, &n, panel);
                !n.animating()
            };
            if done {
                notch::shape(&a.notch, &a.n.borrow(), panel);
                a.n.borrow_mut().anim = false;
                a.anim_timer.stop();
            }
        });
    }

    fn poll(self: &Rc<Self>) {
        #[cfg(not(windows))]
        if QUIT.load(std::sync::atomic::Ordering::SeqCst) { let _ = slint::quit_event_loop(); return; }
        #[cfg(windows)]
        for m in win::take_messages() { self.on_message(m); }
        let act = notch::poll(&mut self.n.borrow_mut());
        match act {
            Some(Action::Peek) => self.expand(true, false),
            Some(Action::Collapse) => self.collapse(),
            None => {}
        }
        // Click-away: focus went to another app once the notch had it.
        let (state, ours) = { let n = self.n.borrow(); (n.hover.state, n.plat.foreground_is_ours()) };
        if state == State::Open && !self.headless {
            if ours { self.had_focus.set(true); } else if self.had_focus.get() && self.pane.borrow().menu.is_none() { hover_core::log::line("click-away: the keyboard went elsewhere, folding"); self.collapse(); }
        }
        // Displays rarely change; a look every 2 s.
        let relayout = {
            let mut n = self.n.borrow_mut();
            if n.last_display_check.elapsed() > Duration::from_secs(2) {
                n.last_display_check = Instant::now();
                if !self.headless && n.hover.state == State::Rest { n.plat.raise(); }
                n.plat.signature() != n.signature
            } else { false }
        };
        if relayout { notch::layout(&self.notch, &mut self.n.borrow_mut(), view::argb(self.palette.borrow().panel)); }
        // Slint makes the pill's repeated segments when it next lays out, so their width
        // is only known a frame after they change: read it again here.
        let kind = self.notch.get_rest_kind();
        let rest = notch::rest_of(&self.notch, kind);
        if rest != self.n.borrow().rest_target() { self.n.borrow_mut().set_rest(rest); self.animate(); }
        if !self.n.borrow().anim { notch::shape(&self.notch, &self.n.borrow(), view::argb(self.palette.borrow().panel)); }
        // The app window: minimised or not decides whether an office is in view.
        self.watching_changed();
    }

    /// KiroPage.Watching: the open notch, or an app window that isn't minimised. An end
    /// counts as seen only where someone is looking: the open notch, or the app window
    /// with the focus. One behind other windows still draws, but its ends go to the notch.
    fn watching_changed(self: &Rc<Self>) {
        let open = self.n.borrow().hover.state != State::Rest;
        let dash = self.dash.borrow().as_ref().is_some_and(|d| d.window().is_visible() && !minimized(d.window()));
        let on = open || dash;
        let looking = open || (dash && self.dash.borrow().as_ref().is_some_and(|d| focused(d.window())));
        self.hover.set_watching(looking);
        self.office_follow();
        self.beats.follow(on);
        if self.beats.volume() != self.beats_target() { self.fade(); }
    }

    fn beats_target(&self) -> f64 { if self.beats.want() && (self.n.borrow().hover.state != State::Rest || self.dash.borrow().is_some()) { hover_app::music::FULL } else { 0.0 } }

    // MARK: The resting shape

    pub fn update_rest(self: &Rc<Self>) {
        use hover_app::rest::{Kind, Seg};
        use hover_core::model::KiroState;
        let hv = &self.hover;
        let on = hv.settings.notch_items();
        let reading = |id: &str| hv.quotas.reading(id);
        let sessions = hv.sessions.all_light();
        let unseen = hv.unseen_last().map(|u| hover_app::rest::Unseen { count: hv.unseen().0, tool: u.tool, state: u.state, title: u.title, took_secs: u.took_secs });
        let isl = hover_app::rest::island(&on, &reading, &sessions, unseen, self.speaker.get(), hv.sessions.now(), self.card.get());
        // No question left: the card goes, and the keyboard goes back.
        if self.card.get() && !matches!(isl.seg, Seg::Ask { .. }) { self.close_card(true); return; }
        let ui = &self.notch;
        // Voice's card, while an interaction shows, in place of the island.
        let vkind = self.voice_draw();
        let kind = if vkind != 0 { 3 } else { match isl.kind { Kind::None => 0, Kind::Pill => 1, Kind::Card => 2 } };
        let motion = self.look.get().animations;
        let at_rest = self.n.borrow().hover.state == State::Rest;
        let (words, ends) = match &isl.seg {
            Seg::Work { verb, obj, name, .. } => (format!("{name}:{verb}:{obj}"), 0),
            Seg::Done { count, title, .. } => (format!("done:{count}:{title}"), *count),
            Seg::Ask { ask, total, .. } => (format!("ask:{}:{total}", ask.id), 0),
            Seg::None => (String::new(), 0),
        };
        let (old_kind, old_key, old_words, old_ends) = self.island.borrow().clone();
        // The kind or the items changed: the content cross-fades (90 ms held, in by 320).
        if at_rest && motion && old_kind >= 0 && (old_kind != kind || (kind == 1 && old_key != isl.key)) {
            ui.set_fade_ms(0);
            ui.set_fade(0.0);
            let w = ui.as_weak();
            Timer::single_shot(Duration::from_millis(90), move || { if let Some(ui) = w.upgrade() { ui.set_fade_ms(230); ui.set_fade(1.0); } });
        } else if old_words != words && motion && old_kind == kind && old_key == isl.key {
            // Only the words changed: they rise into place (320 ms).
            ui.set_rise_ms(0);
            ui.set_rise(0.0);
            let w = ui.as_weak();
            Timer::single_shot(Duration::from_millis(1), move || { if let Some(ui) = w.upgrade() { ui.set_rise_ms(320); ui.set_rise(1.0); } });
        }
        ui.set_rest_kind(kind);
        ui.set_divider(isl.divider);
        let quotas: Vec<QuotaItem> = isl.quotas.iter().map(|q| QuotaItem {
            id: q.id.as_str().into(), name: q.name.into(), ring: q.ring.map_or(-1.0, |v| v as f32), value: q.value.as_str().into(), pct: q.pct, dim: q.dim,
        }).collect();
        if let Some(m) = view::sync(ui.get_quotas(), &quotas) { ui.set_quotas(m); }
        let clear = slint::Color::from_argb_u8(0, 0, 0, 0);
        let mut glow = clear;
        match &isl.seg {
            Seg::None => ui.set_seg(0),
            Seg::Ask { session, tool, ask, total } => {
                ui.set_seg(1);
                let (verb, obj) = hover_agents::words::ask_line(ask);
                ui.set_ask_tool(tool.id().into());
                ui.set_ask_verb(verb.into());
                ui.set_ask_obj(obj.as_str().into());
                ui.set_ask_mono(ask.command.is_some());
                ui.set_ask_question(ask.is_question());
                ui.set_ask_more(if *total > 1 { format!("+{}", total - 1).into() } else { "".into() });
                glow = slint::Color::from_rgb_u8(0xff, 0xb3, 0x40);
                let s = sessions.iter().find(|x| x.id == *session);
                let folder = s.map(|s| hover_office::office::short(&s.folder)).unwrap_or_default();
                let lines: Vec<PreviewLine> = ask.preview.as_deref().unwrap_or("").lines().map(|l| PreviewLine { text: l.into(),
                    kind: if l.starts_with('+') { 1 } else if l.starts_with('-') { -1 } else { 0 } }).collect();
                let why = format!("{}{}", ask.reason, if ask.added + ask.removed > 0 && ask.kind != "edit" { format!(" Â· +{} âˆ’{}", ask.added, ask.removed) } else { String::new() });
                ui.set_card(CardData {
                    tool: tool.id().into(), title: hover_agents::words::ask_title(ask).into(),
                    sub: format!("{folder} Â· {}", s.map(|s| s.title()).unwrap_or_default()).into(),
                    count: if *total > 1 { format!("1 of {total}").into() } else { "".into() },
                    command: ask.command.clone().unwrap_or_default().into(),
                    path: ask.path.clone().or_else(|| (ask.preview.is_none()).then(|| ask.title.clone())).unwrap_or_default().into(),
                    preview: view::model_of(lines), reason: why.into(), danger: ask.danger, allow: hover_agents::words::ask_allow(ask).into(),
                });
                *self.card_ask.borrow_mut() = Some((*session, ask.id.clone()));
            }
            Seg::Work { tools, active, verb, obj, secs, name, more } => {
                ui.set_seg(2);
                // The stack: oldest first, the speaker drawn last, on top.
                let mut marks: Vec<StackMark> = tools.iter().enumerate().map(|(i, t)| StackMark { tool: t.id().into(), front: if i == *active { 1.0 } else { 0.0 }, i: i as i32 }).collect();
                marks.sort_by(|a, b| a.front.total_cmp(&b.front));
                if let Some(m) = view::sync(ui.get_stack(), &marks) { ui.set_stack(m); }
                ui.set_stack_n(tools.len() as i32);
                ui.set_act_verb((*verb).into());
                ui.set_act_obj(obj.as_str().into());
                ui.set_timer(hover_app::rest::clock(*secs).into());
                ui.set_act_label(format!("{name}: {}{}", [*verb, obj.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" "), if *more > 0 { format!(", and {more} more at work") } else { String::new() }).trim().into());
            }
            Seg::Done { tool, state, title, took_secs, count } => {
                ui.set_seg(3);
                ui.set_done_tool(tool.id().into());
                ui.set_done_badge(match state { KiroState::Completed => 1, KiroState::Failed => 2, _ => 0 });
                ui.set_done_verb(match state { KiroState::Completed => "Done", KiroState::Failed => "Couldnâ€™t finish", _ => "Stopped" }.into());
                ui.set_done_title(if title.is_empty() { "the task".into() } else { title.as_str().into() });
                ui.set_done_took(if *count > 1 { format!("+{}", count - 1).into() } else { hover_app::rest::took(*took_secs).into() });
                // A new end glows 6 s, green done, red failed; a stop doesn't.
                if ends > old_ends {
                    let a = self.clone();
                    self.end_glow.start(TimerMode::SingleShot, Duration::from_secs(6), move || a.update_rest());
                }
                if self.end_glow.running() {
                    glow = match state { KiroState::Completed => slint::Color::from_rgb_u8(0x32, 0xd7, 0x4b), KiroState::Failed => slint::Color::from_rgb_u8(0xff, 0x45, 0x3a), _ => clear };
                }
            }
        }
        if vkind != 0 { glow = if ui.get_voice_busy() { slint::Color::from_rgb_u8(0xff, 0xb3, 0x40) } else { clear }; }
        if at_rest { ui.set_glow(glow); }
        *self.island.borrow_mut() = (kind, isl.key.clone(), words, ends);
        // A question waits, or voice's card is up: hovering doesn't open the office.
        self.n.borrow_mut().asking = matches!(isl.seg, Seg::Ask { .. }) || vkind != 0;
        // The 1 s clock runs while someone works or asks.
        let busy = matches!(isl.seg, Seg::Work { .. } | Seg::Ask { .. });
        if busy && !self.second_timer.running() {
            let a = self.clone();
            self.second_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
                let t = a.ticks.get() + 1;
                a.ticks.set(t);
                if t % 3 == 0 { a.speaker.set(a.speaker.get() + 1); }
                a.update_rest();
            });
        } else if !busy { self.second_timer.stop(); self.ticks.set(0); }
        let rest = notch::rest_of(ui, kind);
        let changed = {
            let mut n = self.n.borrow_mut();
            n.rest_kind = kind;
            n.still = !motion;
            let c = n.rest_target() != rest;
            if c { n.set_rest(rest); }
            c
        };
        notch::shape(ui, &self.n.borrow(), view::argb(self.palette.borrow().panel));
        if changed { self.animate(); }
        // Voice's aura, listening and working, moves on the same clock.
        self.clock(busy || vkind == 1 || vkind == 2);
    }

    /// OpenCard: the question grows into a card that takes the keyboard (Enter allows,
    /// Shift+Enter trusts, Esc denies).
    pub fn open_card(self: &Rc<Self>) {
        if self.n.borrow().hover.state != State::Rest { return; }
        // A question's choices are in the office, in its chat: Review opens it there.
        if let Some((id, ask)) = self.card_ask.borrow().clone() {
            if self.hover.sessions.get(id).and_then(|s| s.asks.iter().find(|a| a.id == ask).map(|a| a.is_question())).unwrap_or(false) {
                self.open_session(id);
                self.expand(false, true);
                return;
            }
        }
        {
            let n = self.n.borrow();
            n.plat.remember_foreground();
            n.plat.set_accepts_keys(true);
            n.plat.focus();
        }
        self.card.set(true);
        self.update_rest();
        self.notch.invoke_focus_card();
    }

    /// The card goes; give_back hands the keyboard back to what had it.
    pub fn close_card(self: &Rc<Self>, give_back: bool) {
        if !self.card.replace(false) { return; }
        if self.n.borrow().hover.state == State::Rest {
            let n = self.n.borrow();
            if give_back { n.plat.restore_foreground(); }
            n.plat.set_accepts_keys(false);
        }
        self.update_rest();
    }

    /// AnswerAsked: the question in front gets its answer; the next one, if any, shows.
    pub fn answer_asked(self: &Rc<Self>, answer: hover_agents::ask::AskAnswer) {
        let Some((id, ask)) = self.card_ask.borrow().clone() else { return };
        hover_core::log::line(&format!("run {id}: {answer:?} from the notch").to_lowercase());
        self.hover.sessions.answer(id, &ask, answer);
        self.update_rest();
    }

    /// Animator: one 30 fps clock for the bots and the dots, only while they show.
    fn clock(self: &Rc<Self>, needed: bool) {
        if !needed || !self.look.get().animations || self.headless {
            self.clock_timer.stop();
            self.clock_last.set(None);
            return;
        }
        if self.clock_timer.running() { return; }
        let a = self.clone();
        self.clock_timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
            let now = Instant::now();
            let dt = a.clock_last.get().map_or(0.0, |l| (now - l).as_secs_f32().min(0.1));
            a.clock_last.set(Some(now));
            // A notch folded open hides its pill; nothing there needs drawing.
            if a.notch.get_mini_opacity() <= 0.0 { return; }
            let c = a.notch.global::<Clock>();
            c.set_t(c.get_t() + dt);
            c.set_done_since(c.get_done_since() + dt);
            a.aura_draw();
        });
    }

    /// An end: the system's notification (the island shows it on its own).
    pub fn announce(self: &Rc<Self>, title: &str, text: &str) {
        self.update_rest();
        if let Some(n) = &*self.notify.borrow() { n(title, text); }
    }


    // MARK: Settings and the app window

    pub fn show_settings_in(self: &Rc<Self>, which: i32, section: Section) {
        self.pane.borrow_mut().section = section;
        if which == 0 { self.notch_settings.set(true); self.notch.set_in_settings(true); }
        else if let Some(d) = &*self.dash.borrow() { self.dash_settings.set(true); d.set_in_settings(true); }
        self.refresh_page(true);
    }

    pub fn open_dashboard(self: &Rc<Self>, settings: bool) {
        if self.dash.borrow().is_none() {
            hover_core::log::line("app window opened");
            let d = DashboardWindow::new().expect("the app window");
            wire_page!(d, self, 1);
            self.wire_office(d.global::<Office>());
            self.desk_wire(d.global::<Desk>());
            let a = self.clone();
            d.on_back(move || { a.dash_settings.set(false); if let Some(d) = &*a.dash.borrow() { d.set_in_settings(false); } });
            // The title bar's own buttons and drag, through winit.
            {
                use slint::winit_030::{winit, WinitWindowAccessor};
                let w = d.as_weak();
                d.on_minimize(move || { if let Some(d) = w.upgrade() { crate::hold_gpu(); d.window().set_minimized(true); } });
                let w = d.as_weak();
                d.on_maximize(move || { if let Some(d) = w.upgrade() { crate::hold_gpu(); let m = !d.window().is_maximized(); d.window().set_maximized(m); d.set_is_maximized(m); } });
                // Maximized by the system too (a double-click or a snap the desktop does):
                // the caption's glyph and the resize border follow the window.
                let w = d.as_weak();
                let a = self.clone();
                d.window().on_winit_window_event(move |_, e| {
                    use winit::event::WindowEvent as E;
                    if matches!(e, E::Resized(_) | E::ScaleFactorChanged { .. } | E::RedrawRequested | E::Occluded(_)) { crate::hold_gpu(); }
                    if let (winit::event::WindowEvent::Resized(_), Some(d)) = (e, w.upgrade()) { d.set_is_maximized(d.window().is_maximized()); }
                    // In view or not follows the focus: an end while it sits behind other
                    // windows goes to the notch.
                    if matches!(e, E::Focused(_)) { let a = a.clone(); Timer::single_shot(Duration::ZERO, move || a.watching_changed()); }
                    slint::winit_030::EventResult::Propagate
                });
                let w = d.as_weak();
                let a = self.clone();
                d.on_close_clicked(move || { if let Some(d) = w.upgrade() { let _ = d.hide(); } let a = a.clone(); Timer::single_shot(Duration::ZERO, move || { a.dash.borrow_mut().take(); a.dash_settings.set(false); a.page.wide.set(false); a.watching_changed(); }); });
                let w = d.as_weak();
                d.on_drag(move || { if let Some(d) = w.upgrade() { d.window().with_winit_window(|ww| { let _ = ww.drag_window(); }); } });
                let w = d.as_weak();
                d.on_resize(move |k| {
                    use winit::window::ResizeDirection as R;
                    let dir = match k { 1 => R::North, 2 => R::South, 3 => R::West, 4 => R::East, 5 => R::NorthWest, 6 => R::NorthEast, 7 => R::SouthWest, _ => R::SouthEast };
                    if let Some(d) = w.upgrade() { d.window().with_winit_window(|ww| { let _ = ww.drag_resize_window(dir); }); }
                });
            }
            let a = self.clone();
            d.window().on_close_requested(move || {
                let a = a.clone();
                Timer::single_shot(Duration::ZERO, move || { a.dash.borrow_mut().take(); a.dash_settings.set(false); a.page.wide.set(false); a.watching_changed(); });
                slint::CloseRequestResponse::HideWindow
            });
            publish!(d, &*self.palette.borrow(), self.look.get().animations);
            d.global::<Office>().set_beats(self.beats.want());
            *self.dash.borrow_mut() = Some(d);
            self.refresh_page(false);
        }
        if settings { self.show_settings_in(1, Section::General); }
        if let Some(d) = &*self.dash.borrow() {
            hold_gpu();
            let _ = d.show();
        }
        self.collapse();
        self.watching_changed();
    }

    pub fn refresh_page(self: &Rc<Self>, top: bool) {
        // The Voice page, opened: its microphones are looked up again.
        if top && self.pane.borrow().section == Section::Voice { self.load_mics(); }
        // The Integrations and the agents' pages, opened: Cua Driver, the sandbox and the tools' setup are looked up.
        if top && matches!(self.pane.borrow().section, Section::Integrations) { self.integ_look(false); }
        let bs = view::build(&**self, &mut self.pane.borrow_mut());
        let p = self.palette.borrow().clone();
        let slint_blocks = view::blocks(&bs, &p);
        *self.last_blocks.borrow_mut() = bs;
        self.n.borrow_mut().popover = self.pane.borrow().menu.is_some();
        show_page!(self.notch, &*self.pane.borrow(), slint_blocks.clone(), &p);
        if let Some(d) = &*self.dash.borrow() { show_page!(d, &*self.pane.borrow(), slint_blocks, &p); }
        let _ = top;
        if let Some(t) = &*self.tray_menu.borrow() { t(self.menu()); }
    }

    pub fn menu(&self) -> hover_app::rest::Menu {
        hover_app::rest::tray_menu(&self.hover.settings.sc_workspace().label(), hover_core::platform::SystemAutostart.enabled())
    }

    /// A tray menu item (Actions.BuildMainMenu's order).
    pub fn menu_item(self: &Rc<Self>, i: usize) {
        match i {
            0 => self.toggle(),
            1 => self.open_dashboard(false),
            3 => { let on = hover_core::platform::SystemAutostart.enabled(); let _ = hover_core::platform::SystemAutostart.set(!on); self.refresh_page(false); }
            5 => self.open_dashboard(true),
            6 => view::Host::quit(&**self),
            _ => {}
        }
    }

    fn theme_changed_quiet(&self) {
        let st = &self.hover.settings;
        let p = hover_core::palette::resolve(st.theme().as_ref(), st.appearance(), || self.look.get().dark);
        *self.palette.borrow_mut() = p.clone();
        let motion = self.look.get().animations;
        publish!(self.notch, &p, motion);
        if let Some(d) = &*self.dash.borrow() {
            publish!(d, &p, motion);
            #[cfg(windows)]
            win::caption(d.window(), p.dark, p.panel);
        }
    }

    pub fn look_changed(self: &Rc<Self>, look: Look) {
        self.look.set(look);
        self.theme_changed_quiet();
        self.refresh_page(false);
        self.update_rest();
    }

    pub fn toggle_beats(self: &Rc<Self>) {
        let want = !self.beats.want();
        let ok = self.beats.toggle(want);
        local_set("beats", if want && ok { "on" } else { "off" });
        let on = self.beats.want();
        self.notch.global::<Office>().set_beats(on);
        if let Some(d) = &*self.dash.borrow() { d.global::<Office>().set_beats(on); }
        self.watching_changed();
        self.fade();
    }

    fn fade(self: &Rc<Self>) {
        if self.beats_timer.running() { return; }
        let a = self.clone();
        self.beats_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || { if !a.beats.frame() { a.beats_timer.stop(); } });
    }

    /// App.RegisterHotKeys: let go of the shortcut and take it again; a refusal is said
    /// once per chord, after the current event.
    pub fn register_hotkeys(self: &Rc<Self>) {
        let sc = self.hover.settings.sc_workspace();
        let ok = self.hotkey.borrow().as_ref().is_none_or(|f| f(&sc));
        if ok { *self.reported.borrow_mut() = None; return; }
        let sig = sc.label();
        if self.reported.borrow().as_deref() == Some(sig.as_str()) { return; }
        *self.reported.borrow_mut() = Some(sig.clone());
        let a = self.clone();
        Timer::single_shot(Duration::ZERO, move || { if a.reported.borrow().as_deref() == Some(sig.as_str()) { a.hotkey_warning(&sig); } });
    }

    fn hotkey_warning(self: &Rc<Self>, label: &str) {
        let who = if cfg!(windows) { "Windows has reserved it" } else { "The desktop has reserved it" };
        let message = format!("Hover couldn't register the notch shortcut, {label}.\n\n{who} or another app is already using it. Choose a different shortcut in Settings â†’ General.");
        hover_core::log::line(&message.replace('\n', " "));
        if self.headless { return; }
        let w = WarningWindow::new().expect("the warning");
        publish!(w, &*self.palette.borrow(), self.look.get().animations);
        w.set_message(message.into());
        let a = self.clone();
        w.on_ok(move || { if let Some(w) = a.warn.borrow_mut().take() { let _ = w.hide(); } });
        hold_gpu();
        let _ = w.show();
        *self.warn.borrow_mut() = Some(w);
    }

    #[cfg(windows)]
    fn on_message(self: &Rc<Self>, m: win::Msg) {
        match m {
            win::Msg::Hotkey => self.toggle(),
            win::Msg::Deactivated => {
                let ours = self.n.borrow().plat.foreground_is_ours();
                if self.n.borrow().hover.state != State::Rest && !ours { self.collapse(); }
            }
            // The greeting on a resume or an unlock is gone (main's 639c01c).
            win::Msg::Greet => {}
            win::Msg::TrayLeft => self.open_dashboard(false),
            win::Msg::TrayMenu(i) => self.menu_item(i),
        }
    }
}

fn minimized(w: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|ww| ww.is_minimized().unwrap_or(false)).unwrap_or(false)
}

fn focused(w: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|ww| ww.has_focus()).unwrap_or(false)
}

impl view::Host for App {
    fn hover(&self) -> &Hover { &self.hover }
    fn system_dark(&self) -> bool { self.look.get().dark }
    fn settings_changed(&self) {
        APP.with(|a| if let Some(a) = a.borrow().clone() {
            let size = size_of(a.hover.settings.workspace_size());
            let relayout = { let mut n = a.n.borrow_mut(); n.hover_opens = a.hover.settings.hover_opens_workspace(); let c = n.size != size; n.size = size; c };
            a.update_rest();
            if relayout { notch::layout(&a.notch, &mut a.n.borrow_mut(), view::argb(a.palette.borrow().panel)); }
        });
    }
    fn theme_changed(&self) { self.theme_changed_quiet(); }
    fn shortcut_changed(&self) { APP.with(|a| if let Some(a) = a.borrow().clone() { a.register_hotkeys(); }); }
    fn quit(&self) { let _ = slint::quit_event_loop(); }
    fn choose_folder(&self) -> Option<String> { pick(true) }
    fn choose_theme_file(&self) -> Option<std::path::PathBuf> { pick(false).map(Into::into) }
    fn refresh(&self) { APP.with(|a| if let Some(a) = a.borrow().clone() { a.refresh_page(false); }); }
    fn recheck(&self, tool: hover_core::model::AgentTool, fresh: bool) {
        std::thread::spawn(move || { hover_agents::agents::check(tool, fresh); ui_do(|a| a.refresh_page(false)); });
    }
    fn action(&self, id: &str) {
        APP.with(|a| if let Some(a) = a.borrow().clone() { if id.starts_with("integ.") { a.integ_action(id); } else { a.voice_action(id); } });
    }
    fn later(&self, work: Box<dyn FnOnce() + Send>) {
        std::thread::Builder::new().name("settings-work".into()).spawn(move || { work(); ui_do(|a| a.refresh_page(false)); }).ok();
    }
}

/// The folder and file pickers: the system's own dialog (IFileDialog on Windows, the
/// portal's FileChooser or zenity/kdialog on Linux).
pub fn pick(folder: bool) -> Option<String> {
    #[cfg(windows)]
    return win::pick(folder);
    #[cfg(target_os = "linux")]
    return x11::pick(folder);
}

/// The image picker for + in the task box and the reply (the page's file input:
/// PNG, JPEG, GIF, WebP).
pub fn pick_image() -> Option<String> {
    #[cfg(windows)]
    return win::pick_image();
    #[cfg(target_os = "linux")]
    return x11::pick_image();
}

// MARK: The page's localStorage (office.beats, office.view)

fn local_file() -> std::path::PathBuf { hover_core::paths::support().join("office.json") }

pub fn local_get(key: &str) -> Option<String> {
    let text = std::fs::read(local_file()).ok()?;
    hover_core::json::parse(&hover_core::json::text_of(&text)).ok()?.get(key)?.as_str().map(str::to_owned)
}

pub fn local_set(key: &str, value: &str) {
    use hover_core::json::Json;
    let mut obj = std::fs::read(local_file()).ok().and_then(|t| hover_core::json::parse(&hover_core::json::text_of(&t)).ok())
        .and_then(|j| if let Json::Obj(o) = j { Some(o) } else { None }).unwrap_or_default();
    obj.retain(|(k, _)| k != key);
    obj.push((key.to_owned(), Json::str(value)));
    let _ = std::fs::write(local_file(), Json::Obj(obj).compact());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    // Before the single-instance check, so it answers while Hover runs. (A Windows
    // GUI exe has no console: print shows from a terminal that pipes it.)
    if args.iter().any(|a| a == "--version") { println!("Hover {}", env!("CARGO_PKG_VERSION")); return; }
    if let Some(dir) = arg("--shots") { shots::run(std::path::Path::new(&dir)); return; }
    // The background service: no window, no single-instance claim (it may run beside the app, which takes the timers from it).
    if args.iter().any(|a| a == "--service") { std::process::exit(hover_app::headless::run()); }

    // One notch is the point; two copies of the app is not. A second launch asks the
    // running copy to open its window, then exits.
    let _instance = match hover_core::single::claim(|_token| ui_do(|a| { hover_core::log::line("another launch: opening the app window"); a.open_dashboard(false); })) {
        Ok(hover_core::single::Claim::First(i)) => i,
        Ok(hover_core::single::Claim::Second) => return,
        Err(e) => { hover_core::log::line(&format!("single instance: {e}")); return; }
    };

    #[cfg(target_os = "linux")]
    {
        // No layer-shell in winit: on Wayland the notch runs through XWayland, where an
        // override-redirect window can sit at the top centre (see the report).
        if std::env::var_os("HOVER_WAYLAND").is_none() { std::env::remove_var("WAYLAND_DISPLAY"); }
    }
    select_backend();
    #[cfg(not(windows))]
    on_signals();
    let hover = Hover::start();
    let look = hover_core::platform::look();
    let selftest = arg("--selftest");
    let app = platform_start(hover.clone(), look, selftest.is_some());
    hover_core::platform::watch_look(|| ui_do(|a| a.look_changed(hover_core::platform::look())));
    hover_core::log::line("started");
    // Kiro Web tasks that were still working in the cloud when Hover closed are followed on.
    hover.sessions.reattach_cut_off();
    if bench::active() { bench::listen(); }
    #[cfg(target_os = "linux")]
    if let Some(dir) = selftest {
        let a = app.clone();
        Timer::single_shot(Duration::from_millis(500), move || selftest::start(a, std::path::PathBuf::from(dir)));
    }
    // The product's self-test drives X11; on Windows the notch's is notch-proto's for now
    // (RUN-ON-WINDOWS, 3C).
    #[cfg(not(target_os = "linux"))]
    if selftest.is_some() { hover_core::log::line("--selftest: only the X11 build has one; run notch-proto --selftest on Windows"); }
    let _ = slint::run_event_loop_until_quit();
    hover_core::log::line("quitting");
    // A recording or a local transcription stops first (and Phonon's helper with it).
    app.voice_quit();
    // Stop the agents before anything is torn down; then the history and the settings.
    app.hover.shutdown();
    app.beats.toggle(false);
    #[cfg(windows)]
    win::tray_stop();
    APP.with(|a| a.borrow_mut().take());
    hover_core::log::line("quit: tools shut down, history and settings flushed");
}

/// One renderer for the process: femtovg on wgpu through DirectComposition on Windows
/// (per-pixel alpha in the notch), femtovg on OpenGL on Linux (an ARGB visual on X11).
fn select_backend() {
    #[cfg(windows)]
    let sel = {
        use slint::wgpu_30::{wgpu, WGPUConfiguration, WGPUSettings};
        let config = match shared_gpu() {
            Ok(c) => c,
            Err(e) => {
                hover_core::log::line(&format!("shared GPU device: {e}; each window makes its own"));
                let mut s = WGPUSettings::default();
                s.backends = wgpu::Backends::DX12;
                s.backend_options.dx12.presentation_system = wgpu::wgt::Dx12SwapchainKind::DxgiFromVisual;
                s.power_preference = wgpu::PowerPreference::LowPower;
                WGPUConfiguration::Automatic(s)
            }
        };
        slint::BackendSelector::new().backend_name("winit".into()).renderer_name("femtovg-wgpu".into()).require_wgpu_30(config)
    };
    #[cfg(target_os = "linux")]
    let sel = slint::BackendSelector::new().backend_name("winit".into())
        .renderer_name(std::env::var("HOVER_RENDERER").unwrap_or_else(|_| "femtovg".into()));
    let sel = sel.with_winit_window_attributes_hook(notch_attributes);
    if let Err(e) = sel.select() {
        hover_core::log::line(&format!("renderer: {e}"));
        // Whatever renderer is left, the notch keeps its window: without the hook it is an
        // ordinary one (activating, opaque, in the taskbar).
        let any = slint::BackendSelector::new().backend_name("winit".into()).with_winit_window_attributes_hook(notch_attributes);
        if let Err(e) = any.select() { hover_core::log::line(&format!("renderer fallback: {e}")); }
    }
}

/// The notch's window, for whichever renderer Slint ends up with.
fn notch_attributes(a: slint::winit_030::winit::window::WindowAttributes) -> slint::winit_030::winit::window::WindowAttributes {
    use slint::winit_030::winit;
    {
        // Only the notch, which is the first window made: the app window and the warning
        // are ordinary ones. (The title isn't set yet when winit asks.)
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if !FIRST.swap(false, std::sync::atomic::Ordering::SeqCst) { return a; }
        let a = a.with_transparent(true).with_decorations(false).with_active(false).with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop).with_position(winit::dpi::PhysicalPosition::new(-32000, -32000));
        #[cfg(windows)]
        let a = {
            use winit::platform::windows::WindowAttributesExtWindows;
            a.with_no_redirection_bitmap(true).with_skip_taskbar(true).with_undecorated_shadow(false).with_class_name("HoverNotch")
        };
        #[cfg(target_os = "linux")]
        let a = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            a.with_override_redirect(true).with_x11_window_type(vec![WindowType::Dock]).with_name("hover", "Hover")
        };
        a
    }
}

/// One GPU device for every window and the office. Left to Slint, each window made its
/// own (the app window added 100 to 200 MB), and the office a further one of its own.
#[cfg(windows)]
fn shared_gpu() -> Result<slint::wgpu_30::WGPUConfiguration, String> {
    use slint::wgpu_30::{wgpu, WGPUConfiguration};
    let instance = wgpu::Instance::new({
        let mut d = wgpu::InstanceDescriptor::new_without_display_handle();
        d.backends = wgpu::Backends::DX12;
        d.backend_options.dx12.presentation_system = wgpu::wgt::Dx12SwapchainKind::DxgiFromVisual;
        d
    });
    // The card that drives the main display, where the notch is. Left to "low power",
    // wgpu picked the integrated GPU on a PC whose screens hang off the discrete one:
    // every frame then crossed between the cards, and the integrated GPU's textures live
    // in system memory, which counts against Hover.
    let primary = win::primary_display_adapter();
    let adapters = futures_lite::future::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
    let adapter = match adapters.into_iter().find(|a| primary.as_deref() == Some(a.get_info().name.as_str())) {
        Some(a) => a,
        None => futures_lite::future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::LowPower, ..Default::default() }))
            .map_err(|e| format!("no GPU adapter: {e}"))?,
    };
    let mut limits = wgpu::Limits::default().using_resolution(adapter.limits());
    // wgpu sizes DX12's descriptor heap from this. The default, a million, is 30 MB per
    // device; Hover's windows and the office bind a few hundred at a time.
    limits.max_non_sampler_bindings = 1 << 16;
    let (device, queue) = futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("hover"), required_limits: limits,
        // Small blocks. The allocator keeps the last block of each kind for good and grows
        // them up to 64 MB (MemoryUsage); anything bigger than a block gets memory of its
        // own, which goes back as soon as it is freed.
        memory_hints: wgpu::MemoryHints::Manual { suballocated_device_memory_block_size: (2 << 20)..(8 << 20) },
        ..Default::default()
    })).map_err(|e| e.to_string())?;
    let info = adapter.get_info();
    hover_core::log::line(&format!("GPU: {} ({:?})", info.name, info.backend));
    // A machine with no GPU (a VM, many RDP hosts) has only WARP. Slint's selector then
    // refuses wgpu unless this is set, and falls back to a plain window: the notch lost
    // its class, its no-activate style and its transparency, and showed in the taskbar.
    // The device is ours and works on WARP, so let Slint use it.
    if info.device_type == wgpu::DeviceType::Cpu && std::env::var_os("SLINT_WGPU_CPU").is_none() {
        std::env::set_var("SLINT_WGPU_CPU", "1");
    }
    hover_office::render::share_device(device.clone(), queue.clone(), format!("{} ({:?})", info.name, info.backend), info.device_type == wgpu::DeviceType::Cpu);
    Ok(WGPUConfiguration::Manual { instance, adapter, device, queue })
}

#[cfg(target_os = "linux")]
fn platform_start(hover: Arc<Hover>, look: Look, _selftest: bool) -> Rc<App> {
    let Some((conn, root, size)) = x11::connect() else {
        hover_core::log::line("no X display: the notch is a plain window");
        return App::new(hover, Box::new(shots::Plain), look, false);
    };
    let scale_cell: Rc<Cell<f64>> = Rc::new(Cell::new(1.0));
    let sc = scale_cell.clone();
    let x = x11::X::new(conn.clone(), root, size, Box::new(move || sc.get()));
    let win_cell = x.win.clone();
    let app = App::new(hover, Box::new(x), look, false);
    let _ = app.notch.window().set_rendering_notifier(|s, _| {
        if matches!(s, slint::RenderingState::AfterRendering) { FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
    });
    let _ = app.notch.show();
    // winit makes its windows once the event loop runs: place the notch from there.
    let a = app.clone();
    let find = Rc::new(Timer::default());
    let f2 = find.clone();
    find.start(TimerMode::Repeated, Duration::from_millis(10), move || {
        let Some(w) = x11::window_of(a.notch.window()) else { return };
        f2.stop();
        scale_cell.set(a.notch.window().scale_factor() as f64);
        win_cell.set(w);
        notch::layout(&a.notch, &mut a.n.borrow_mut(), view::argb(a.palette.borrow().panel));
        a.update_rest();
        bench::visible();
    });
    std::mem::forget(find);
    // The shortcut, rebindable; a refusal is warned about once per chord.
    let grab = x11::Grab::new(conn, root);
    grab.listen(|| ui_do(|a| a.toggle()));
    let g = grab.clone();
    *app.hotkey.borrow_mut() = Some(Box::new(move |sc| { g.clear(); g.register(sc) }));
    app.register_hotkeys();
    // Hold to talk, on the shortcut's event thread: handed to the UI thread.
    let g = grab.clone();
    *app.voice_ui.hold.borrow_mut() = Some(Box::new(move |sc| match sc {
        Some(sc) => g.register_hold(sc, || ui_do(|a| a.voice_press(false)), || ui_do(|a| a.voice_released())),
        None => { g.clear_hold(); Ok(()) }
    }));
    app.register_voice();
    // The tray and its notifications (StatusNotifierItem); none when no tray host runs.
    let icon = hover_app::sni::icon_pixmaps(include_bytes!("../assets/hover.ico"));
    match hover_app::sni::Tray::start(None, icon, app.menu(), |e| ui_do(move |a| match e {
        hover_app::sni::Event::Activate => a.open_dashboard(false),
        hover_app::sni::Event::Item(i) => a.menu_item(i),
    })) {
        Ok(t) => {
            let t = Rc::new(t);
            *app.tray_menu.borrow_mut() = Some(Box::new(move |m| t.set_menu(m)));
        }
        Err(e) => hover_core::log::line(&format!("tray: {e}")),
    }
    *app.notify.borrow_mut() = Some(Box::new(|t, b| {
        let (t, b) = (t.to_owned(), b.to_owned());
        std::thread::spawn(move || { if let Err(e) = hover_app::sni::notify(None, &t, &b) { hover_core::log::line(&format!("notification: {e}")); } });
    }));
    app
}

#[cfg(windows)]
fn platform_start(hover: Arc<Hover>, look: Look, _selftest: bool) -> Rc<App> {
    let app = App::new(hover, Box::new(win::Plat::default()), look, false);
    // The notch's own frames, for the bench's `frames` (as on X11).
    let _ = app.notch.window().set_rendering_notifier(|s, _| {
        if matches!(s, slint::RenderingState::AfterRendering) { FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
    });
    let _ = app.notch.show();
    notch::layout(&app.notch, &mut app.n.borrow_mut(), view::argb(app.palette.borrow().panel));
    *app.hotkey.borrow_mut() = Some(Box::new(|sc| { win::clear_hotkeys(); win::register_hotkey(sc) }));
    // Hold to talk: the press and release come on the UI thread, inside the notch
    // window's message handler, so they are handed on to run after it.
    *app.voice_ui.hold.borrow_mut() = Some(Box::new(|sc| match sc {
        Some(sc) => win::register_hold(sc, || ui_do(|a| a.voice_press(false)), || ui_do(|a| a.voice_released())),
        None => { win::clear_hold(); Ok(()) }
    }));
    win::set_tray_menu(app.menu());
    *app.tray_menu.borrow_mut() = Some(Box::new(win::set_tray_menu));
    *app.notify.borrow_mut() = Some(Box::new(|t, b| win::tray_notify(t, b)));
    // winit makes its windows once the event loop runs, so the notch has no handle yet.
    // Without one it was never placed (it sat off screen), had no tray icon, and the
    // shortcut always failed. The handle is picked up from the loop, as on X11.
    let a = app.clone();
    let find = Rc::new(Timer::default());
    let f2 = find.clone();
    let tries = Cell::new(0u32);
    find.start(TimerMode::Repeated, Duration::from_millis(10), move || {
        tries.set(tries.get() + 1);
        let Some(h) = win::hwnd_of(a.notch.window()) else { return };
        f2.stop();
        hover_core::log::line(&format!("notch window ready (try {})", tries.get()));
        win::disable_transitions(h);
        win::hook(h, || {});
        win::set_notch(h);
        notch::layout(&a.notch, &mut a.n.borrow_mut(), view::argb(a.palette.borrow().panel));
        a.update_rest();
        a.register_hotkeys();
        a.register_voice();
        win::tray_start();
        bench::visible();
    });
    std::mem::forget(find);
    app
}


/// A link in the chat opens in the browser, as KiroPage's `link` did (http(s) only:
/// hover-md makes sure).
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r { hover_core::log::line(&format!("couldn't open {url}: {e}")); }
}
