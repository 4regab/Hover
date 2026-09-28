//! Phase 1B prototype: the office's chat drawer, native.
//!
//!   chat-proto                         a window (Linux dev or Windows)
//!   chat-proto --stream                an answer streams in at 20 chunks per second
//!   chat-proto --turns 200             a long rich conversation
//!   chat-proto --screenshot out.png [--select] [--scale 2] [--hscroll 80]   headless, software renderer
//!                                     (--hscroll: the code blocks scrolled sideways by that much; --top: the thread scrolled to its top)
//!   chat-proto --bench                 headless timings for the report
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use hover_chat::scroll::{Bar, BarId, Part, THICK};
use hover_chat::{state, Hit, Painter, Pos, Shaper, Stage, Tail, Thread, Turn, Unit};
use slint::{ComponentHandle, Model, SharedPixelBuffer, VecModel};

slint::include_modules!();

fn repo() -> PathBuf {
    // Next to the exe in a release (`fonts/`, `fixtures/`), else the source tree.
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    if let Some(d) = exe.filter(|d| d.join("fixtures/rich.md").exists()) {
        return d;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(rel_src: &str, rel_dist: &str) -> Vec<u8> {
    let r = repo();
    std::fs::read(r.join(rel_dist)).or_else(|_| std::fs::read(r.join(rel_src))).unwrap_or_else(|e| panic!("{rel_src}: {e}"))
}

fn fonts() -> Vec<Vec<u8>> {
    vec![read("web/office/fonts/PixelifySans.ttf", "fonts/PixelifySans.ttf")]
}

fn rich() -> String {
    String::from_utf8(read("native/golden/fixtures/rich.md", "fixtures/rich.md")).unwrap()
}

fn fixture() -> serde_json::Value {
    serde_json::from_str(&String::from_utf8(read("native/golden/fixtures/office-state.json", "fixtures/office-state.json")).unwrap()).unwrap()
}

/// A session from the office-state fixture (1: the rich answer), made n turns long by
/// repeating its turns.
fn turns(n: usize, session: usize) -> (Vec<Turn>, &'static str, [u8; 4]) {
    let fx = fixture();
    let s = &fx["state"]["sessions"][session];
    let (name, color) = state::BOTS[s["bot"].as_u64().unwrap_or(1) as usize];
    let base = state::turns(s);
    let mut v: Vec<Turn> = (0..n.max(1)).map(|i| {
        let mut t = base[i % base.len()].clone();
        if i > 0 { t.prompt = format!("Step {}: tighten the refresh-token check.", i + 1); t.status = None; if t.stage == Stage::Working { t.stage = Stage::Done; } }
        t
    }).collect();
    if n > 1 { if let Some(l) = v.last_mut() { *l = base[base.len() - 1].clone(); } }
    let _ = rich;
    (v, name, color)
}

/// renderDrawer's header: the bot, the tool's badge, the title, the folder, the context.
fn header(ui: &ChatWindow, session: usize) {
    let fx = fixture();
    let s = &fx["state"]["sessions"][session];
    let (name, c) = state::BOTS[s["bot"].as_u64().unwrap_or(1) as usize];
    // main.js TOOLS: name and badge colour.
    let (tool, tc) = match s["tool"].as_str().unwrap_or("kiro") { "codex" => ("Codex", 0x3fd6a0), "cursor" => ("Cursor", 0x7cc0ff), _ => ("Kiro", 0xb48cff) };
    let busy = matches!(s["stage"].as_str(), Some("waking" | "working"));
    ui.set_who(name.into());
    ui.set_bot_color(slint::Color::from_rgb_u8(c[0], c[1], c[2]));
    ui.set_tool(tool.into());
    ui.set_tool_color(slint::Color::from_argb_encoded(0xff000000 | tc));
    ui.set_session_title(s["title"].as_str().unwrap_or("").into());
    ui.set_folder(s["folder"].as_str().unwrap_or("").into());
    ui.set_context(s["ctx"].as_f64().unwrap_or(0.0) as f32);
    ui.set_busy(busy);
}

/// Counts clicks as Windows does: another press within the double-click time and
/// rectangle adds one (a third makes a triple click).
struct Clicks { at: Option<Instant>, x: f32, y: f32, n: u32 }

impl Clicks {
    fn press(&mut self, x: f32, y: f32) -> u32 {
        let (time, (w, h)) = double_click();
        let now = Instant::now();
        let near = (x - self.x).abs() <= w / 2.0 && (y - self.y).abs() <= h / 2.0;
        self.n = if near && self.at.is_some_and(|t| now - t <= time) { self.n + 1 } else { 1 };
        (self.at, self.x, self.y) = (Some(now), x, y);
        self.n
    }
}

/// GetDoubleClickTime and SM_CXDOUBLECLK / SM_CYDOUBLECLK (in DIPs, as the pointer is).
#[cfg(windows)]
fn double_click() -> (Duration, (f32, f32)) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK};
    unsafe { (Duration::from_millis(GetDoubleClickTime() as u64), (GetSystemMetrics(SM_CXDOUBLECLK) as f32, GetSystemMetrics(SM_CYDOUBLECLK) as f32)) }
}

/// Windows' defaults, on the Linux dev VM.
#[cfg(not(windows))]
fn double_click() -> (Duration, (f32, f32)) {
    (Duration::from_millis(500), (4.0, 4.0))
}

struct App {
    thread: Thread,
    painter: Painter,
    turns: Vec<Turn>,
    scroll: f32,
    anchor: Option<Pos>,
    /// The word or paragraph a double or triple click selected: a drag grows it by those.
    unit: Unit,
    unit_anchor: (Pos, Pos, Tail),
    clicks: Clicks,
    dragging: bool,
    stick: bool,
    /// The turns changed (a send, a streamed chunk): lay out again.
    dirty: bool,
    /// The viewport the thread was laid out for, and whether its scrollbar shows.
    view: (f32, f32),
    vbar: bool,
    /// A thumb being dragged (and how far into it it was grabbed), or an arrow or the
    /// track held down (where, and since when: it repeats after 250 ms, every 50 ms).
    grab: Option<(BarId, f32)>,
    press: Option<(BarId, Part, f32, Instant)>,
    t0: Instant,
}

impl App {
    fn new((turns, who, color): (Vec<Turn>, &str, [u8; 4])) -> Self {
        let f = fonts();
        // Laid out on the first frame, once the viewport (and so the bar) is known.
        let thread = Thread::new(Shaper::new(&f), who, color);
        let p0 = Pos { section: 0, text: 0, byte: 0 };
        App { thread, painter: Painter::new(&f, Box::new(|_| None)), turns, scroll: 0.0, anchor: None, unit: Unit::Char, unit_anchor: (p0, p0, Tail::None),
            clicks: Clicks { at: None, x: 0.0, y: 0.0, n: 0 }, dragging: false, stick: true, dirty: true, view: (0.0, 0.0), vbar: true, grab: None, press: None, t0: Instant::now() }
    }

    fn relayout(&mut self, width: f32, height: f32) {
        // #thread keeps to the bottom when it was within 40 px of it.
        let was_near = self.thread.height - self.scroll - height < 40.0;
        // overflow: auto: once the thread is taller than its box, the thin scrollbar takes
        // 10 px of its width. It is laid out at the width it had last (a session opens
        // with the bar, as most overflow the notch), and again only when that flips.
        self.thread.set(&self.turns, if self.vbar { width - THICK } else { width });
        let over = self.thread.height > height + 0.5;
        if over != self.vbar {
            self.vbar = over;
            self.thread.set(&self.turns, if over { width - THICK } else { width });
            // Narrower, it may now overflow after all: the bar stays, as in Chromium.
            if !over && self.thread.height > height + 0.5 { self.vbar = true; self.thread.set(&self.turns, width - THICK); }
        }
        (self.view, self.dirty) = ((width, height), false);
        if was_near || self.stick {
            self.scroll = (self.thread.height - height).max(0.0);
            self.stick = false;
        }
    }

    fn frame(&mut self, ui: &ChatWindow) {
        let (w, h) = (ui.get_thread_width(), ui.get_thread_height());
        if w < 1.0 || h < 1.0 {
            return;
        }
        if self.stick || self.dirty || self.view != (w, h) || self.thread.sections.len() != self.turns.len() {
            self.relayout(w, h);
        }
        self.scroll = self.scroll.clamp(0.0, (self.thread.height - h).max(0.0));
        let k = ui.window().scale_factor();
        self.painter.time = self.t0.elapsed().as_secs_f32();
        let mut px = self.painter.paint(&self.thread, self.scroll, (w * k).round() as u32, (h * k).round() as u32, k, hover_chat::theme::DRAWER_BG);
        if let Some(b) = self.bar(BarId::Thread) {
            let hover = self.painter.hover == Some(BarId::Thread);
            self.painter.bar(&mut px, &b, k, hover, [0.0; 4]);
        }
        let mut buf = SharedPixelBuffer::<slint::Rgba8Pixel>::new(px.width(), px.height());
        buf.make_mut_bytes().copy_from_slice(px.data());
        ui.set_thread(slint::Image::from_rgba8_premultiplied(buf));
        let blocks: Vec<A11yBlock> = self.thread.accessible_blocks().into_iter()
            .filter(|(_, r)| r[1] + r[3] > self.scroll && r[1] < self.scroll + h)
            .map(|(text, r)| A11yBlock { x: r[0], y: r[1] - self.scroll, w: r[2], h: r[3], text: text.into() })
            .collect();
        ui.set_blocks(Rc::new(VecModel::from(blocks)).into());
    }

    /// A scrollbar, in viewport coordinates.
    fn bar(&self, id: BarId) -> Option<Bar> {
        let (w, h) = self.view;
        match id {
            BarId::Thread => self.vbar.then(|| Bar { vertical: true, x: w - THICK, y: 0.0, len: h, content: self.thread.height, view: h, pos: self.scroll }),
            BarId::Box(s, k) => self.thread.hbars().find(|(i, _)| *i == (s, k)).map(|(_, b)| Bar { y: b.y - self.scroll, ..b }),
        }
    }

    /// The scrollbar under a point of the viewport, and how far along it the point is.
    fn bar_at(&self, x: f32, y: f32) -> Option<(BarId, Bar, f32)> {
        let boxes = self.thread.hbars().map(|(i, _)| BarId::Box(i.0, i.1)).collect::<Vec<_>>();
        std::iter::once(BarId::Thread).chain(boxes).find_map(|id| {
            let b = self.bar(id)?;
            b.along(x, y).map(|a| (id, b, a))
        })
    }

    fn scroll_to(&mut self, id: BarId, pos: f32) {
        match id {
            BarId::Thread => self.scroll = pos,
            BarId::Box(s, k) => self.thread.scroll_box((s, k), pos),
        }
    }

    /// The pointer on the scrollbars, which come before the text: Some(redraw) when
    /// they took the event. kind: 0 down, 1 move while down, 2 up, 3 move (hover).
    fn bars_pointer(&mut self, kind: i32, x: f32, y: f32) -> Option<bool> {
        if let Some((id, grab)) = self.grab {
            match kind {
                1 => if let Some(b) = self.bar(id) {
                    let along = if b.vertical { y - b.y } else { x - b.x };
                    let pos = b.drag(grab, along);
                    self.scroll_to(id, pos);
                    return Some(true);
                },
                2 => { self.grab = None; return Some(true); }
                _ => {}
            }
        }
        match kind {
            0 => {
                let (id, b, along) = self.bar_at(x, y)?;
                match b.part(along) {
                    Part::Thumb => self.grab = Some((id, along - b.thumb().0)),
                    part => {
                        let pos = b.step(part, along);
                        self.scroll_to(id, pos);
                        self.press = Some((id, part, along, Instant::now()));
                    }
                }
                Some(true)
            }
            // A held track press follows the pointer, and stops when the thumb reaches it.
            1 => { let (id, part, _, t) = self.press?; let b = self.bar(id)?; self.press = Some((id, part, if b.vertical { y - b.y } else { x - b.x }, t)); Some(false) }
            2 => { self.press.take().map(|_| false) }
            3 => {
                let hover = self.bar_at(x, y).filter(|(_, b, a)| b.part(*a) == Part::Thumb).map(|(id, _, _)| id);
                let changed = hover != self.painter.hover;
                self.painter.hover = hover;
                Some(changed)
            }
            _ => None,
        }
    }

    /// A held arrow or track press, one step on.
    fn repeat(&mut self) -> bool {
        let Some((id, part, along, since)) = self.press else { return false };
        if since.elapsed() < Duration::from_millis(250) { return false; }
        let Some(b) = self.bar(id) else { return false };
        let pos = b.step(part, along);
        self.scroll_to(id, pos);
        pos != b.pos
    }
}

fn open_link(url: &str) {
    // The office sends links to the browser through Hover; http(s) only (md.js made sure).
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        eprintln!("couldn't open {url}: {e}");
    }
}

fn wire(ui: &ChatWindow, app: Rc<RefCell<App>>) {
    let redraw = {
        let (app, ui) = (app.clone(), ui.as_weak());
        move || { if let Some(ui) = ui.upgrade() { app.borrow_mut().frame(&ui); } }
    };
    let (r, a, w) = (redraw.clone(), app.clone(), ui.as_weak());
    ui.on_pointer(move |kind, x, y, shift| {
        let bars = a.borrow_mut().bars_pointer(kind, x, y);
        if let Some(ui) = w.upgrade() { ui.set_over_bar(a.borrow().bar_at(x, y).is_some() || a.borrow().grab.is_some()); }
        match bars {
            Some(true) => return r(),
            Some(false) => return,
            None if kind == 3 => return,
            None => {}
        }
        {
            let mut s = a.borrow_mut();
            let yy = y + s.scroll;
            let n = if kind == 0 { s.clicks.press(x, y) } else { 0 };
            match (kind, s.thread.hit(x, yy)) {
                (0, Hit::Link(url)) if !shift => { open_link(&url); }
                (0, Hit::Toggle(i)) => { let turns = s.turns.clone(); s.thread.toggle_steps(&turns, i); }
                (0, Hit::Text(p)) if shift || n == 1 => {
                    if shift { if let Some(an) = s.anchor { s.thread.select(an, p); } } else { s.anchor = Some(p); s.thread.select(p, p); }
                    s.unit = Unit::Char;
                    s.dragging = true;
                }
                (0, Hit::Text(p)) => {
                    // A double click selects the word (and, as WebView2 does on Windows, the
                    // spaces after it); a third, the paragraph.
                    s.unit = if n == 2 { Unit::Word } else { Unit::Para };
                    let (a0, mut a1, tail) = s.thread.unit_at(p, s.unit);
                    if n == 2 { a1 = s.thread.trailing_space(a1); }
                    s.unit_anchor = (a0, a1, tail);
                    s.anchor = Some(a0);
                    let (unit, anchor) = (s.unit, s.unit_anchor);
                    s.thread.select_units(anchor, p, unit);
                    s.dragging = true;
                }
                (0, _) => { s.thread.select(Pos { section: 0, text: 0, byte: 0 }, Pos { section: 0, text: 0, byte: 0 }); s.anchor = None; }
                (1, Hit::Text(p)) if s.dragging && s.unit != Unit::Char => { let (unit, anchor) = (s.unit, s.unit_anchor); s.thread.select_units(anchor, p, unit); }
                (1, Hit::Text(p)) if s.dragging => { if let Some(an) = s.anchor { s.thread.select(an, p); } }
                (2, _) => s.dragging = false,
                _ => return,
            }
        }
        r();
    });
    // The wheel scrolls the thread; sideways (a tilt, a touchpad, or Shift with the wheel,
    // as Chromium on Windows takes it) the code block or table under the pointer.
    let (r, a) = (redraw.clone(), app.clone());
    ui.on_wheel(move |dx, dy, x, y, shift| {
        {
            let mut s = a.borrow_mut();
            let (dx, dy) = if shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
            s.scroll += dy;
            if dx != 0.0 {
                let yy = y + s.scroll;
                if let Some(id) = s.thread.box_at(x, yy) {
                    let pos = s.thread.hscroll.get(&id).copied().unwrap_or(0.0) + dx;
                    s.thread.scroll_box(id, pos);
                }
            }
        }
        r();
    });
    // Held arrows and track presses repeat.
    let (r, a) = (redraw.clone(), app.clone());
    let rep = slint::Timer::default();
    rep.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || { let more = a.borrow_mut().repeat(); if more { r(); } });
    std::mem::forget(rep);
    let a = app.clone();
    ui.on_copy_selection(move || {
        let text = a.borrow().thread.selected_text();
        if text.is_empty() { return; }
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.clone())) {
            Ok(()) => eprintln!("copied {} chars", text.chars().count()),
            Err(e) => eprintln!("clipboard: {e}"),
        }
    });
    let (r, a, w) = (redraw.clone(), app.clone(), ui.as_weak());
    ui.on_send(move |text| {
        {
            let mut s = a.borrow_mut();
            s.turns.push(Turn { stage: Stage::Waking, status: Some("Waking up…".into()), ..Turn::new(text.trim()) });
            s.stick = true;
        }
        if let Some(ui) = w.upgrade() { ui.set_busy(true); }
        r();
    });
    let (r, a, w) = (redraw.clone(), app.clone(), ui.as_weak());
    ui.on_stop(move || {
        a.borrow_mut().dirty = true;
        if let Some(t) = a.borrow_mut().turns.last_mut() {
            t.stage = Stage::Stopped;
            t.status = None;
            t.answer = "Stopped. Nothing after the last step above was changed.".into();
        }
        if let Some(ui) = w.upgrade() { ui.set_busy(false); }
        r();
    });
    let (r, a) = (redraw.clone(), app.clone());
    ui.on_select_all(move || { a.borrow_mut().thread.select_all(); r(); });
    ui.on_close(|| { let _ = slint::quit_event_loop(); });
    // The live step's shimmer runs while it is on screen, at the page's 30 fps.
    let (r, a) = (redraw.clone(), app.clone());
    let shimmer = slint::Timer::default();
    shimmer.start(slint::TimerMode::Repeated, Duration::from_millis(33), move || {
        if a.borrow().thread.sections.iter().any(|s| s.frag.texts.iter().any(|t| t.shimmer)) { r(); }
    });
    std::mem::forget(shimmer);
    // The thread's size is known once laid out; redraw whenever the window changes.
    let r2 = redraw.clone();
    let t = slint::Timer::default();
    let last = Rc::new(RefCell::new((0.0f32, 0.0f32, 0.0f32)));
    let w = ui.as_weak();
    t.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(ui) = w.upgrade() else { return };
        let now = (ui.get_thread_width(), ui.get_thread_height(), ui.window().scale_factor());
        if *last.borrow() != now { *last.borrow_mut() = now; r2(); }
    });
    std::mem::forget(t);
}

fn stream(ui: &ChatWindow, app: Rc<RefCell<App>>) {
    // An answer arrives in chunks as ACP streams it: 20 a second.
    let words: Vec<String> = rich().split_inclusive(' ').map(String::from).collect();
    {
        let mut s = app.borrow_mut();
        s.turns.push(Turn { stage: Stage::Working, status: Some("Writing it up…".into()), ..Turn::new("Say it again, slowly.") });
        s.stick = true;
    }
    ui.set_busy(true);
    let n = Rc::new(RefCell::new(0usize));
    let w = ui.as_weak();
    let t = slint::Timer::default();
    t.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = w.upgrade() else { return };
        let mut k = n.borrow_mut();
        let t0 = Instant::now();
        {
            let mut s = app.borrow_mut();
            s.dirty = true;
            let last = s.turns.last_mut().unwrap();
            if *k < words.len() {
                last.answer.push_str(&words[*k]);
                *k += 1;
            } else if last.stage == Stage::Working {
                last.stage = Stage::Done;
                last.status = None;
                last.took = Some("1 min".into());
                ui.set_busy(false);
            } else {
                return;
            }
        }
        app.borrow_mut().frame(&ui);
        if *k % 20 == 0 { eprintln!("chunk {k}: relayout + paint {:?}", t0.elapsed()); }
    });
    std::mem::forget(t);
}

// ---- headless ------------------------------------------------------------------------

mod headless {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Platform, WindowAdapter};
    use std::rc::Rc;

    pub struct Headless(pub Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    pub fn window() -> Rc<MinimalSoftwareWindow> {
        MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer)
    }
}

fn screenshot(out: &str, select: bool, scale: f32, n: usize, session: usize, hscroll: f32, top: bool) {
    let win = headless::window();
    slint::platform::set_platform(Box::new(headless::Headless(win.clone()))).unwrap();
    let ui = ChatWindow::new().unwrap();
    win.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
    let (w, h) = ((1104.0 * scale) as u32, (424.0 * scale) as u32);
    win.set_size(slint::PhysicalSize::new(w, h));
    ui.show().unwrap();
    header(&ui, session);
    let app = Rc::new(RefCell::new(App::new(turns(n, session))));
    app.borrow_mut().thread.session = fixture()["state"]["sessions"][session]["id"].as_u64().unwrap_or(0);
    let render = |ui: &ChatWindow| -> Vec<slint::Rgb8Pixel> {
        slint::platform::update_timers_and_animations();
        let mut buf = vec![slint::Rgb8Pixel::default(); (w * h) as usize];
        win.request_redraw();
        win.draw_if_needed(|r| { r.render(&mut buf, w as usize); });
        let _ = ui;
        buf
    };
    render(&ui);
    app.borrow_mut().frame(&ui);
    if select {
        let mut s = app.borrow_mut();
        let find = |th: &Thread, needle: &str| {
            th.sections.iter().enumerate().find_map(|(si, sec)| sec.frag.texts.iter().enumerate()
                .find_map(|(ti, t)| t.text.find(needle).map(|b| Pos { section: si, text: ti, byte: b })))
        };
        let (a, mut f) = (find(&s.thread, "export function").unwrap(), find(&s.thread, "That's all").unwrap());
        f.byte += "That's all".len();
        s.thread.select(a, f);
        eprintln!("selected, copies as:\n---\n{}\n---", s.thread.selected_text());
    }
    if top { app.borrow_mut().scroll = 0.0; }
    if hscroll > 0.0 {
        let mut s = app.borrow_mut();
        let ids: Vec<_> = s.thread.hbars().map(|(id, _)| id).collect();
        for id in ids { s.thread.scroll_box(id, hscroll); }
    }
    app.borrow_mut().frame(&ui);
    let buf = render(&ui);
    let img = image::RgbImage::from_raw(w, h, buf.iter().flat_map(|p| [p.r, p.g, p.b]).collect()).unwrap();
    img.save(out).unwrap();
    let names: Vec<String> = ui.get_blocks().iter().take(4).map(|b| b.text.chars().take(40).collect()).collect();
    eprintln!("wrote {out} ({w}x{h}); accessible text nodes in view: {} (first: {names:?})", ui.get_blocks().row_count());
}

fn rss_mb() -> f64 {
    // Linux dev only: resident set from /proc (the Windows numbers come from Measure-Hover.ps1).
    std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split(' ').nth(1).and_then(|v| v.parse::<f64>().ok())).map_or(0.0, |p| p * 4096.0 / 1048576.0)
}

/// Peak resident set so far (Linux VmHWM), in MB.
fn peak_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())).map_or(0.0, |k| k / 1024.0)
}

fn bench() {
    // Headless timings of the engine and the Slint frame for the report.
    let win = headless::window();
    slint::platform::set_platform(Box::new(headless::Headless(win.clone()))).unwrap();
    eprintln!("start: RSS {:.1} MB, peak {:.1} MB", rss_mb(), peak_mb());
    let ui = ChatWindow::new().unwrap();
    win.set_size(slint::PhysicalSize::new(1104, 424));
    ui.show().unwrap();
    eprintln!("window: RSS {:.1} MB, peak {:.1} MB", rss_mb(), peak_mb());
    let mut buf = vec![slint::Rgb8Pixel::default(); 1104 * 424];
    win.draw_if_needed(|r| { r.render(&mut buf, 1104); });
    eprintln!("first Slint frame: RSS {:.1} MB, peak {:.1} MB", rss_mb(), peak_mb());
    let t = Instant::now();
    let app = Rc::new(RefCell::new(App::new(turns(200, 1))));
    app.borrow_mut().frame(&ui);
    eprintln!("200 rich turns: open (layout + first frame) {:?}; RSS {:.1} MB", t.elapsed(), rss_mb());
    let mut worst = Duration::ZERO;
    let t = Instant::now();
    for i in 0..60 {
        let t1 = Instant::now();
        app.borrow_mut().scroll -= 37.0;
        app.borrow_mut().frame(&ui);
        win.request_redraw();
        win.draw_if_needed(|r| { r.render(&mut buf, 1104); });
        worst = worst.max(t1.elapsed());
        let _ = i;
    }
    eprintln!("scroll frame (thread paint + Slint software frame of the window): mean {:?}, worst {worst:?}; RSS {:.1} MB", t.elapsed() / 60, rss_mb());
    let t = Instant::now();
    for i in 0..60 {
        let mut s = app.borrow_mut();
        s.turns.last_mut().unwrap().answer.push_str(&format!(" chunk{i}"));
        s.dirty = true;
        s.frame(&ui);
        if i % 10 == 0 { eprintln!("  chunk {i}: RSS {:.1} MB, sections {}, texts {}", rss_mb(), s.thread.sections.len(), s.thread.sections.last().map_or(0, |x| x.frag.texts.len())); }
    }
    eprintln!("streamed chunk into a 200-turn thread (relayout + paint): mean {:?}; RSS {:.1} MB, peak {:.1} MB", t.elapsed() / 60, rss_mb(), peak_mb());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let n: usize = val("--turns").and_then(|s| s.parse().ok()).unwrap_or(1);
    let session: usize = val("--session").and_then(|s| s.parse().ok()).unwrap_or(1);
    if has("--bench") {
        return bench();
    }
    if let Some(out) = val("--screenshot") {
        let scale = val("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let hs = val("--hscroll").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        return screenshot(&out, has("--select"), scale, n, session, hs, has("--top"));
    }
    let ui = ChatWindow::new().unwrap();
    header(&ui, session);
    let app = Rc::new(RefCell::new(App::new(turns(n, session))));
    app.borrow_mut().thread.session = fixture()["state"]["sessions"][session]["id"].as_u64().unwrap_or(0);
    wire(&ui, app.clone());
    if has("--stream") {
        stream(&ui, app);
    }
    ui.run().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scrollbars under the pointer, as the window drives them (without a window).
    #[test]
    fn scrollbars_take_presses_drags_and_hover() {
        let mut app = App::new(turns(1, 1));
        app.relayout(358.0, 271.0);
        assert!(app.vbar, "the rich session overflows 271 px");
        assert_eq!(app.thread.width, 348.0, "the bar takes 10 px from the thread");
        app.scroll = 0.0;
        // A press on the track under the thumb pages down by 87.5 % of the view.
        let b = app.bar(BarId::Thread).unwrap();
        let (t0, tl) = b.thumb();
        assert_eq!(app.bars_pointer(0, 353.0, t0 + tl + 40.0), Some(true));
        assert!((app.scroll - 271.0 * 0.875).abs() < 0.01, "{}", app.scroll);
        app.bars_pointer(2, 353.0, 200.0);
        // The thumb dragged 20 px down scrolls by 20 px of track.
        let b = app.bar(BarId::Thread).unwrap();
        let (t0, _) = b.thumb();
        app.bars_pointer(0, 353.0, t0 + 2.0);
        app.bars_pointer(1, 353.0, t0 + 22.0);
        assert!((app.bar(BarId::Thread).unwrap().thumb().0 - (t0 + 20.0)).abs() < 0.01);
        app.bars_pointer(2, 353.0, t0 + 22.0);
        // Hovering the thumb darkens it; the text is left to the thread.
        let (t0, _) = app.bar(BarId::Thread).unwrap().thumb();
        assert_eq!(app.bars_pointer(3, 353.0, t0 + 1.0), Some(true));
        assert_eq!(app.painter.hover, Some(BarId::Thread));
        assert_eq!(app.bars_pointer(0, 100.0, 100.0), None);
        // The code block's own bar: its arrow scrolls it 40 px.
        let (id, b) = app.thread.hbars().next().map(|(i, b)| (BarId::Box(i.0, i.1), b)).unwrap();
        app.scroll = (b.y - 100.0).max(0.0);
        let b = app.bar(id).unwrap();
        app.bars_pointer(0, b.x + b.len - 5.0, b.y + 5.0);
        let BarId::Box(s, k) = id else { unreachable!() };
        assert_eq!(app.thread.hscroll.get(&(s, k)).copied(), Some(40.0));
    }
}
