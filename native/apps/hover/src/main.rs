//! Hover (App.xaml.cs): one copy per user, the agent office in the notch at the top
//! centre, the app window, Settings over the office, the tray icon and the shortcut.
//!
//!   hover                 run
//!   hover --shots DIR     render every view headless (software renderer) into DIR
//!   hover --selftest DIR  run on the real display, drive it, and write report.json

// A window for the app, not a console, on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod icons;
mod notch;
mod office_ui;
mod shots;
mod view;
#[cfg(windows)]
mod win;
#[cfg(not(windows))]
mod x11;
#[cfg(not(windows))]
mod selftest;

pub mod ui { slint::include_modules!(); }

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
    alert: RefCell<Option<(String, String)>>,
    alert_timer: Timer,
    anim_timer: Timer,
    clock_timer: Timer,
    clock_last: Cell<Option<Instant>>,
    poll_timer: Timer,
    quota_timer: Timer,
    working: RefCell<Option<String>>,
    done_count: Cell<usize>,
    had_focus: Cell<bool>,
    reported: RefCell<Option<String>>,
    warn: RefCell<Option<WarningWindow>>,
    pub hotkey: RefCell<Option<HotkeyFn>>,
    pub tray_menu: RefCell<Option<MenuFn>>,
    pub notify: RefCell<Option<NotifyFn>>,
    /// Headless: nothing is grabbed, placed or announced outside the process.
    pub headless: bool,
    pub page: office_ui::Page,
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
        let app = Rc::new(App {
            n: RefCell::new(Notch::new(plat)),
            notch, dash: RefCell::new(None), pane: RefCell::new(Pane::default()), last_blocks: RefCell::new(vec![]),
            palette: RefCell::new(Palette::hover_dark()), look: Cell::new(look), notch_settings: Cell::new(false), dash_settings: Cell::new(false),
            beats, beats_timer: Timer::default(), alert: RefCell::new(None), alert_timer: Timer::default(), anim_timer: Timer::default(),
            clock_timer: Timer::default(), clock_last: Cell::new(None), poll_timer: Timer::default(), quota_timer: Timer::default(),
            working: RefCell::new(None), done_count: Cell::new(0), had_focus: Cell::new(false), reported: RefCell::new(None),
            warn: RefCell::new(None), hotkey: RefCell::new(None), tray_menu: RefCell::new(None), notify: RefCell::new(None), headless, hover, page: Default::default(),
        });
        APP.with(|a| *a.borrow_mut() = Some(app.clone()));
        wire_page!(app.notch, app, 0);
        app.wire_office(app.notch.global::<Office>());
        app.wire_notch();
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
        let a = self.clone();
        self.notch.on_shape_clicked(move || {
            let (state, kind) = { let n = a.n.borrow(); (n.hover.state, n.rest_kind) };
            if state == State::Rest && kind != 0 { a.expand(false, false); }
        });
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
            self.notch.set_greet(self.n.borrow().greet_from.is_some());
            self.had_focus.set(false);
        }
        self.notch.set_view_visible(true);
        self.watching_changed();
        self.animate();
    }

    pub fn collapse(self: &Rc<Self>) {
        notch::collapse(&mut self.n.borrow_mut());
        self.notch.set_greet(false);
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
                notch::shape(&a.notch, &n, panel);
                if let Some(t0) = n.greet_from { if n.now() - t0 >= 940.0 { n.greet_from = None; a.notch.set_greet(false); } }
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
            if ours { self.had_focus.set(true); } else if self.had_focus.get() && self.pane.borrow().menu.is_none() { self.collapse(); }
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
        if rest != self.n.borrow().rest { self.n.borrow_mut().rest = rest; }
        if !self.n.borrow().anim { notch::shape(&self.notch, &self.n.borrow(), view::argb(self.palette.borrow().panel)); }
        // The app window: minimised or not decides whether an office is in view.
        self.watching_changed();
    }

    /// KiroPage.Watching: the open notch, or an app window that isn't minimised.
    fn watching_changed(self: &Rc<Self>) {
        let open = self.n.borrow().hover.state != State::Rest;
        let dash = self.dash.borrow().as_ref().is_some_and(|d| d.window().is_visible() && !minimized(d.window()));
        let on = open || dash;
        self.hover.set_watching(on);
        self.office_follow();
        self.beats.follow(on);
        if self.beats.volume() != self.beats_target() { self.fade(); }
    }

    fn beats_target(&self) -> f64 { if self.beats.want() && (self.n.borrow().hover.state != State::Rest || self.dash.borrow().is_some()) { hover_app::music::FULL } else { 0.0 } }

    // MARK: The resting shape

    pub fn update_rest(self: &Rc<Self>) {
        let hv = &self.hover;
        let on = hv.settings.notch_items();
        let reading = |id: &str| hv.quotas.reading(id);
        let r = hover_app::rest::rest(&on, &reading, hv.working_text(), hv.unseen(), self.alert.borrow().clone());
        let ui = &self.notch;
        ui.set_rest_kind(match r.kind { hover_app::rest::Kind::None => 0, hover_app::rest::Kind::Pill => 1, hover_app::rest::Kind::Alert => 2 });
        ui.set_quotas(view::model_of(r.quotas.iter().map(|q| QuotaItem {
            id: q.id.as_str().into(), name: q.name.into(), ring: q.ring.map_or(-1.0, |v| v as f32), value: q.value.as_str().into(), dim: q.dim,
        }).collect()));
        let motion = self.look.get().animations;
        if r.working != *self.working.borrow() {
            if let Some(w) = &r.working { ui.set_working(w.as_str().into()); }
            // The new words rise into place.
            if r.working.is_some() && motion {
                ui.set_rise_ms(0);
                ui.set_rise(0.0);
                let w = ui.as_weak();
                Timer::single_shot(Duration::from_millis(1), move || { if let Some(ui) = w.upgrade() { ui.set_rise_ms(260); ui.set_rise(1.0); } });
            }
            *self.working.borrow_mut() = r.working.clone();
        }
        ui.set_show_working(r.working.is_some());
        ui.set_show_done(r.done.is_some());
        if let Some(d) = &r.done { ui.set_done(d.as_str().into()); }
        if r.done_count != self.done_count.get() {
            if r.done_count > self.done_count.get() { ui.global::<Clock>().set_done_since(0.0); }
            self.done_count.set(r.done_count);
        }
        if let Some((t, x)) = &r.alert { ui.set_alert_title(t.as_str().into()); ui.set_alert_text(x.as_str().into()); }
        let kind = ui.get_rest_kind();
        let rest = notch::rest_of(ui, kind);
        {
            let mut n = self.n.borrow_mut();
            n.rest_kind = kind;
            if n.rest != rest { n.rest = rest; }
        }
        notch::shape(ui, &self.n.borrow(), view::argb(self.palette.borrow().panel));
        self.clock(r.working.is_some() || r.done.is_some());
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
        });
    }

    /// A few seconds of message in the notch, so an end is seen even when the system
    /// holds notifications back; and the tray's notification.
    pub fn announce(self: &Rc<Self>, title: &str, text: &str) {
        *self.alert.borrow_mut() = Some((title.to_owned(), text.to_owned()));
        self.update_rest();
        let a = self.clone();
        self.alert_timer.start(TimerMode::SingleShot, Duration::from_secs(8), move || a.alert_clear());
        if let Some(n) = &*self.notify.borrow() { n(title, text); }
    }

    /// The alert's 8 s are up.
    pub fn alert_clear(self: &Rc<Self>) { self.alert_timer.stop(); *self.alert.borrow_mut() = None; self.update_rest(); }

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
            let a = self.clone();
            d.on_back(move || { a.dash_settings.set(false); if let Some(d) = &*a.dash.borrow() { d.set_in_settings(false); } });
            let a = self.clone();
            d.window().on_close_requested(move || {
                let a = a.clone();
                Timer::single_shot(Duration::ZERO, move || { a.dash.borrow_mut().take(); a.dash_settings.set(false); a.watching_changed(); });
                slint::CloseRequestResponse::HideWindow
            });
            publish!(d, &*self.palette.borrow(), self.look.get().animations);
            d.global::<Office>().set_beats(self.beats.want());
            *self.dash.borrow_mut() = Some(d);
            self.refresh_page(false);
        }
        if settings { self.show_settings_in(1, Section::General); }
        if let Some(d) = &*self.dash.borrow() {
            let _ = d.show();
            #[cfg(windows)]
            { let p = self.palette.borrow(); win::caption(d.window(), p.dark, p.panel); }
        }
        self.collapse();
        self.watching_changed();
    }

    pub fn refresh_page(self: &Rc<Self>, top: bool) {
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
        let message = format!("Hover couldn't register the notch shortcut, {label}.\n\n{who} or another app is already using it. Choose a different shortcut in Settings → General.");
        hover_core::log::line(&message.replace('\n', " "));
        if self.headless { return; }
        let w = WarningWindow::new().expect("the warning");
        publish!(w, &*self.palette.borrow(), self.look.get().animations);
        w.set_message(message.into());
        let a = self.clone();
        w.on_ok(move || { if let Some(w) = a.warn.borrow_mut().take() { let _ = w.hide(); } });
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
            win::Msg::Greet => self.n.borrow_mut().greet_next = true,
            win::Msg::TrayLeft => self.open_dashboard(false),
            win::Msg::TrayMenu(i) => self.menu_item(i),
        }
    }
}

fn minimized(w: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|ww| ww.is_minimized().unwrap_or(false)).unwrap_or(false)
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
}

/// The folder and file pickers: the system's own dialog (IFileDialog on Windows, the
/// portal's FileChooser or zenity/kdialog on Linux).
pub fn pick(folder: bool) -> Option<String> {
    #[cfg(windows)]
    return win::pick(folder);
    #[cfg(not(windows))]
    return x11::pick(folder);
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
    if let Some(dir) = arg("--shots") { shots::run(std::path::Path::new(&dir)); return; }

    // One notch is the point; two copies of the app is not. A second launch asks the
    // running copy to open its window, then exits.
    let _instance = match hover_core::single::claim(|_token| ui_do(|a| { hover_core::log::line("another launch: opening the app window"); a.open_dashboard(false); })) {
        Ok(hover_core::single::Claim::First(i)) => i,
        Ok(hover_core::single::Claim::Second) => return,
        Err(e) => { hover_core::log::line(&format!("single instance: {e}")); return; }
    };

    #[cfg(not(windows))]
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
    #[cfg(not(windows))]
    if let Some(dir) = selftest {
        let a = app.clone();
        Timer::single_shot(Duration::from_millis(500), move || selftest::start(a, std::path::PathBuf::from(dir)));
    }
    let _ = slint::run_event_loop_until_quit();
    hover_core::log::line("quitting");
    // Stop the agents before anything is torn down; then the history and the settings.
    app.hover.shutdown();
    app.beats.toggle(false);
    #[cfg(windows)]
    win::tray_stop();
    APP.with(|a| a.borrow_mut().take());
    hover_core::log::line("quit: tools shut down, history and settings flushed");
}

/// One renderer for the process: femtovg on wgpu through DirectComposition on Windows
/// (per-pixel alpha in the notch), femtovg on OpenGL elsewhere (an ARGB visual on X11).
fn select_backend() {
    use slint::winit_030::winit;
    #[cfg(windows)]
    let sel = {
        use slint::wgpu_30::{wgpu, WGPUConfiguration, WGPUSettings};
        let mut s = WGPUSettings::default();
        s.backends = wgpu::Backends::DX12;
        s.backend_options.dx12.presentation_system = wgpu::wgt::Dx12SwapchainKind::DxgiFromVisual;
        s.power_preference = wgpu::PowerPreference::LowPower;
        slint::BackendSelector::new().backend_name("winit".into()).renderer_name("femtovg-wgpu".into()).require_wgpu_30(WGPUConfiguration::Automatic(s))
    };
    #[cfg(not(windows))]
    let sel = slint::BackendSelector::new().backend_name("winit".into())
        .renderer_name(std::env::var("HOVER_RENDERER").unwrap_or_else(|_| "femtovg".into()));
    let sel = sel.with_winit_window_attributes_hook(|a: winit::window::WindowAttributes| {
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
        #[cfg(not(windows))]
        let a = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            a.with_override_redirect(true).with_x11_window_type(vec![WindowType::Dock]).with_name("hover", "Hover")
        };
        a
    });
    if let Err(e) = sel.select() { hover_core::log::line(&format!("renderer: {e}")); }
}

#[cfg(not(windows))]
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
    });
    std::mem::forget(find);
    // The shortcut, rebindable; a refusal is warned about once per chord.
    let grab = x11::Grab::new(conn, root);
    grab.listen(|| ui_do(|a| a.toggle()));
    let g = grab.clone();
    *app.hotkey.borrow_mut() = Some(Box::new(move |sc| { g.clear(); g.register(sc) }));
    app.register_hotkeys();
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
    let _ = app.notch.show();
    if let Some(h) = win::hwnd_of(app.notch.window()) {
        win::disable_transitions(h);
        win::hook(h, || {});
        win::set_notch(h);
    }
    notch::layout(&app.notch, &mut app.n.borrow_mut(), view::argb(app.palette.borrow().panel));
    *app.hotkey.borrow_mut() = Some(Box::new(|sc| { win::clear_hotkeys(); win::register_hotkey(sc) }));
    app.register_hotkeys();
    win::set_tray_menu(app.menu());
    win::tray_start();
    *app.tray_menu.borrow_mut() = Some(Box::new(win::set_tray_menu));
    *app.notify.borrow_mut() = Some(Box::new(|t, b| win::tray_notify(t, b)));
    app
}
