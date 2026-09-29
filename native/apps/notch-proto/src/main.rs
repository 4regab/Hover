//! Phase 1A prototype: the notch as Hover draws it, on Slint + winit + wgpu.
//!
//! Rendering: Slint's FemtoVG renderer on wgpu 30, DX12 only, with a swapchain made from a
//! DirectComposition visual (`Dx12SwapchainKind::DxgiFromVisual`) over a window created
//! with WS_EX_NOREDIRECTIONBITMAP, so per-pixel alpha reaches the desktop. The office
//! stand-in is a wgpu texture rendered on Slint's own device and shown as a Slint image
//! (no copy through the CPU).
//!
//!   notch-proto                      run it (Alt+N, hover the top centre, Esc)
//!   notch-proto --selftest out-dir   drive it from outside and write report.json + PNGs
//!   notch-proto --hit transparent    compare the click-through modes
// The Windows layer uses most of this; elsewhere the notch is only a dev window.
#![cfg_attr(not(windows), allow(unused))]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use hover_notch::{frame, open_size, outline, placement, rest_size, rim, Action, Hover, OfficeSize, Openness, Pointer, Rect, Rest, State, PAD};
use slint::ComponentHandle;

#[cfg(windows)]
mod win;

slint::include_modules!();

const SHADOW_BLUR: f64 = 24.0;
const SHADOW_DEPTH: f64 = 4.0;

struct Notch {
    hover: Hover,
    open: Openness,
    rest: (f64, f64),
    open_size: (f64, f64),
    scale: f64,
    work: Rect,
    win: Rect,
    t0: Instant,
    accepts_keys: bool,
    click_through: bool,
    frames: u64,
    anim_running: bool,
    #[cfg(windows)]
    hwnd: Option<win::Hwnd>,
    #[cfg(windows)]
    previous: Option<win::Hwnd>,
    #[cfg(windows)]
    hit_mode: win::HitMode,
    signature: String,
    last_display_check: Instant,
    log: Vec<String>,
}

type Shared = Rc<RefCell<Notch>>;

impl Notch {
    fn now(&self) -> f64 { self.t0.elapsed().as_secs_f64() * 1000.0 }
    fn log(&mut self, s: String) {
        eprintln!("[{:7.0} ms] {s}", self.now());
        self.log.push(format!("{:.0} {s}", self.now()));
    }
}

fn shape(ui: &NotchWindow, n: &Notch) {
    let t = n.open.value(n.now());
    let f = frame(t, n.rest, n.open_size);
    let win_w = n.open_size.0 + 2.0 * PAD;
    let x0 = (win_w - f.w) / 2.0;
    ui.set_shape_commands(outline(f.w, f.h, f.r, f.ear, x0).into());
    ui.set_rim_commands(rim(f.w, f.h, f.r, f.ear, x0).into());
    ui.set_shape_x(x0 as f32);
    ui.set_shape_w(f.w as f32);
    ui.set_shape_h(f.h as f32);
    ui.set_shape_r(f.r as f32);
    ui.set_rim_opacity(f.fill_mix as f32);
    ui.set_mini_opacity(f.mini_opacity as f32);
    ui.set_view_opacity(f.view_opacity as f32);
    ui.set_view_visible(t > 0.001 || n.open.animating(n.now()) && n.hover.state != State::Rest);
}

fn layout(ui: &NotchWindow, n: &mut Notch) {
    #[cfg(windows)]
    {
        let m = win::primary();
        n.work = m.work;
        n.scale = m.scale;
    }
    let work_dips = (n.work.width() as f64 / n.scale, n.work.height() as f64 / n.scale);
    n.open_size = open_size(OfficeSize::Default, work_dips);
    n.win = placement(n.work, n.scale, n.open_size);
    ui.set_open_w(n.open_size.0 as f32);
    ui.set_open_h(n.open_size.1 as f32);
    #[cfg(windows)]
    if let Some(h) = n.hwnd {
        win::place(h, n.win);
    }
    #[cfg(not(windows))]
    ui.window().set_size(slint::LogicalSize::new((n.open_size.0 + 2.0 * PAD) as f32, (n.open_size.1 + PAD) as f32));
    n.signature = signature();
    shape(ui, n);
}

fn signature() -> String {
    #[cfg(windows)]
    return win::monitors().iter().map(|m| format!("{}{}{:?}{:?}{}", m.device, m.primary, m.bounds, m.work, m.scale)).collect();
    #[cfg(not(windows))]
    String::new()
}

fn animate(ui: &NotchWindow, st: &Shared) {
    if st.borrow().anim_running {
        return;
    }
    st.borrow_mut().anim_running = true;
    let (w, s) = (ui.as_weak(), st.clone());
    let t = Rc::new(slint::Timer::default());
    let keep = t.clone();
    t.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(ui) = w.upgrade() else { return };
        let mut n = s.borrow_mut();
        shape(&ui, &n);
        if !n.open.animating(n.now()) {
            shape(&ui, &n);
            n.anim_running = false;
            keep.stop();
        }
    });
    // The timer lives until it stops itself.
    std::mem::forget(t);
}

/// Expand (NotchManager.Expand): NOACTIVATE comes off before the animation starts.
fn expand(ui: &NotchWindow, st: &Shared, peek: bool, focus: bool) {
    {
        let mut n = st.borrow_mut();
        #[cfg(windows)]
        if n.hover.state == State::Rest {
            n.previous = Some(win::foreground());
            n.accepts_keys = true;
            if let Some(h) = n.hwnd {
                win::apply_styles(h, true, n.click_through, n.hit_mode);
                win::raise(h);
            }
        }
        #[cfg(not(windows))]
        { n.accepts_keys = true; }
        n.hover.opened(peek);
        let now = n.now();
        n.open.go(1.0, now);
        let s = if n.hover.state == State::Peek { "peek" } else { "open" };
        n.log(format!("expand -> {s} (focus {focus})"));
    }
    ui.set_view_visible(true);
    animate(ui, st);
    if focus {
        #[cfg(windows)]
        if let Some(h) = st.borrow().hwnd { win::set_foreground(h); }
        ui.invoke_focus_input();
    }
}

/// Collapse: back to rest; the app that had the foreground gets it back.
fn collapse(ui: &NotchWindow, st: &Shared) {
    {
        let mut n = st.borrow_mut();
        if n.hover.state == State::Rest {
            return;
        }
        n.hover.collapsed();
        #[cfg(windows)]
        if let Some(h) = n.hwnd {
            if win::foreground() == h {
                if let Some(p) = n.previous.filter(|p| win::is_window(*p)) { win::set_foreground(p); }
            }
            n.accepts_keys = false;
            win::apply_styles(h, false, n.click_through, n.hit_mode);
        }
        let now = n.now();
        n.open.go(0.0, now);
        n.log("collapse".into());
    }
    animate(ui, st);
}

fn toggle(ui: &NotchWindow, st: &Shared) {
    if st.borrow().hover.state == State::Rest { expand(ui, st, false, true) } else { collapse(ui, st) }
}

/// The 50 ms poll (DispatcherPriority.Normal in the C#): pointer, hover rules, click-through.
fn poll(ui: &NotchWindow, st: &Shared) {
    #[cfg(windows)]
    for m in win::take_messages() {
        match m {
            win::Msg::Hotkey => toggle(ui, st),
            win::Msg::Deactivated => {
                let (h, state) = (st.borrow().hwnd, st.borrow().hover.state);
                if let Some(h) = h {
                    // Click-away: only when the new foreground isn't ours (or owned by us).
                    let fg = win::foreground();
                    if state != State::Rest && !fg.is_invalid() && !win::is_ours(fg, h) { collapse(ui, st); }
                }
            }
        }
    }
    #[cfg(windows)]
    {
        let (x, y) = win::cursor();
        let buttons = win::buttons_down();
        let (zone, panel, rest, win_r, scale, state) = {
            let n = st.borrow();
            (hover_notch::zone(n.work, n.scale, n.rest), hover_notch::panel_zone(n.work, n.scale, n.open_size), n.rest, n.win, n.scale, n.hover.state)
        };
        let _ = rest;
        let p = Pointer { in_zone: zone.contains(x, y), in_panel: panel.contains(x, y), buttons, popover: false, hover_opens: true };
        let now = st.borrow().now() as u64;
        let act = st.borrow_mut().hover.poll(now, &p);
        match act {
            Some(Action::Peek) => expand(ui, st, true, false),
            Some(Action::Collapse) => collapse(ui, st),
            None => {}
        }
        // Click-through: the window takes the pointer only over the shape (and its shadow).
        let mut n = st.borrow_mut();
        let t = n.open.value(n.now());
        let f = frame(t, n.rest, n.open_size);
        let (dx, dy) = ((x - win_r.left) as f64 / scale, (y - win_r.top) as f64 / scale);
        let over = win_r.contains(x, y) && hover_notch::hittable(dx, dy, n.open_size.0 + 2.0 * PAD, &f, SHADOW_BLUR, SHADOW_DEPTH);
        let through = !over;
        if through != n.click_through {
            n.click_through = through;
            if let Some(h) = n.hwnd { win::apply_styles(h, n.accepts_keys, through, n.hit_mode); }
        }
        let _ = state;
        // Display changes: re-place every 2 s when the monitor layout changed.
        if n.last_display_check.elapsed() > Duration::from_secs(2) {
            n.last_display_check = Instant::now();
            if signature() != n.signature {
                n.log("displays changed".into());
                drop(n);
                layout(ui, &mut st.borrow_mut());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (ui, st);
}

const SCENE_WGSL: &str = r#"
struct V { @builtin(position) pos: vec4f, @location(0) uv: vec2f };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    var p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    var o: V;
    o.pos = vec4f(p[i], 0.0, 1.0);
    o.uv = p[i] * vec2f(0.5, -0.5) + vec2f(0.5, 0.5);
    return o;
}
@fragment fn fs(v: V) -> @location(0) vec4f {
    // #office's radial background, and an isometric floor grid as a stand-in scene.
    let d = length((v.uv - vec2f(0.5, 0.45)) * vec2f(1.0, 1.25));
    var c = mix(vec3f(0.165, 0.094, 0.141), vec3f(0.027, 0.020, 0.039), clamp(d * 1.6, 0.0, 1.0));
    let g = abs(fract((v.uv.x + v.uv.y * 2.4) * 18.0) - 0.5) + abs(fract((v.uv.x - v.uv.y * 2.4) * 18.0) - 0.5);
    let floor = step(0.55, v.uv.y) * (1.0 - smoothstep(0.0, 0.06, min(g, 1.0 - g)));
    c = c + vec3f(0.35, 0.24, 0.20) * floor * 0.5;
    return vec4f(c, 1.0);
}
"#;

/// The office stand-in, on Slint's wgpu device: a texture rendered once by a shader and
/// handed to Slint as an image. Proves the shared device and the no-readback path.
fn install_scene(ui: &NotchWindow, st: &Shared) {
    let w = ui.as_weak();
    let s = st.clone();
    let made = Rc::new(RefCell::new(false));
    let res = ui.window().set_rendering_notifier(move |state, api| match state {
        slint::RenderingState::RenderingSetup | slint::RenderingState::BeforeRendering => {
            if *made.borrow() { return; }
            let slint::GraphicsAPI::WGPU30 { device, queue, .. } = api else { return };
            *made.borrow_mut() = true;
            use slint::wgpu_30::wgpu;
            let (tw, th) = (1104u32, 424u32);
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("office stand-in"), size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()) });
            let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None, layout: None,
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())], compilation_options: Default::default() }),
                primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
            });
            let view = tex.create_view(&Default::default());
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipe);
                pass.draw(0..3, 0..1);
            }
            queue.submit([enc.finish()]);
            let w = w.clone();
            let s = s.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let (Some(ui), Ok(img)) = (w.upgrade(), slint::Image::try_from(tex)) {
                    ui.set_scene(img);
                    s.borrow_mut().log("scene texture made on Slint's wgpu device".into());
                }
            });
        }
        slint::RenderingState::AfterRendering => s.borrow_mut().frames += 1,
        _ => {}
    });
    if let Err(e) = res {
        st.borrow_mut().log(format!("rendering notifier: {e:?}"));
    }
}

fn select_backend() -> Result<(), slint::PlatformError> {
    use slint::wgpu_30::{wgpu, WGPUConfiguration, WGPUSettings};
    let mut s = WGPUSettings::default();
    if cfg!(windows) {
        // DX12, with the swapchain on a DirectComposition visual: the only DXGI path that
        // keeps per-pixel alpha. An HWND swapchain shows black where the notch is empty.
        s.backends = wgpu::Backends::DX12;
        s.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    s.power_preference = wgpu::PowerPreference::LowPower;
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg-wgpu".into())
        .require_wgpu_30(WGPUConfiguration::Automatic(s))
        .with_winit_window_attributes_hook(|a| {
            let a = a.with_transparent(true).with_decorations(false).with_active(false).with_resizable(false)
                .with_window_level(slint::winit_030::winit::window::WindowLevel::AlwaysOnTop)
                .with_position(slint::winit_030::winit::dpi::PhysicalPosition::new(-32000, -32000));
            #[cfg(windows)]
            let a = {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                a.with_no_redirection_bitmap(true).with_skip_taskbar(true).with_undecorated_shadow(false).with_class_name("HoverNotch")
            };
            a
        })
        .select()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    #[cfg(windows)]
    if let Some(r) = val("--helper-bg") {
        let v: Vec<i32> = r.split(',').filter_map(|s| s.parse().ok()).collect();
        win::run_helper(Rect { left: v[0], top: v[1], right: v[2], bottom: v[3] });
        return;
    }
    if let Err(e) = select_backend() {
        eprintln!("backend: {e}");
        std::process::exit(3);
    }
    let ui = NotchWindow::new().unwrap();
    let rest = match val("--rest").as_deref() {
        Some("none") => Rest::None,
        // The question's card (the alert it replaced is gone).
        Some("alert") => Rest::Card(500.0, 182.0),
        _ => Rest::Pill(150.0),
    };
    let st: Shared = Rc::new(RefCell::new(Notch {
        hover: Hover::default(), open: Openness::default(), rest: rest_size(rest), open_size: (1120.0, 440.0), scale: 1.0,
        work: Rect { left: 0, top: 0, right: 1920, bottom: 1040 }, win: Rect { left: 0, top: 0, right: 1, bottom: 1 },
        t0: Instant::now(), accepts_keys: false, click_through: true, frames: 0, anim_running: false,
        #[cfg(windows)] hwnd: None,
        #[cfg(windows)] previous: None,
        #[cfg(windows)] hit_mode: if val("--hit").as_deref() == Some("transparent") { win::HitMode::Transparent } else { win::HitMode::Layered },
        signature: String::new(), last_display_check: Instant::now(), log: vec![],
    }));
    install_scene(&ui, &st);
    {
        let (w, s) = (ui.as_weak(), st.clone());
        ui.on_escape(move || if let Some(ui) = w.upgrade() { collapse(&ui, &s) });
        let (w, s) = (ui.as_weak(), st.clone());
        // A press on a peeking notch makes it stay (PreviewMouseDown -> Open).
        ui.on_shape_pressed(move || if let Some(ui) = w.upgrade() {
            if s.borrow().hover.state == State::Peek { s.borrow_mut().hover.opened(false); s.borrow_mut().log("peek -> open (press)".into()); }
            let _ = ui;
        });
        let (w, s) = (ui.as_weak(), st.clone());
        // A click on the resting pill opens it (without the keyboard).
        ui.on_shape_clicked(move || if let Some(ui) = w.upgrade() {
            if s.borrow().hover.state == State::Rest && s.borrow().rest.0 > 0.0 { expand(&ui, &s, false, false); }
        });
    }
    ui.show().unwrap();
    #[cfg(windows)]
    {
        let h = win::hwnd_of(ui.window());
        st.borrow_mut().hwnd = h;
        if let Some(h) = h {
            let mode = st.borrow().hit_mode;
            win::apply_styles(h, false, true, mode);
            win::disable_transitions(h);
            let (w, s) = (ui.as_weak(), st.clone());
            let ok = win::hook(h, move || {
                let (w, s) = (w.clone(), s.clone());
                slint::Timer::single_shot(Duration::ZERO, move || if let Some(ui) = w.upgrade() { poll(&ui, &s) });
            });
            st.borrow_mut().log(format!("hotkey Alt+N registered: {ok}"));
        }
    }
    layout(&ui, &mut st.borrow_mut());
    let poll_timer = slint::Timer::default();
    {
        let (w, s) = (ui.as_weak(), st.clone());
        poll_timer.start(slint::TimerMode::Repeated, Duration::from_millis(hover_notch::POLL_MS), move || if let Some(ui) = w.upgrade() { poll(&ui, &s) });
    }
    #[cfg(windows)]
    if let Some(dir) = val("--selftest") {
        selftest::start(&ui, &st, dir);
    }
    #[cfg(not(windows))]
    if val("--selftest").is_some() {
        eprintln!("the self-test drives the Windows desktop; on this OS the notch runs as a plain dev window");
        return;
    }
    ui.run().unwrap();
}

#[cfg(windows)]
mod selftest {
    //! Drives the notch the way a person does, and watches it from another process: the
    //! helper window underneath gets the clicks that pass through, the screen shows what
    //! DWM composed, and the foreground says who has the keyboard.
    use super::*;
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    struct T {
        out: std::path::PathBuf,
        report: serde_json::Map<String, Value>,
        helper: Option<std::process::Child>,
        lines: Arc<Mutex<Vec<String>>>,
        helper_hwnd: Option<win::Hwnd>,
        marks: (u64, f64),
    }

    impl T {
        fn clicks(&self) -> usize { self.lines.lock().unwrap().iter().filter(|l| l.starts_with("click")).count() }
        fn check(&mut self, name: &str, pass: bool, detail: Value) {
            eprintln!("{} {name} {detail}", if pass { "PASS" } else { "FAIL" });
            self.report.insert(name.into(), json!({ "pass": pass, "detail": detail }));
        }
        fn shot(&self, name: &str, r: Rect) -> (u32, u32, Vec<u8>) {
            let (w, h, rgb) = win::capture(r);
            if let Some(img) = image::RgbImage::from_raw(w, h, rgb.clone()) { let _ = img.save(self.out.join(format!("{name}.png"))); }
            (w, h, rgb)
        }
    }

    fn px(img: &(u32, u32, Vec<u8>), x: i32, y: i32) -> [u8; 3] {
        let (w, h, d) = img;
        if x < 0 || y < 0 || x >= *w as i32 || y >= *h as i32 { return [0, 0, 0]; }
        let i = ((y as u32 * w + x as u32) * 3) as usize;
        [d[i], d[i + 1], d[i + 2]]
    }
    fn magenta(c: [u8; 3]) -> bool { c[0] > 230 && c[1] < 30 && c[2] > 230 }
    fn dark(c: [u8; 3]) -> bool { (c[0] as u32 + c[1] as u32 + c[2] as u32) < 60 }

    pub fn start(ui: &NotchWindow, st: &Shared, dir: String) {
        let out = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&out);
        let t = Rc::new(RefCell::new(T { out, report: Default::default(), helper: None, lines: Arc::new(Mutex::new(vec![])), helper_hwnd: None, marks: (0, 0.0) }));
        type Step = Box<dyn FnMut(&NotchWindow, &Shared, &mut T)>;
        let mut steps: Vec<(u64, Step)> = vec![];
        let at = |ms: u64, f: Step, steps: &mut Vec<(u64, Step)>| steps.push((ms, f));
        // Geometry in device pixels.
        fn g(st: &Shared) -> (Rect, f64, i32, (f64, f64)) {
            let n = st.borrow();
            (n.win, n.scale, n.win.left + n.win.width() / 2, n.open_size)
        }
        at(300, Box::new(|_, st, t| {
            let (r, ..) = g(st);
            let cover = Rect { left: r.left - 60, top: r.top, right: r.right + 60, bottom: r.bottom + 120 };
            let exe = std::env::current_exe().unwrap();
            let mut child = Command::new(exe).args(["--helper-bg", &format!("{},{},{},{}", cover.left, cover.top, cover.right, cover.bottom)])
                .stdout(Stdio::piped()).spawn().expect("helper");
            let stdout = child.stdout.take().unwrap();
            let lines = t.lines.clone();
            std::thread::spawn(move || for l in BufReader::new(stdout).lines().map_while(Result::ok) { lines.lock().unwrap().push(l); });
            t.helper = Some(child);
        }), &mut steps);
        at(2000, Box::new(|_, st, t| {
            t.helper_hwnd = t.lines.lock().unwrap().iter().find_map(|l| l.strip_prefix("hwnd ").and_then(|v| v.parse::<isize>().ok())).map(|v| win::Hwnd(v as *mut _));
            let n = st.borrow();
            let h = n.hwnd.unwrap();
            let ex = win::ex_style(h) as u32;
            let actual = win::window_rect(h);
            let m = win::primary();
            t.report.insert("monitors".into(), json!(win::monitors().iter().map(|m| json!({"device": m.device, "primary": m.primary, "bounds": format!("{:?}", m.bounds), "work": format!("{:?}", m.work), "scale": m.scale})).collect::<Vec<_>>()));
            t.report.insert("adapter_note".into(), json!("see the wgpu log line; CI runners have no GPU (WARP)"));
            drop(n);
            t.check("placement_primary_work_area", actual == st.borrow().win, json!({"expected": format!("{:?}", st.borrow().win), "actual": format!("{:?}", actual), "scale": m.scale}));
            t.check("styles_at_rest", ex & 0x80 != 0 && ex & 0x0800_0000 != 0 && ex & 0x8 != 0, json!({"exstyle": format!("{ex:#x}"), "want": "TOOLWINDOW|NOACTIVATE|TOPMOST"}));
            let fg = win::foreground();
            t.check("resting_notch_leaves_foreground", Some(fg) == t.helper_hwnd, json!({"foreground": fg.0 as isize, "helper": t.helper_hwnd.map(|h| h.0 as isize)}));
        }), &mut steps);
        at(2300, Box::new(|_, st, t| {
            let (r, k, cx, _) = g(st);
            let img = t.shot("rest", r);
            let (pad, corner, shape) = (px(&img, cx - r.left, ((24.0 + 30.0) * k) as i32), px(&img, 4, r.height() - 4), px(&img, cx - r.left, (12.0 * k) as i32));
            t.check("transparent_over_desktop_at_rest", magenta(pad) && magenta(corner) && dark(shape), json!({"below_pill": pad, "window_corner": corner, "pill": shape}));
        }), &mut steps);
        at(2600, Box::new(|_, st, t| {
            let (r, k, cx, _) = g(st);
            t.marks.0 = t.clicks() as u64;
            let (x, y) = (cx, r.top + ((24.0 + 60.0) * k) as i32);
            let wfp = win::window_from_point(x, y);
            t.report.insert("window_from_point_below_pill".into(), json!({"hwnd": wfp.0 as isize, "is_helper": Some(wfp) == t.helper_hwnd}));
            win::click_at(x, y);
        }), &mut steps);
        at(3000, Box::new(|_, _, t| {
            let n = t.clicks() as u64 - t.marks.0;
            t.check("click_through_empty_area", n == 1, json!({"helper_clicks": n}));
            t.marks.0 = t.clicks() as u64;
        }), &mut steps);
        at(3100, Box::new(|_, st, _| { let (r, k, cx, _) = g(st); win::click_at(cx, r.top + (12.0 * k) as i32); }), &mut steps);
        at(3800, Box::new(|_, st, t| {
            let n = t.clicks() as u64 - t.marks.0;
            let state = st.borrow().hover.state;
            let h = st.borrow().hwnd.unwrap();
            let ex = win::ex_style(h) as u32;
            let fg = win::foreground();
            t.check("click_on_pill_opens_without_stealing_focus", n == 0 && state == State::Open && ex & 0x0800_0000 == 0 && Some(fg) == t.helper_hwnd,
                json!({"helper_clicks": n, "state": format!("{state:?}"), "exstyle": format!("{ex:#x}"), "foreground_is_helper": Some(fg) == t.helper_hwnd}));
            let (r, k, cx, open) = g(st);
            let img = t.shot("open", r);
            let (corner, inside) = (px(&img, 4, r.height() - 4), px(&img, cx - r.left, (open.1 * 0.5 * k) as i32));
            t.check("transparent_over_desktop_open", magenta(corner) && !magenta(inside), json!({"window_corner": corner, "office": inside}));
        }), &mut steps);
        at(4000, Box::new(|_, st, _| {
            let (r, k, _, open) = g(st);
            // The composer box: x 22..432, y open.h-70..open.h-22 inside the view.
            win::click_at(r.left + ((PAD + 22.0 + 200.0) * k) as i32, r.top + ((open.1 - 46.0) * k) as i32);
        }), &mut steps);
        at(4300, Box::new(|_, _, _| win::type_text("hello ÅÎ日本")), &mut steps);
        at(4900, Box::new(|ui, st, t| {
            let h = st.borrow().hwnd.unwrap();
            let fg = win::foreground();
            t.check("composer_takes_focus_and_text", fg == h && ui.get_draft() == "hello ÅÎ日本", json!({"draft": ui.get_draft().to_string(), "foreground_is_notch": fg == h}));
            win::escape();
        }), &mut steps);
        at(5400, Box::new(|_, st, t| {
            let h = st.borrow().hwnd.unwrap();
            let ex = win::ex_style(h) as u32;
            let fg = win::foreground();
            let state = st.borrow().hover.state;
            t.check("esc_collapses_and_restores_foreground", state == State::Rest && ex & 0x0800_0000 != 0 && Some(fg) == t.helper_hwnd,
                json!({"state": format!("{state:?}"), "exstyle": format!("{ex:#x}"), "foreground_is_helper": Some(fg) == t.helper_hwnd}));
            let (r, k, cx, _) = g(st);
            let img = t.shot("collapsed", r);
            t.check("transparent_after_collapse", magenta(px(&img, cx - r.left, ((24.0 + 30.0) * k) as i32)), json!({}));
            win::move_to(cx, r.top + 1);
        }), &mut steps);
        at(5900, Box::new(|_, st, t| {
            let state = st.borrow().hover.state;
            let fg = win::foreground();
            t.check("hover_peeks_without_activating", state == State::Peek && Some(fg) == t.helper_hwnd, json!({"state": format!("{state:?}"), "foreground_is_helper": Some(fg) == t.helper_hwnd}));
            let (r, k, cx, open) = g(st);
            win::move_to(cx, r.top + ((open.1 + 120.0) * k) as i32);
        }), &mut steps);
        at(6700, Box::new(|_, st, t| {
            let state = st.borrow().hover.state;
            t.check("peek_folds_after_leave_grace", state == State::Rest, json!({"state": format!("{state:?}")}));
            win::alt_n();
        }), &mut steps);
        at(7300, Box::new(|_, _, _| win::type_text("x")), &mut steps);
        at(7700, Box::new(|ui, st, t| {
            let h = st.borrow().hwnd.unwrap();
            let fg = win::foreground();
            let state = st.borrow().hover.state;
            t.check("hotkey_opens_with_keyboard", state == State::Open && fg == h && ui.get_draft().ends_with('x'), json!({"state": format!("{state:?}"), "foreground_is_notch": fg == h, "draft": ui.get_draft().to_string()}));
            win::escape();
        }), &mut steps);
        at(8500, Box::new(|_, st, t| {
            t.marks = (st.borrow().frames, win::cpu_ms());
        }), &mut steps);
        at(11500, Box::new(|_, st, t| {
            let frames = st.borrow().frames - t.marks.0;
            let cpu = win::cpu_ms() - t.marks.1;
            t.check("no_redraws_while_resting", frames == 0, json!({"frames_in_3s": frames, "cpu_ms_in_3s": cpu, "private_bytes": win::private_bytes()}));
            t.report.insert("log".into(), json!(st.borrow().log));
            let _ = std::fs::write(t.out.join("report.json"), serde_json::to_string_pretty(&Value::Object(t.report.clone())).unwrap());
            if let Some(mut c) = t.helper.take() { let _ = c.kill(); }
            let fails = t.report.values().filter(|v| v.get("pass") == Some(&json!(false))).count();
            eprintln!("self-test done: {fails} failed; report in {}", t.out.display());
            let _ = slint::quit_event_loop();
        }), &mut steps);

        let t0 = Instant::now();
        let steps = Rc::new(RefCell::new(steps.into_iter().map(Some).collect::<Vec<_>>()));
        let timer = slint::Timer::default();
        let (w, s) = (ui.as_weak(), st.clone());
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(20), move || {
            let Some(ui) = w.upgrade() else { return };
            let now = t0.elapsed().as_millis() as u64;
            for step in steps.borrow_mut().iter_mut() {
                if matches!(step, Some((ms, _)) if *ms <= now) {
                    let (_, mut f) = step.take().unwrap();
                    f(&ui, &s, &mut t.borrow_mut());
                }
            }
        });
        std::mem::forget(timer);
        // A hard stop, whatever happens.
        slint::Timer::single_shot(Duration::from_secs(40), || { let _ = slint::quit_event_loop(); });
    }
}
