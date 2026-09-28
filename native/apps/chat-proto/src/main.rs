//! Phase 1B prototype: the office's chat drawer, native.
//!
//!   chat-proto                         a window (Linux dev or Windows)
//!   chat-proto --stream                an answer streams in at 20 chunks per second
//!   chat-proto --turns 200             a long rich conversation
//!   chat-proto --screenshot out.png [--select] [--scale 2]   headless, software renderer
//!   chat-proto --bench                 headless timings for the report
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use hover_chat::{Hit, Painter, Pos, Shaper, Stage, Thread, Turn};
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

fn turns(n: usize) -> Vec<Turn> {
    (0..n).map(|i| Turn {
        prompt: if i == 0 { "The notch blinks when I change the workspace size in Settings. Find out why and fix it.".into() } else { format!("Step {}: tighten the refresh-token check.", i + 1) },
        queued: false,
        steps: 3,
        took: Some("3 min".into()),
        stage: Stage::Done,
        status: None,
        answer: rich(),
    }).collect()
}

struct App {
    thread: Thread,
    painter: Painter,
    turns: Vec<Turn>,
    scroll: f32,
    anchor: Option<Pos>,
    dragging: bool,
    stick: bool,
}

impl App {
    fn new(turns: Vec<Turn>) -> Self {
        let f = fonts();
        let mut thread = Thread::new(Shaper::new(&f), "Juno", [0x2f, 0xc9, 0xb0, 255]);
        thread.set(&turns, 360.0);
        App { thread, painter: Painter::new(&f, Box::new(|_| None)), turns, scroll: 0.0, anchor: None, dragging: false, stick: true }
    }

    fn relayout(&mut self, width: f32, height: f32) {
        // #thread keeps to the bottom when it was within 40 px of it.
        let was_near = self.thread.height - self.scroll - height < 40.0;
        self.thread.set(&self.turns, width);
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
        if self.stick || (w - self.thread.width).abs() > 0.5 || self.thread.sections.len() != self.turns.len() {
            self.relayout(w, h);
        }
        self.scroll = self.scroll.clamp(0.0, (self.thread.height - h).max(0.0));
        let k = ui.window().scale_factor();
        let px = self.painter.paint(&self.thread, self.scroll, (w * k).round() as u32, (h * k).round() as u32, k, hover_chat::theme::DRAWER_BG);
        let mut buf = SharedPixelBuffer::<slint::Rgba8Pixel>::new(px.width(), px.height());
        buf.make_mut_bytes().copy_from_slice(px.data());
        ui.set_thread(slint::Image::from_rgba8_premultiplied(buf));
        let blocks: Vec<A11yBlock> = self.thread.accessible_blocks().into_iter()
            .filter(|(_, r)| r[1] + r[3] > self.scroll && r[1] < self.scroll + h)
            .map(|(text, r)| A11yBlock { x: r[0], y: r[1] - self.scroll, w: r[2], h: r[3], text: text.into() })
            .collect();
        ui.set_blocks(Rc::new(VecModel::from(blocks)).into());
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
    let r = redraw.clone();
    let a = app.clone();
    ui.on_pointer(move |kind, x, y, shift| {
        {
            let mut s = a.borrow_mut();
            let yy = y + s.scroll;
            match (kind, s.thread.hit(x, yy)) {
                (0, Hit::Link(url)) if !shift => { open_link(&url); }
                (0, Hit::Text(p)) => {
                    if shift { if let Some(an) = s.anchor { s.thread.select(an, p); } } else { s.anchor = Some(p); s.thread.select(p, p); }
                    s.dragging = true;
                }
                (0, _) => { s.thread.select(Pos { section: 0, text: 0, byte: 0 }, Pos { section: 0, text: 0, byte: 0 }); s.anchor = None; }
                (1, Hit::Text(p)) if s.dragging => { if let Some(an) = s.anchor { s.thread.select(an, p); } }
                (2, _) => s.dragging = false,
                _ => return,
            }
        }
        r();
    });
    let (r, a) = (redraw.clone(), app.clone());
    ui.on_wheel(move |dy| { a.borrow_mut().scroll += dy; r(); });
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
            s.turns.push(Turn { prompt: text.trim().to_string(), queued: false, steps: 0, took: None, stage: Stage::Waking, status: Some("Waking up…".into()), answer: String::new() });
            s.stick = true;
        }
        if let Some(ui) = w.upgrade() { ui.set_busy(true); }
        r();
    });
    let (r, a, w) = (redraw.clone(), app.clone(), ui.as_weak());
    ui.on_stop(move || {
        if let Some(t) = a.borrow_mut().turns.last_mut() {
            t.stage = Stage::Stopped;
            t.status = None;
            t.answer = "Stopped. Nothing after the last step above was changed.".into();
        }
        if let Some(ui) = w.upgrade() { ui.set_busy(false); }
        r();
    });
    ui.on_close(|| { let _ = slint::quit_event_loop(); });
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
        s.turns.push(Turn { prompt: "Say it again, slowly.".into(), queued: false, steps: 0, took: None, stage: Stage::Working, status: Some("Writing it up…".into()), answer: String::new() });
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

fn screenshot(out: &str, select: bool, scale: f32, n: usize) {
    let win = headless::window();
    slint::platform::set_platform(Box::new(headless::Headless(win.clone()))).unwrap();
    let ui = ChatWindow::new().unwrap();
    win.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
    let (w, h) = ((1104.0 * scale) as u32, (424.0 * scale) as u32);
    win.set_size(slint::PhysicalSize::new(w, h));
    ui.show().unwrap();
    let app = Rc::new(RefCell::new(App::new(turns(n))));
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

fn bench() {
    // Headless timings of the engine and the Slint frame for the report.
    let win = headless::window();
    slint::platform::set_platform(Box::new(headless::Headless(win.clone()))).unwrap();
    let ui = ChatWindow::new().unwrap();
    win.set_size(slint::PhysicalSize::new(1104, 424));
    ui.show().unwrap();
    let mut buf = vec![slint::Rgb8Pixel::default(); 1104 * 424];
    win.draw_if_needed(|r| { r.render(&mut buf, 1104); });
    let t = Instant::now();
    let app = Rc::new(RefCell::new(App::new(turns(200))));
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
        let turns = s.turns.clone();
        s.thread.set(&turns, 360.0);
        s.frame(&ui);
        if i % 10 == 0 { eprintln!("  chunk {i}: RSS {:.1} MB, sections {}, texts {}", rss_mb(), s.thread.sections.len(), s.thread.sections.last().map_or(0, |x| x.frag.texts.len())); }
    }
    eprintln!("streamed chunk into a 200-turn thread (relayout + paint): mean {:?}; RSS {:.1} MB", t.elapsed() / 60, rss_mb());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let n: usize = val("--turns").and_then(|s| s.parse().ok()).unwrap_or(1);
    if has("--bench") {
        return bench();
    }
    if let Some(out) = val("--screenshot") {
        let scale = val("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        return screenshot(&out, has("--select"), scale, n);
    }
    let ui = ChatWindow::new().unwrap();
    let app = Rc::new(RefCell::new(App::new(turns(n))));
    wire(&ui, app.clone());
    if has("--stream") {
        stream(&ui, app);
    }
    ui.run().unwrap();
}
