//! The office on a thread of its own: the model and the renderer live there, the UI
//! sends it what happens (Hover's state, the pointer, a resize, the drawer or a panel
//! opening, being shown or hidden) and gets back each frame with the tags and what the
//! pointer is over. requestAnimationFrame's role: frames are made only when the page's
//! pacing wants one, and none while hidden.

use crate::office::{Click, Hover, Office, Prop, Tag, Time};
use crate::render::Renderer;
use hover_core::json::Json;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum In {
    State(Json),
    Resize(u32, u32),
    Pointer(Option<(f64, f64)>),
    Down(f64, f64),
    Up,
    Wheel(f64, f64, f64),
    Key(char),
    DoubleClick,
    Visible(bool),
    Drawer(Option<i64>),
    Panel(Option<&'static str>),
    Time(Option<Time>),
    /// The user's camera, when the page is made again (office.view).
    View([f64; 3]),
    Quit,
}

#[derive(Clone, Default)]
pub struct Out {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub tags: Vec<Tag>,
    pub hovered: Option<Hover>,
    pub hint: String,
    pub pointer: Option<(f64, f64)>,
    pub clicks: Vec<Click>,
    pub day: bool,
    pub frames: u64,
    pub adapter: String,
    pub error: Option<String>,
    /// The user's camera (office.view), kept by the app across a drop.
    pub view: [f64; 3],
    /// The page's picture: the frame over the background, with the vignette and border.
    pub rgb: Vec<u8>,
}

pub struct Live { tx: Sender<In>, pub out: Arc<Mutex<Out>> }

impl Live {
    /// `wake` is called (off the UI thread) when a new frame is waiting.
    pub fn start(w: u32, h: u32, still: bool, wake: impl Fn() + Send + 'static) -> Live {
        let (tx, rx) = channel();
        let out: Arc<Mutex<Out>> = Default::default();
        let o2 = out.clone();
        std::thread::Builder::new().name("office".into()).spawn(move || run(rx, o2, w, h, still, wake)).expect("the office's thread");
        Live { tx, out }
    }

    pub fn send(&self, m: In) { let _ = self.tx.send(m); }
    pub fn take(&self) -> Out { std::mem::take(&mut *self.out.lock().unwrap()) }
}

impl Drop for Live {
    fn drop(&mut self) { let _ = self.tx.send(In::Quit); }
}

fn hour_now() -> i64 {
    let t = hover_core::time::Stamp::now();
    let secs = (t.ticks - 621_355_968_000_000_000) / 10_000_000 + hover_core::time::local_offset_min(t.ticks) * 60;
    secs.rem_euclid(86400) / 3600
}

fn run(rx: Receiver<In>, out: Arc<Mutex<Out>>, w: u32, h: u32, still: bool, wake: impl Fn()) {
    let mut o = Office::new(w as f64, h as f64, still);
    let mut r = match Renderer::new(w, h) {
        Ok(r) => r,
        Err(e) => { out.lock().unwrap().error = Some(e); wake(); return; }
    };
    o.apply_time(Office::auto_time(hour_now()));
    let t0 = Instant::now();
    let mut last = 0.0;
    let (mut visible, mut down, mut prev) = (true, None::<(f64, f64)>, (0.0, 0.0));
    let mut clicks = vec![];
    let mut time_check = Instant::now();
    let mut page = crate::page::Composer::default();
    loop {
        // One frame's worth of waiting: 16 ms, as requestAnimationFrame.
        let msg = rx.recv_timeout(Duration::from_millis(if visible { 16 } else { 500 }));
        let mut msgs = vec![];
        match msg { Ok(m) => msgs.push(m), Err(RecvTimeoutError::Disconnected) => return, Err(RecvTimeoutError::Timeout) => {} }
        while let Ok(m) = rx.try_recv() { msgs.push(m); }
        for m in msgs {
            o.poke();
            match m {
                In::Quit => return,
                In::State(j) => o.state(&j),
                In::Resize(w, h) => { if w > 0 && h > 0 { o.resize(w as f64, h as f64); r.resize(w, h); } }
                In::Pointer(p) => {
                    if let (Some(d), Some(p)) = (down, p) {
                        if !o.dragging && (p.0 - d.0).hypot(p.1 - d.1) > 6.0 && !o.drawer_open && o.panel.is_none() { o.dragging = true; }
                        if o.dragging { o.drag(p.0 - prev.0, p.1 - prev.1); }
                    }
                    if let Some(p) = p { prev = p; }
                    o.pointer = p;
                }
                In::Down(x, y) => { down = Some((x, y)); prev = (x, y); o.dragging = false; }
                In::Up => {
                    let was = o.dragging;
                    o.dragging = false;
                    down = None;
                    if !was {
                        o.pick();
                        let c = o.click();
                        if let Click::Time(t) = c { o.manual_time = Some(t); o.apply_time(t); }
                        clicks.push(c);
                    }
                }
                In::Wheel(dy, x, y) => { if !o.drawer_open && o.panel.is_none() { o.zoom_by((-dy * 0.0015).exp(), x - o.w / 2.0, -(y - o.h / 2.0)); } }
                In::Key(c) => match c { '+' | '=' => o.zoom_by(1.25, 0.0, 0.0), '-' => o.zoom_by(0.8, 0.0, 0.0), '0' => o.reset_view(), _ => {} },
                In::DoubleClick => { if o.hovered.is_none() && !o.drawer_open && o.panel.is_none() { o.reset_view(); } }
                In::Visible(v) => visible = v,
                In::Drawer(id) => { o.drawer_open = id.is_some(); o.sel = id; o.draw_tv(o.clock_t); }
                In::Panel(p) => o.panel = p,
                In::View(v) => { o.user = v; o.clamp_view(); o.cam = [v[0], 1.7, v[1], v[2]]; }
                In::Time(t) => { o.manual_time = t; o.apply_time(t.unwrap_or(Office::auto_time(hour_now()))); }
            }
        }
        // The time of day follows the clock unless picked, checked every minute.
        if time_check.elapsed() > Duration::from_secs(60) {
            time_check = Instant::now();
            if o.manual_time.is_none() { let t = Office::auto_time(hour_now()); if t != o.time { o.apply_time(t); } }
        }
        if !visible { last = t0.elapsed().as_secs_f64() * 1000.0; continue; }
        let now = t0.elapsed().as_secs_f64() * 1000.0;
        let dt = now - last;
        last = now;
        if !o.frame(now, dt) && clicks.is_empty() { continue; }
        let rgba = r.render(&mut o);
        let rgb = page.compose(&rgba, r.w as usize, r.h as usize, o.time == Time::Day);
        let hint = match o.hovered { Some(Hover::Prop(Prop::Clock)) => String::from("clock"), Some(Hover::Prop(p)) => o.hint(p).to_owned(), _ => String::new() };
        *out.lock().unwrap() = Out { rgba, w: r.w, h: r.h, tags: o.tags(), hovered: o.hovered, hint, pointer: o.pointer, clicks: std::mem::take(&mut clicks),
            day: o.time == Time::Day, frames: o.frames, adapter: r.adapter_name.clone(), error: None, view: o.user, rgb };
        wake();
    }
}
