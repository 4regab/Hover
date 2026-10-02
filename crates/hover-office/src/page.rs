//! What page.html lays around and over the canvas in host mode: #office's radial
//! background (night or day) and the ::after vignette. Since 639c01c the page drops
//! #office's rounded corners and border in Hover (body.host #office), so the office
//! fills its box edge to edge. CSS gradients are interpolated premultiplied
//! in sRGB; ellipse radii are percentages of the box.

use crate::canvas::css;

fn ramp(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    let mut i = 0;
    while i + 1 < stops.len() && stops[i + 1].0 < t { i += 1; }
    let (a, b) = (stops[i], stops[(i + 1).min(stops.len() - 1)]);
    let k = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
    let pa = [a.1[0] * a.1[3], a.1[1] * a.1[3], a.1[2] * a.1[3], a.1[3]];
    let pb = [b.1[0] * b.1[3], b.1[1] * b.1[3], b.1[2] * b.1[3], b.1[3]];
    std::array::from_fn(|j| pa[j] + (pb[j] - pa[j]) * k)
}

/// radial-gradient(rx% ry% at cx% cy%, stops): premultiplied RGBA at a pixel centre.
fn radial(x: f64, y: f64, w: f64, h: f64, r: (f64, f64), at: (f64, f64), stops: &[(f64, [f64; 4])]) -> [f64; 4] {
    let (dx, dy) = ((x - at.0 * w) / (r.0 * w), (y - at.1 * h) / (r.1 * h));
    ramp(stops, dx.hypot(dy))
}

/// The office as the page shows it: the frame (premultiplied RGBA8 from the renderer)
/// over the background, and the vignette over both. RGB8.
pub fn compose(frame: &[u8], w: usize, h: usize, day: bool) -> Vec<u8> {
    let bg: [(f64, [f64; 4]); 3] = if day { [(0.0, css("#4a3530")), (0.6, css("#241815")), (1.0, css("#0e0a09"))] } else { [(0.0, css("#2a1824")), (0.55, css("#150c14")), (1.0, css("#07050a"))] };
    let vig = [(0.0, [0.0, 0.0, 0.0, 0.0]), (0.6, [0.0, 0.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 0.0, 0.45])];
    let (fw, fh) = (w as f64, h as f64);
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h { for x in 0..w {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let b = radial(px, py, fw, fh, (1.2, 0.9), (0.5, 0.45), &bg);
        let f = &frame[(y * w + x) * 4..(y * w + x) * 4 + 4];
        let fa = f[3] as f64 / 255.0;
        let mut c: [f64; 3] = std::array::from_fn(|k| f[k] as f64 / 255.0 + b[k] * (1.0 - fa));
        let v = radial(px, py, fw, fh, (1.3, 1.0), (0.5, 0.5), &vig);
        for k in 0..3 { c[k] = v[k] + c[k] * (1.0 - v[3]); }
        out.extend(c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
    } }
    out
}

/// compose(), with what doesn't change from frame to frame (the background, the
/// vignette) worked out once per size and time of day. Kept as bytes, not floats:
/// every value is a whole byte (or one from a byte difference), so the result is the
/// same, at 7 bytes a pixel instead of 28 (10 MB less at the default office size).
#[derive(Default)]
pub struct Composer { key: (usize, usize, bool), under: Vec<[u8; 3]>, over: Vec<[u8; 4]> }

impl Composer {
    pub fn compose(&mut self, frame: &[u8], w: usize, h: usize, day: bool) -> Vec<u8> {
        let mut out = Vec::new();
        self.compose_into(frame, w, h, day, &mut out);
        out
    }

    /// compose, into a buffer the caller keeps between frames.
    pub fn compose_into(&mut self, frame: &[u8], w: usize, h: usize, day: bool, out: &mut Vec<u8>) {
        if self.key != (w, h, day) || self.under.is_empty() {
            self.key = (w, h, day);
            // Two layers from compose() itself: the page with a clear frame (the
            // background), and with a white opaque one (what lies over the frame).
            let clear = compose(&vec![0u8; w * h * 4], w, h, day);
            self.under = clear.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            drop(clear);
            // Over the frame everything is linear in it: out = frame * k + c; k from white − black-opaque.
            let white = compose(&vec![255u8; w * h * 4], w, h, day);
            let black = compose(&[0u8, 0, 0, 255].repeat(w * h), w, h, day);
            self.over = white.chunks(3).zip(black.chunks(3)).map(|(a, b)| [a[0].saturating_sub(b[0]), b[0], b[1], b[2]]).collect();
        }
        out.clear();
        out.reserve(w * h * 3);
        for (i, p) in frame.chunks(4).enumerate() {
            let a = p[3] as f32 / 255.0;
            let (u, o) = (self.under[i], self.over[i]);
            let k = o[0] as f32 / 255.0;
            for c in 0..3 {
                // The frame over its background, then what lies over both; the part of the
                // background the frame lets through is under's, which already carries it.
                let v = p[c] as f32 * k + o[c + 1] as f32 * a + u[c] as f32 * (1.0 - a);
                out.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
}


/// How many frames the office keeps textures for. One is being drawn into, one may be
/// waiting for the UI to take it, and each window that shows the office holds the one it
/// last handed to Slint (the notch and the app window, so two): four is the most that can
/// be in use at once, so the office never has to wait for a slot.
pub const SLOTS: usize = 4;

/// The number of uniform slices in `Gpu::uni`, one per pass kind.
const PASSES: u64 = 5;
/// wgpu's default `min_uniform_buffer_offset_alignment`.
const SLICE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassU {
    size: [f32; 2],
    dir: [i32; 2],
    bg: [[f32; 4]; 3],
    bg_at: [f32; 4],
    vig: [[f32; 4]; 3],
    vig_at: [f32; 4],
}

// page.wgsl's `U`, which naga lays out at the same offsets: size 0, dir 8, bg 16, bg_at
// 64, vig 80, vig_at 128. A field added or reordered on one side only would read the
// background's colours from the wrong bytes, which no compiler would catch.
const _: () = assert!(std::mem::size_of::<PassU>() == 144);

/// One frame's two finished pictures, which Slint samples as they are.
struct Slot {
    page: wgpu::Texture,
    glass: wgpu::Texture,
    page_view: wgpu::TextureView,
    glass_view: wgpu::TextureView,
    /// The blur's first pass reads this slot's own composed picture.
    down: wgpu::BindGroup,
}

/// compose() and the glass blur as GPU passes, into textures Slint draws directly: no
/// readback, no CPU composition, no upload. Everything that depends only on the size is
/// made once here and used again every frame.
pub struct Gpu {
    compose: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    boxp: wgpu::RenderPipeline,
    sat: wgpu::RenderPipeline,
    uni: wgpu::Buffer,
    slots: Vec<Slot>,
    ping_view: wgpu::TextureView,
    pong_view: wgpu::TextureView,
    /// The scene the renderer drew, which the compose pass reads.
    scene: wgpu::BindGroup,
    ping: wgpu::BindGroup,
    pong: wgpu::BindGroup,
    w: u32,
    h: u32,
    day: bool,
}

const PAGE_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The blur's working pair. The CPU blur keeps full float precision through all seven
/// passes; 8-bit intermediates would quantise at each one.
const WORK_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

impl Gpu {
    /// `scene` is the renderer's colour target, at `w` x `h`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, scene: &wgpu::TextureView, w: u32, h: u32, day: bool) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("page"), source: wgpu::ShaderSource::Wgsl(include_str!("page.wgsl").into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<PassU>() as u64) }, count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("page"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipe = |name: &str, entry: &str, fmt: wgpu::TextureFormat| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(name), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(entry), targets: &[Some(wgpu::ColorTargetState { format: fmt, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let uni = device.create_buffer(&wgpu::BufferDescriptor { label: Some("page"), size: SLICE * PASSES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let tex = |label: &str, tw: u32, th: u32, f: wgpu::TextureFormat| device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label), size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: f,
            // Slint takes a texture it can sample and also render to. COPY_SRC is for
            // explicit captures and checks (a screenshot, the composition compared with
            // compose()'s own bytes), never for the normal path.
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let (sw, sh) = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
        let bind = |label: &str, v: &wgpu::TextureView| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label), layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uni, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<PassU>() as u64) }) },
            ],
        });
        let slots = (0..SLOTS).map(|i| {
            let page = tex(&format!("office page {i}"), w, h, PAGE_FMT);
            let glass = tex(&format!("office glass {i}"), sw, sh, PAGE_FMT);
            let page_view = page.create_view(&Default::default());
            let glass_view = glass.create_view(&Default::default());
            let down = bind("page down", &page_view);
            Slot { page, glass, page_view, glass_view, down }
        }).collect();
        let ping = tex("office blur a", sw, sh, WORK_FMT);
        let pong = tex("office blur b", sw, sh, WORK_FMT);
        let ping_view = ping.create_view(&Default::default());
        let pong_view = pong.create_view(&Default::default());
        let g = Gpu {
            compose: pipe("page compose", "fs_compose", PAGE_FMT),
            down: pipe("page down", "fs_down", WORK_FMT),
            boxp: pipe("page box", "fs_box", WORK_FMT),
            sat: pipe("page saturate", "fs_sat", PAGE_FMT),
            scene: bind("page scene", scene),
            ping: bind("page blur a", &ping_view),
            pong: bind("page blur b", &pong_view),
            uni, slots, ping_view, pong_view, w, h, day,
        };
        g.write_uni(queue, day);
        g
    }

    /// The five passes' uniforms. Only the background's stops change after this (the time
    /// of day), so it is written again only then.
    fn write_uni(&self, queue: &wgpu::Queue, day: bool) {
        let stops = if day { ["#4a3530", "#241815", "#0e0a09"] } else { ["#2a1824", "#150c14", "#07050a"] };
        let bg = stops.map(|s| css(s).map(|v| v as f32));
        let bg_at = if day { [0.0, 0.6, 1.0, 0.0] } else { [0.0, 0.55, 1.0, 0.0] };
        let vig = [[0.0; 4], [0.0; 4], [0.0, 0.0, 0.0, 0.45]];
        let vig_at = [0.0, 0.6, 1.0, 0.0];
        let (sw, sh) = (self.w.div_ceil(4).max(1) as f32, self.h.div_ceil(4).max(1) as f32);
        let base = PassU { size: [self.w as f32, self.h as f32], dir: [0, 0], bg, bg_at, vig, vig_at };
        let small = PassU { size: [sw, sh], ..base };
        let passes = [
            base,
            small,
            PassU { dir: [1, 0], ..small },
            PassU { dir: [0, 1], ..small },
            small,
        ];
        for (i, p) in passes.iter().enumerate() {
            queue.write_buffer(&self.uni, i as u64 * SLICE, bytemuck::bytes_of(p));
        }
    }

    /// The time of day, when it changed: only the background's stops differ.
    pub fn sync(&mut self, queue: &wgpu::Queue, day: bool) {
        if self.day != day {
            self.day = day;
            self.write_uni(queue, day);
        }
    }

    pub fn size(&self) -> (u32, u32) { (self.w, self.h) }

    /// Each slot's two textures, for the app to hand to Slint.
    pub fn textures(&self) -> Vec<(wgpu::Texture, wgpu::Texture)> {
        self.slots.iter().map(|s| (s.page.clone(), s.glass.clone())).collect()
    }

    /// The page over the scene the renderer just drew, and the glass blur, into `slot`.
    /// Encoded into the caller's encoder, so it goes with the scene in one submission.
    pub fn encode(&self, enc: &mut wgpu::CommandEncoder, slot: usize) {
        let s = &self.slots[slot];
        // The background, the scene over it, the vignette over both.
        pass(enc, "page", &s.page_view, &self.compose, &self.scene, 0);
        // The glass blur, at a quarter of the size: three box passes each way, then
        // saturate, exactly as blur() does them on the CPU.
        pass(enc, "page blur down", &self.ping_view, &self.down, &s.down, 1);
        for _ in 0..3 {
            pass(enc, "page blur across", &self.pong_view, &self.boxp, &self.ping, 2);
            pass(enc, "page blur down", &self.ping_view, &self.boxp, &self.pong, 3);
        }
        pass(enc, "page glass", &s.glass_view, &self.sat, &self.ping, 4);
    }
}

/// One fullscreen pass. Every pixel of the target is written, so nothing is loaded.
fn pass(enc: &mut wgpu::CommandEncoder, label: &str, dst: &wgpu::TextureView, pipe: &wgpu::RenderPipeline, bind: &wgpu::BindGroup, slice: u32) {
    let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
    });
    p.set_pipeline(pipe);
    p.set_bind_group(0, bind, &[slice * SLICE as u32]);
    p.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    /// The byte tables give what the float ones did, pixel for pixel.
    #[test]
    fn the_byte_tables_compose_as_the_float_ones_did() {
        let (w, h) = (97, 41);
        let mut seed = 7u32;
        let frame: Vec<u8> = (0..w * h * 4).map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 24) as u8 }).collect();
        for day in [false, true] {
            // The float tables, as they were.
            let clear = super::compose(&vec![0u8; w * h * 4], w, h, day);
            let white = super::compose(&vec![255u8; w * h * 4], w, h, day);
            let black = super::compose(&[0u8, 0, 0, 255].repeat(w * h), w, h, day);
            let under: Vec<[f32; 3]> = clear.chunks(3).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect();
            let over: Vec<[f32; 4]> = white.chunks(3).zip(black.chunks(3)).map(|(a, b)| [(a[0] as f32 - b[0] as f32) / 255.0, b[0] as f32, b[1] as f32, b[2] as f32]).collect();
            let mut want = vec![];
            for (i, p) in frame.chunks(4).enumerate() {
                let a = p[3] as f32 / 255.0;
                for k in 0..3 { want.push((p[k] as f32 * over[i][0] + over[i][k + 1] * a + under[i][k] * (1.0 - a)).round().clamp(0.0, 255.0) as u8); }
            }
            assert!(white.iter().zip(&black).all(|(a, b)| a >= b));
            assert_eq!(super::Composer::default().compose(&frame, w, h, day), want);
        }
    }
}
