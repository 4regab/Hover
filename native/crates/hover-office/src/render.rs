//! WebGLRenderer.render(scene, camera), on wgpu: the shadow map (redrawn only when the
//! office says so), then opaque objects, then transparent ones back to front, into an
//! RGBA8 target that holds encoded colour as a WebGL canvas does. Offscreen: the app
//! reads the frame back and hands it to Slint.

use crate::m::{v3, M4, V3};
use crate::office::Office;
use crate::scene::{Blend, Geo, Mat, VBox};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex { pos: [f32; 3], normal: [f32; 3], color: [f32; 3], uv: [f32; 2] }

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameU {
    view_proj: [f32; 16], view: [f32; 16], proj: [f32; 16], shadow: [f32; 16],
    view_dir: [f32; 4], hemi_sky: [f32; 4], hemi_ground: [f32; 4], sun_dir: [f32; 4], sun: [f32; 4], fill_dir: [f32; 4], fill: [f32; 4],
    points: [[f32; 4]; 14], misc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawU { model: [f32; 16], normal: [f32; 16], color: [f32; 4], params: [f32; 4], flags: [f32; 4], _pad: [f32; 20] }

const SHADOW: u32 = 1536;
const DRAW_SIZE: u64 = 256;
const MAX_DRAWS: u64 = 2048;

struct Mesh { vb: wgpu::Buffer, ib: wgpu::Buffer, n: u32 }

fn box_verts(v: &mut Vec<Vertex>, ix: &mut Vec<u32>, b: &VBox) {
    // BoxGeometry's faces: +x, -x, +y, -y, +z, -z; each two triangles, counter-clockwise.
    let (x0, y0, z0, x1, y1, z1) = (b.x, b.y, b.z, b.x + b.w, b.y + b.h, b.z + b.d);
    let c = b.c.f32();
    let faces: [([f64; 3], [[f64; 3]; 4]); 6] = [
        ([1., 0., 0.], [[x1, y1, z1], [x1, y1, z0], [x1, y0, z1], [x1, y0, z0]]),
        ([-1., 0., 0.], [[x0, y1, z0], [x0, y1, z1], [x0, y0, z0], [x0, y0, z1]]),
        ([0., 1., 0.], [[x0, y1, z0], [x1, y1, z0], [x0, y1, z1], [x1, y1, z1]]),
        ([0., -1., 0.], [[x0, y0, z1], [x1, y0, z1], [x0, y0, z0], [x1, y0, z0]]),
        ([0., 0., 1.], [[x0, y1, z1], [x1, y1, z1], [x0, y0, z1], [x1, y0, z1]]),
        ([0., 0., -1.], [[x1, y1, z0], [x0, y1, z0], [x1, y0, z0], [x0, y0, z0]]),
    ];
    for (n, p) in faces {
        let base = v.len() as u32;
        for (k, q) in p.iter().enumerate() {
            v.push(Vertex { pos: q.map(|x| x as f32), normal: n.map(|x| x as f32), color: c, uv: [(k % 2) as f32, 1.0 - (k / 2) as f32] });
        }
        ix.extend([base, base + 2, base + 1, base + 2, base + 3, base + 1]);
    }
}

fn geometry(geo: &Geo, merged: &[Vec<VBox>]) -> (Vec<Vertex>, Vec<u32>) {
    let (mut v, mut ix) = (vec![], vec![]);
    let w = crate::m::Rgb(1.0, 1.0, 1.0);
    match geo {
        Geo::Unit => box_verts(&mut v, &mut ix, &VBox { x: -0.5, y: -0.5, z: -0.5, w: 1.0, h: 1.0, d: 1.0, c: w }),
        Geo::Merged(i) => for b in &merged[*i] { box_verts(&mut v, &mut ix, b) },
        Geo::Cylinder { top, bottom, h, seg } => {
            let n = *seg;
            let slope = (bottom - top) / h;
            for y in 0..2 {
                let r = if y == 0 { *top } else { *bottom };
                for k in 0..=n {
                    let t = k as f64 / n as f64 * std::f64::consts::TAU;
                    let nn = v3(t.sin(), slope, t.cos()).norm();
                    v.push(Vertex { pos: [(r * t.sin()) as f32, (h / 2.0 * if y == 0 { 1.0 } else { -1.0 }) as f32, (r * t.cos()) as f32], normal: [nn.x as f32, nn.y as f32, nn.z as f32], color: [1.0; 3], uv: [0.0; 2] });
                }
            }
            for k in 0..n { let (a, b, c, d) = (k, k + n + 1, k + n + 2, k + 1); ix.extend([a, b, d, b, c, d]); }
            for (top_cap, r, y) in [(true, *top, h / 2.0), (false, *bottom, -h / 2.0)] {
                let centre = v.len() as u32;
                let ny = if top_cap { 1.0 } else { -1.0 };
                v.push(Vertex { pos: [0.0, y as f32, 0.0], normal: [0.0, ny, 0.0], color: [1.0; 3], uv: [0.0; 2] });
                for k in 0..=n {
                    let t = k as f64 / n as f64 * std::f64::consts::TAU;
                    v.push(Vertex { pos: [(r * t.sin()) as f32, y as f32, (r * t.cos()) as f32], normal: [0.0, ny, 0.0], color: [1.0; 3], uv: [0.0; 2] });
                }
                for k in 0..n { if top_cap { ix.extend([centre, centre + 1 + k, centre + 2 + k]); } else { ix.extend([centre, centre + 2 + k, centre + 1 + k]); } }
            }
        }
        Geo::Ring { inner, outer, seg } => {
            for k in 0..=*seg {
                let t = k as f64 / *seg as f64 * std::f64::consts::TAU;
                for r in [*inner, *outer] { v.push(Vertex { pos: [(r * t.cos()) as f32, (r * t.sin()) as f32, 0.0], normal: [0.0, 0.0, 1.0], color: [1.0; 3], uv: [0.0; 2] }); }
            }
            for k in 0..*seg { let a = k * 2; ix.extend([a, a + 1, a + 3, a, a + 3, a + 2]); }
        }
        Geo::Plane { .. } | Geo::Sprite => {
            let (pw, ph) = if let Geo::Plane { w, h } = geo { (*w, *h) } else { (1.0, 1.0) };
            for (k, (x, y)) in [(-0.5, 0.5), (0.5, 0.5), (-0.5, -0.5), (0.5, -0.5)].iter().enumerate() {
                v.push(Vertex { pos: [(x * pw) as f32, (y * ph) as f32, 0.0], normal: [0.0, 0.0, 1.0], color: [1.0; 3], uv: [(k % 2) as f32, (k / 2) as f32] });
            }
            ix.extend([0, 2, 1, 2, 3, 1]);
        }
        Geo::Quad(p) => {
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            for (k, q) in p.iter().enumerate() { v.push(Vertex { pos: [q.x as f32, q.y as f32, q.z as f32], normal: [0.0, 1.0, 0.0], color: [1.0; 3], uv: uv[k] }); }
            ix.extend([0, 3, 1, 1, 3, 2]);
        }
        Geo::Points(p) => { for (k, q) in p.iter().enumerate() { v.push(Vertex { pos: [q.x as f32, q.y as f32, q.z as f32], normal: [0.0; 3], color: [1.0; 3], uv: [0.0; 2] }); ix.push(k as u32); } }
    }
    (v, ix)
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub w: u32, pub h: u32,
    color: wgpu::Texture, depth: wgpu::Texture, shadow: wgpu::Texture,
    frame_buf: wgpu::Buffer, draw_buf: wgpu::Buffer,
    g0: wgpu::BindGroup, g0_shadow: wgpu::BindGroup, g1: wgpu::BindGroup, g1_layout: wgpu::BindGroupLayout,
    tex_groups: Vec<wgpu::BindGroup>, textures: Vec<wgpu::Texture>,
    pipes: HashMap<(u8, u8, bool, bool), wgpu::RenderPipeline>,
    shadow_pipe: wgpu::RenderPipeline,
    meshes: HashMap<String, Mesh>,
    readback: wgpu::Buffer,
    pub adapter_name: String,
}

const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The device the app's windows draw with, when the app shares it (Windows). Each GPU
/// device costs tens of MB of its own (driver state, descriptor heaps), and a device of
/// the office's own also woke Vulkan and OpenGL on every graphics card.
static SHARED: std::sync::OnceLock<(wgpu::Device, wgpu::Queue, String)> = std::sync::OnceLock::new();

pub fn share_device(device: wgpu::Device, queue: wgpu::Queue, adapter_name: String) {
    let _ = SHARED.set((device, queue, adapter_name));
}

/// Frees what was dropped on the shared device. wgpu does that only when the device is
/// polled, and a resting notch draws nothing that would poll it.
pub fn flush_shared() {
    if let Some((d, _, _)) = SHARED.get() { let _ = d.poll(wgpu::PollType::wait_indefinitely()); }
}

impl Renderer {
    /// The app's shared device when there is one; else a device of its own (Vulkan or GL
    /// on Linux, DX12 on Windows), low power.
    pub fn new(w: u32, h: u32) -> Result<Renderer, String> {
        let (device, queue, adapter_name) = match SHARED.get() {
            Some((d, q, n)) => (d.clone(), q.clone(), n.clone()),
            None => {
                let instance = wgpu::Instance::new({ let mut d = wgpu::InstanceDescriptor::new_without_display_handle(); d.backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL; d });
                let adapter = futures_lite::future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::LowPower, ..Default::default() }))
                    .map_err(|e| format!("no GPU adapter: {e}"))?;
                let adapter_name = format!("{} ({:?})", adapter.get_info().name, adapter.get_info().backend);
                let (device, queue) = futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("office"), ..Default::default() })).map_err(|e| e.to_string())?;
                (device, queue, adapter_name)
            }
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("office"), source: wgpu::ShaderSource::Wgsl(include_str!("office.wgsl").into()) });
        let tex = |d: &wgpu::Device, w: u32, h: u32, f: wgpu::TextureFormat, u: wgpu::TextureUsages| d.create_texture(&wgpu::TextureDescriptor {
            label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: f, usage: u, view_formats: &[] });
        let color = tex(&device, w, h, FMT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let depth = tex(&device, w, h, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let shadow = tex(&device, SHADOW, SHADOW, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<FrameU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let draw_buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: DRAW_SIZE * MAX_DRAWS, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let vis = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let g0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: vis, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        ] });
        let g1_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(DRAW_SIZE) }, count: None },
        ] });
        let g2_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: vis, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: vis, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ] });
        let sv = shadow.create_view(&Default::default());
        let g0 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g0_layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sv) },
        ] });
        // The shadow pass writes the map, so it can't also be bound for reading there.
        let stand_in = tex(&device, 1, 1, DEPTH, wgpu::TextureUsages::TEXTURE_BINDING);
        let siv = stand_in.create_view(&Default::default());
        let g0_shadow = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g0_layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&siv) },
        ] });
        let g1 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g1_layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &draw_buf, offset: 0, size: wgpu::BufferSize::new(DRAW_SIZE) }) },
        ] });
        // The canvases (sky, TV, board, clock: sRGB, nearest when magnified), beam and patch
        // (data, linear), the glow sprite's radial texture, and white for untextured draws.
        let sizes = [(128, 96), (208, 118), (480, 280), (96, 44), (4, 64), (64, 64), (64, 64), (1, 1)];
        let (mut textures, mut tex_groups) = (vec![], vec![]);
        for (i, &(tw, th)) in sizes.iter().enumerate() {
            let f = if i < 4 { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
            let t = tex(&device, tw, th, f, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            let s = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: if i < 4 { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear }, min_filter: wgpu::FilterMode::Linear,
                address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });
            let v = t.create_view(&Default::default());
            tex_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g2_layout, entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&v) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&s) },
            ] }));
            textures.push(t);
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&g0_layout), Some(&g1_layout), Some(&g2_layout)], immediate_size: 0 });
        let vbl = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x2] };
        let mut pipes = HashMap::new();
        // (blend 0 opaque / 1 normal / 2 additive, topology 0 triangles / 1 points, depth write, double sided)
        for blend in 0..3u8 { for topo in 0..2u8 { for dw in [false, true] { for double in [false, true] {
            let b = match blend {
                0 => None,
                1 => Some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                    alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add } }),
                _ => Some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add } }),
            };
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None, layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[Some(vbl.clone())], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: FMT, blend: b, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: wgpu::PrimitiveState { topology: if topo == 1 { wgpu::PrimitiveTopology::PointList } else { wgpu::PrimitiveTopology::TriangleList },
                    cull_mode: if double || topo == 1 { None } else { Some(wgpu::Face::Back) }, front_face: wgpu::FrontFace::Ccw, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(dw), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(), multiview_mask: None, cache: None,
            });
            pipes.insert((blend, topo, dw, double), p);
        } } } }
        // Shadows: back faces into the depth map (three's shadowSide for FrontSide materials).
        let shadow_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"), layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_shadow"), buffers: &[Some(vbl)], compilation_options: Default::default() },
            fragment: None,
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Front), front_face: wgpu::FrontFace::Ccw, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (align(w * 4) * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let r = Renderer { device, queue, w, h, color, depth, shadow, frame_buf, draw_buf, g0, g0_shadow, g1, g1_layout, tex_groups, textures, pipes, shadow_pipe, meshes: HashMap::new(), readback, adapter_name };
        // glowTex: white, alpha from 1 at the centre through .4 at 35 % to 0 at the edge.
        let mut gc = crate::canvas::Canvas::new(64, 64);
        gc.gradient_r(32.0, 32.0, 32.0, &[(0.0, [1.0, 1.0, 1.0, 1.0]), (0.35, [1.0, 1.0, 1.0, 0.4]), (1.0, [1.0, 1.0, 1.0, 0.0])]);
        r.upload(6, 64, 64, &gc.rgba());
        r.upload(7, 1, 1, &[255, 255, 255, 255]);
        Ok(r)
    }

    fn upload(&self, i: usize, w: u32, h: u32, rgba: &[u8]) {
        self.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &self.textures[i], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    }

    /// A new size: the frame buffers are made again.
    pub fn resize(&mut self, w: u32, h: u32) {
        if (w, h) == (self.w, self.h) || w == 0 || h == 0 { return; }
        let d = &self.device;
        let tex = |f: wgpu::TextureFormat, u: wgpu::TextureUsages| d.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: f, usage: u, view_formats: &[] });
        self.color = tex(FMT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        self.depth = tex(DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
        self.readback = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (align(w * 4) * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        self.w = w;
        self.h = h;
    }

    fn mesh(&mut self, key: String, geo: &Geo, merged: &[Vec<VBox>]) -> &Mesh {
        use wgpu::util::DeviceExt;
        let d = &self.device;
        self.meshes.entry(key).or_insert_with(|| {
            let (v, ix) = geometry(geo, merged);
            let v = if v.is_empty() { vec![Vertex::zeroed()] } else { v };
            let ix = if ix.is_empty() { vec![0u32] } else { ix };
            Mesh { vb: d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&v), usage: wgpu::BufferUsages::VERTEX }),
                ib: d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&ix), usage: wgpu::BufferUsages::INDEX }), n: ix.len() as u32 }
        })
    }

    /// Renders the office into the frame and reads it back: RGBA8, premultiplied (the
    /// canvas's own alpha), top row first.
    pub fn render(&mut self, o: &mut Office) -> Vec<u8> {
        for (i, c) in o.canvases.iter().enumerate() {
            if o.dirty[i] { self.upload(i, c.w as u32, c.h as u32, &c.rgba()); o.dirty[i] = false; }
        }
        let (view, proj) = o.camera();
        let lights = o.lights();
        let shadow_proj = M4::ortho(-12.0, 12.0, 12.0, -12.0, 1.0, 40.0);
        let shadow_vp = shadow_proj.mul(&lights.sun_view);
        let iso = v3(1.0, 0.86, 1.0).norm();
        let mut pts = [[0f32; 4]; 14];
        for (k, (p, c, dist, decay)) in lights.points.iter().enumerate().take(7) {
            pts[k * 2] = [p.x as f32, p.y as f32, p.z as f32, *dist as f32];
            pts[k * 2 + 1] = [c.0 as f32, c.1 as f32, c.2 as f32, *decay as f32];
        }
        let v4 = |v: V3| [v.x as f32, v.y as f32, v.z as f32, 0.0];
        let c4 = |c: crate::m::Rgb| [c.0 as f32, c.1 as f32, c.2 as f32, 0.0];
        let fu = FrameU {
            view_proj: proj.mul(&view).f32(), view: view.f32(), proj: proj.f32(), shadow: shadow_vp.f32(),
            view_dir: v4(iso), hemi_sky: c4(lights.hemi_sky), hemi_ground: c4(lights.hemi_ground), sun_dir: v4(lights.sun_dir), sun: c4(lights.sun),
            fill_dir: v4(lights.fill_dir), fill: c4(lights.fill), points: pts, misc: [lights.exposure as f32, SHADOW as f32, -0.0004, 0.03],
        };
        self.queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&fu));

        // Draw list: what is shown, opaque first, then transparent back to front.
        let world = o.g.world();
        let shown = o.g.shown();
        let vp = proj.mul(&view);
        struct D { node: usize, depth: f64, trans: bool }
        let mut list: Vec<D> = vec![];
        for (i, n) in o.g.nodes.iter().enumerate() {
            let Some((_, m)) = &n.draw else { continue };
            if !shown[i] { continue; }
            if let Mat::Basic { opacity, .. } | Mat::Glow { opacity, .. } | Mat::Points { opacity, .. } = m { if *opacity <= 0.0 && m.transparent() { continue; } }
            let c = vp.point(world[i].point(V3::default()));
            list.push(D { node: i, depth: c.z, trans: m.transparent() });
        }
        list.sort_by(|a, b| a.trans.cmp(&b.trans).then(if a.trans { b.depth.total_cmp(&a.depth) } else { std::cmp::Ordering::Equal }));
        let mut draws: Vec<DrawU> = vec![];
        let merged = o.g.merged.clone();
        for d in &list {
            let n = &o.g.nodes[d.node];
            let (geo, m) = n.draw.as_ref().unwrap();
            let model = world[d.node];
            let nm = model.inverse();
            // The normal matrix: the inverse transpose.
            let mut t = [0f64; 16];
            for c in 0..4 { for r in 0..4 { t[c * 4 + r] = nm.0[r * 4 + c]; } }
            let (color, params, flags) = match m {
                Mat::Std { color, rough, metal, vertex } => ([color.0, color.1, color.2, 1.0], [0.0, *rough, *metal, 1.0], [f64::from(*vertex as u8), f64::from(n.receive as u8), 0.0, 0.0]),
                Mat::Basic { color, tone, opacity, tex, .. } => ([color.0, color.1, color.2, *opacity], [if tex.is_some() { 2.0 } else { 1.0 }, 0.0, 0.0, f64::from(*tone as u8)], [0.0; 4]),
                Mat::Glow { color, opacity } => ([color.0, color.1, color.2, *opacity], [3.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]),
                Mat::Points { color, opacity } => ([color.0, color.1, color.2, *opacity], [4.0, 0.0, 0.0, 1.0], [0.0; 4]),
            };
            draws.push(DrawU { model: model.f32(), normal: M4(t).f32(), color: color.map(|x| x as f32), params: params.map(|x| x as f32), flags: flags.map(|x| x as f32), _pad: [0.0; 20] });
            let key = match geo { Geo::Merged(i) => format!("m{i}"), Geo::Quad(_) | Geo::Points(_) => format!("n{}", d.node), other => format!("{other:?}") };
            self.mesh(key, geo, &merged);
        }
        let bytes: Vec<u8> = draws.iter().flat_map(|d| bytemuck::bytes_of(d).to_vec()).collect();
        self.queue.write_buffer(&self.draw_buf, 0, &bytes);

        let mut enc = self.device.create_command_encoder(&Default::default());
        if o.shadow_dirty {
            o.shadow_dirty = false;
            let sv = self.shadow.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("shadow"), color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &sv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            pass.set_pipeline(&self.shadow_pipe);
            pass.set_bind_group(0, &self.g0_shadow, &[]);
            pass.set_bind_group(2, &self.tex_groups[7], &[]);
            for (k, d) in list.iter().enumerate() {
                let n = &o.g.nodes[d.node];
                let (geo, m) = n.draw.as_ref().unwrap();
                if !n.cast || !matches!(m, Mat::Std { .. }) { continue; }
                let key = match geo { Geo::Merged(i) => format!("m{i}"), other => format!("{other:?}") };
                let mesh = &self.meshes[&key];
                pass.set_bind_group(1, &self.g1, &[(k as u64 * DRAW_SIZE) as u32]);
                pass.set_vertex_buffer(0, mesh.vb.slice(..));
                pass.set_index_buffer(mesh.ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.n, 0, 0..1);
            }
        }
        {
            let cv = self.color.create_view(&Default::default());
            let dv = self.depth.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("office"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &cv, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            pass.set_bind_group(0, &self.g0, &[]);
            for (k, d) in list.iter().enumerate() {
                let n = &o.g.nodes[d.node];
                let (geo, m) = n.draw.as_ref().unwrap();
                let (blend, dw, double, tex) = match m {
                    Mat::Std { .. } => (0u8, true, false, 7),
                    Mat::Basic { blend, depth_write, double, tex, .. } => (if m.transparent() { if *blend == Blend::Additive { 2 } else { 1 } } else { 0 }, *depth_write, *double, tex.unwrap_or(7)),
                    Mat::Glow { .. } => (2, false, false, 6),
                    Mat::Points { .. } => (2, false, false, 7),
                };
                let topo = u8::from(matches!(geo, Geo::Points(_)));
                let key = match geo { Geo::Merged(i) => format!("m{i}"), Geo::Quad(_) | Geo::Points(_) => format!("n{}", d.node), other => format!("{other:?}") };
                let mesh = &self.meshes[&key];
                pass.set_pipeline(&self.pipes[&(blend, topo, dw, double)]);
                pass.set_bind_group(1, &self.g1, &[(k as u64 * DRAW_SIZE) as u32]);
                pass.set_bind_group(2, &self.tex_groups[tex], &[]);
                pass.set_vertex_buffer(0, mesh.vb.slice(..));
                pass.set_index_buffer(mesh.ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.n, 0, 0..1);
            }
        }
        let row = align(self.w * 4);
        enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &self.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &self.readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(self.h) } },
            wgpu::Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 });
        self.queue.submit([enc.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("the frame maps");
        let mut out = Vec::with_capacity((self.w * self.h * 4) as usize);
        for y in 0..self.h as usize { out.extend_from_slice(&data[y * row as usize..y * row as usize + self.w as usize * 4]); }
        drop(data);
        self.readback.unmap();
        let _ = &self.g1_layout;
        out
    }
}

fn align(n: u32) -> u32 { n.div_ceil(256) * 256 }

/// The frame over the page's background (#office), as the eye sees it: straight RGB.
pub fn over(rgba: &[u8], bg: [u8; 3]) -> Vec<u8> {
    let mut o = Vec::with_capacity(rgba.len() / 4 * 3);
    for p in rgba.chunks(4) {
        let a = p[3] as u32;
        for c in 0..3 { o.push((p[c] as u32 + bg[c] as u32 * (255 - a) / 255).min(255) as u8); }
    }
    o
}
