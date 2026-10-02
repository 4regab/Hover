//! The office on a thread of its own: the model and the renderer live there, the UI
//! sends it what happens (Hover's state, the pointer, a resize, the drawer or a panel
//! opening, being shown or hidden) and gets back each frame with the tags and what the
//! pointer is over. requestAnimationFrame's role: frames are made only when the page's
//! pacing wants one, and none while hidden.

use crate::office::{Click, Hover, Office, Prop, Tag, Time};
use crate::render::Renderer;
use hover_core::json::Json;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Slint would not take the office's textures: go back to reading frames back and
    /// composing them on the CPU, rather than show nothing.
    NoGpu,
    Quit,
}

/// The slots' textures and which set they belong to. The set changes when the office is
/// resized, so the app knows the images it made are for the old size.
#[derive(Default)]
pub struct Slots {
    pub gen: u64,
    pub tex: Vec<(wgpu::Texture, wgpu::Texture)>,
}

#[derive(Clone, Default)]
pub struct Out {
    pub w: u32,
    pub h: u32,
    pub tags: Vec<Tag>,
    pub hovered: Option<Hover>,
    pub hint: String,
    pub pointer: Option<(f64, f64)>,
    /// Every click since the UI last took a frame, oldest first (a frame the UI hadn't
    /// taken yet is replaced by the next, but its clicks carry over).
    pub clicks: Vec<Click>,
    pub day: bool,
    pub frames: u64,
    pub adapter: String,
    pub error: Option<String>,
    /// The user's camera (office.view), kept by the app across a drop.
    pub view: [f64; 3],
    /// The page's picture: the frame over the background, with the vignette and border.
    /// Empty when only clicks came, and on the GPU path (`slot` carries it instead).
    /// Hand it back with `Live::recycle` once drawn.
    pub rgb: Vec<u8>,
    /// The GPU path: which slot holds this frame, and which set of slots it is from.
    /// Hand it back with `Live::release` once a later frame has reached Slint.
    pub slot: Option<(u64, usize)>,
}

pub struct Live {
    tx: Sender<In>,
    pub out: Arc<Mutex<Out>>,
    spare: Arc<Mutex<Vec<u8>>>,
    slots: Arc<Mutex<Slots>>,
    freed: Arc<Mutex<Vec<(u64, usize)>>>,
}

impl Live {
    /// `wake` is called (off the UI thread) when a new frame is waiting.
    pub fn start(w: u32, h: u32, still: bool, wake: impl Fn() + Send + 'static) -> Live {
        let (tx, rx) = channel();
        let out: Arc<Mutex<Out>> = Default::default();
        let spare: Arc<Mutex<Vec<u8>>> = Default::default();
        let slots: Arc<Mutex<Slots>> = Default::default();
        let freed: Arc<Mutex<Vec<(u64, usize)>>> = Default::default();
        let (o2, s2, sl2, f2) = (out.clone(), spare.clone(), slots.clone(), freed.clone());
        std::thread::Builder::new().name("office".into()).spawn(move || run(rx, o2, s2, sl2, f2, w, h, still, wake)).expect("the office's thread");
        Live { tx, out, spare, slots, freed }
    }

    pub fn send(&self, m: In) { let _ = self.tx.send(m); }
    pub fn take(&self) -> Out { std::mem::take(&mut *self.out.lock().unwrap()) }
    /// A frame's picture, drawn: its buffer is used again for a later frame.
    pub fn recycle(&self, rgb: Vec<u8>) { if rgb.capacity() > 0 { *self.spare.lock().unwrap() = rgb; } }

    /// Every slot's page and glass texture, and which set they are. Empty when the office
    /// reads its frames back instead.
    pub fn slot_textures(&self) -> (u64, Vec<(wgpu::Texture, wgpu::Texture)>) {
        let s = self.slots.lock().unwrap();
        (s.gen, s.tex.clone())
    }

    /// A slot no window shows any more: the office may draw into it again. Nothing is
    /// drawn into it until this is called, so a texture Slint still samples is never
    /// written over.
    pub fn release(&self, gen: u64, slot: usize) { self.freed.lock().unwrap().push((gen, slot)); }
}

impl Drop for Live {
    fn drop(&mut self) { let _ = self.tx.send(In::Quit); }
}

fn hour_now() -> i64 {
    let t = hover_core::time::Stamp::now();
    let secs = (t.ticks - 621_355_968_000_000_000) / 10_000_000 + hover_core::time::local_offset_min(t.ticks) * 60;
    secs.rem_euclid(86400) / 3600
}

#[allow(clippy::too_many_arguments)]
fn run(rx: Receiver<In>, out: Arc<Mutex<Out>>, spare: Arc<Mutex<Vec<u8>>>, slots: Arc<Mutex<Slots>>, freed: Arc<Mutex<Vec<(u64, usize)>>>, w: u32, h: u32, still: bool, wake: impl Fn()) {
    let mut o = Office::new(w as f64, h as f64, still);
    let mut r = match Renderer::new(w, h) {
        Ok(r) => r,
        Err(e) => { out.lock().unwrap().error = Some(e); wake(); return; }
    };
    // On the CPU adapter (WARP on a VM or an RDP host with no GPU) a full-size frame
    // took about 75 ms of every core, and the windows draw on the same device: the
    // office at half size costs a quarter of that, and is shown scaled up (pixelated,
    // as the voxels are).
    let scale = if r.software { 0.5 } else { 1.0 };
    let px = |n: u32| ((n as f64 * scale).round() as u32).max(1);
    if scale != 1.0 { r.resize(px(w), px(h)); }
    o.apply_time(Office::auto_time(hour_now()));
    // The frame stays on the GPU when the app shares its device with the office, so the
    // windows can draw the office's own textures (Windows). Elsewhere, and when
    // HOVER_OFFICE_READBACK is set to measure the two against each other, the frame is
    // read back and composed on the CPU.
    let mut gen = 0u64;
    let mut gpu = (r.shared && std::env::var_os("HOVER_OFFICE_READBACK").is_none()).then(|| {
        let g = crate::page::Gpu::new(&r.device, &r.queue, &r.color_view(), r.w, r.h, o.time == Time::Day);
        gen = 1;
        *slots.lock().unwrap() = Slots { gen, tex: g.textures() };
        g
    });
    hover_core::log::line(if gpu.is_some() { "office: the frame and its composition stay on the GPU" } else { "office: the frame is read back and composed on the CPU" });
    // The slots nothing shows. One is drawn into, one may wait for the UI, and each
    // window that shows the office holds the one it last handed to Slint.
    let mut free: Vec<usize> = (0..crate::page::SLOTS).collect();
    // The last frame's own time on the GPU, measured without waiting for it.
    let gpu_us = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let mut last = 0.0;
    // A frame isn't started before this: the device gets twice a frame's own time to
    // itself after each one, so a slow one (WARP, an old GPU) is never asked for more
    // than it can draw, which left the windows waiting behind the office's frames.
    let mut rest_until = Instant::now();
    let (mut visible, mut down, mut prev) = (true, None::<(f64, f64)>, (0.0, 0.0));
    let mut clicks = vec![];
    let mut time_check = Instant::now();
    let mut page = crate::page::Composer::default();
    // The frame as read back, kept between frames.
    let mut rgba: Vec<u8> = vec![];
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
                In::Resize(w, h) => {
                    if w > 0 && h > 0 {
                        o.resize(w as f64, h as f64);
                        r.resize(px(w), px(h));
                        // The slots are the frame's size, so a new size needs new ones. The
                        // old textures stay alive while the windows still show them, and the
                        // new set's number tells the app its images are for the old size.
                        if let Some(g) = gpu.as_mut() {
                            if g.size() != (r.w, r.h) {
                                *g = crate::page::Gpu::new(&r.device, &r.queue, &r.color_view(), r.w, r.h, o.time == Time::Day);
                                gen += 1;
                                *slots.lock().unwrap() = Slots { gen, tex: g.textures() };
                                free = (0..crate::page::SLOTS).collect();
                                freed.lock().unwrap().clear();
                            }
                        }
                    }
                }
                In::NoGpu => {
                    if gpu.take().is_some() {
                        hover_core::log::line("office: Slint would not take the office's textures; reading frames back instead");
                        *slots.lock().unwrap() = Slots::default();
                    }
                }
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
        // Resting the device: the time waited is added to the next frame's step.
        if Instant::now() < rest_until && clicks.is_empty() { continue; }
        let now = t0.elapsed().as_secs_f64() * 1000.0;
        let dt = now - last;
        last = now;
        if !o.frame(now, dt) && clicks.is_empty() { continue; }
        let mut rgb = Vec::new();
        let mut slot_out = None;
        if let Some(g) = gpu.as_mut() {
            // Slots the windows have finished with.
            for (gn, i) in freed.lock().unwrap().drain(..) { if gn == gen && !free.contains(&i) { free.push(i); } }
            // Nothing free: the UI is behind, so this frame is dropped rather than drawn
            // over a texture a window still shows. The clicks wait for the next one.
            let Some(slot) = free.pop() else { continue };
            g.sync(&r.queue, o.time == Time::Day);
            let spent = Instant::now();
            r.render_gpu(&mut o, g, slot);
            // Resting the device still needs the frame's cost, and nothing waits for the
            // GPU here: the time is taken when the work reports done, which happens as the
            // windows' own drawing polls the device.
            let us = gpu_us.clone();
            r.queue.on_submitted_work_done(move || us.store(spent.elapsed().as_micros() as u64, Ordering::Relaxed));
            rest_until = Instant::now() + Duration::from_micros(gpu_us.load(Ordering::Relaxed)) * 2;
            slot_out = Some((gen, slot));
        } else {
            let spent = Instant::now();
            r.render_into(&mut o, &mut rgba);
            // The buffer the UI gave back, or a new one.
            rgb = std::mem::take(&mut *spare.lock().unwrap());
            page.compose_into(&rgba, r.w as usize, r.h as usize, o.time == Time::Day, &mut rgb);
            rest_until = Instant::now() + spent.elapsed() * 2;
        }
        let hint = match o.hovered { Some(Hover::Prop(Prop::Clock)) => String::from("clock"), Some(Hover::Prop(p)) => o.hint(p).to_owned(), _ => String::new() };
        let mut g = out.lock().unwrap();
        // A frame the UI hasn't taken yet: its picture is replaced, its clicks are not,
        // and its slot goes back (no window ever saw it).
        let mut all = std::mem::take(&mut g.clicks);
        all.append(&mut clicks);
        if let Some((gn, prev)) = g.slot { if gn == gen && Some(prev) != slot_out.map(|(_, s)| s) && !free.contains(&prev) { free.push(prev); } }
        let old = std::mem::replace(&mut *g, Out { w: r.w, h: r.h, tags: o.tags(), hovered: o.hovered, hint, pointer: o.pointer, clicks: all,
            day: o.time == Time::Day, frames: o.frames, adapter: r.adapter_name.clone(), error: None, view: o.user, rgb, slot: slot_out });
        drop(g);
        if old.rgb.capacity() > 0 { let mut s = spare.lock().unwrap(); if s.capacity() == 0 { *s = old.rgb; } }
        wake();
    }
}
