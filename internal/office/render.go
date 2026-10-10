package office

// render.rs: WebGLRenderer.render(scene, camera), on wgpu: the shadow map (redrawn only
// when the office says so), then opaque objects, then transparent ones back to front,
// into an RGBA8 target that holds encoded colour as a WebGL canvas does. Offscreen: the
// frame is read back and composed on the CPU (page.go).
//
// ponytail: the Rust renderer can also share the app's wgpu device and keep the frame on
// the GPU (Windows). Gio draws with a device of its own, so here the frame is always read
// back: render.rs's CPU path, which it takes on Linux.

import (
	_ "embed"
	"errors"
	"fmt"
	"math"
	"sort"
	"strings"
	"unsafe"

	"github.com/4regab/Hover/internal/gpu"
)

//go:embed office.wgsl
var officeWGSL string

type vertex struct {
	pos, normal, color [3]float32
	uv                 [2]float32
}

type frameU struct {
	viewProj, view, proj, shadow                             [16]float32
	viewDir, hemiSky, hemiGround, sunDir, sun, fillDir, fill [4]float32
	points                                                   [14][4]float32
	misc                                                     [4]float32
}

type drawU struct {
	model, normal        [16]float32
	color, params, flags [4]float32
	_                    [20]float32
}

const (
	shadowSize = 1536
	drawSize   = 256
	maxDraws   = 2048
	colorFmt   = gpu.FormatRGBA8Unorm
	depthFmt   = gpu.FormatDepth32Float
)

type mesh struct {
	vb, ib gpu.Buffer
	n      uint32
}

// bytesOf is a slice's memory as bytes (the GPU's little-endian floats and ints).
func bytesOf[T any](s []T) []byte {
	if len(s) == 0 {
		return nil
	}
	return unsafe.Slice((*byte)(unsafe.Pointer(&s[0])), len(s)*int(unsafe.Sizeof(s[0])))
}

func f3(x, y, z float64) [3]float32 { return [3]float32{float32(x), float32(y), float32(z)} }

func boxVerts(v *[]vertex, ix *[]uint32, b VBox) {
	// BoxGeometry's faces: +x, -x, +y, -y, +z, -z; each two triangles, counter-clockwise.
	x0, y0, z0, x1, y1, z1 := b.X, b.Y, b.Z, b.X+b.W, b.Y+b.H, b.Z+b.D
	c := b.C.F32()
	faces := [6]struct {
		n [3]float64
		p [4][3]float64
	}{
		{[3]float64{1, 0, 0}, [4][3]float64{{x1, y1, z1}, {x1, y1, z0}, {x1, y0, z1}, {x1, y0, z0}}},
		{[3]float64{-1, 0, 0}, [4][3]float64{{x0, y1, z0}, {x0, y1, z1}, {x0, y0, z0}, {x0, y0, z1}}},
		{[3]float64{0, 1, 0}, [4][3]float64{{x0, y1, z0}, {x1, y1, z0}, {x0, y1, z1}, {x1, y1, z1}}},
		{[3]float64{0, -1, 0}, [4][3]float64{{x0, y0, z1}, {x1, y0, z1}, {x0, y0, z0}, {x1, y0, z0}}},
		{[3]float64{0, 0, 1}, [4][3]float64{{x0, y1, z1}, {x1, y1, z1}, {x0, y0, z1}, {x1, y0, z1}}},
		{[3]float64{0, 0, -1}, [4][3]float64{{x1, y1, z0}, {x0, y1, z0}, {x1, y0, z0}, {x0, y0, z0}}},
	}
	for _, f := range faces {
		base := uint32(len(*v))
		for k, q := range f.p {
			*v = append(*v, vertex{pos: f3(q[0], q[1], q[2]), normal: f3(f.n[0], f.n[1], f.n[2]), color: c,
				uv: [2]float32{float32(k % 2), 1 - float32(k/2)}})
		}
		*ix = append(*ix, base, base+2, base+1, base+2, base+3, base+1)
	}
}

func geometry(geo Geo, merged [][]VBox) ([]vertex, []uint32) {
	var v []vertex
	var ix []uint32
	white := [3]float32{1, 1, 1}
	switch geo.Kind {
	case GeoUnit:
		boxVerts(&v, &ix, VBox{X: -0.5, Y: -0.5, Z: -0.5, W: 1, H: 1, D: 1, C: Rgb{1, 1, 1}})
	case GeoMerged:
		for _, b := range merged[geo.Index] {
			boxVerts(&v, &ix, b)
		}
	case GeoCylinder:
		n := uint32(geo.Seg)
		slope := (geo.Bottom - geo.Top) / geo.H
		for y := 0; y < 2; y++ {
			r, sy := geo.Top, 1.0
			if y == 1 {
				r, sy = geo.Bottom, -1
			}
			for k := uint32(0); k <= n; k++ {
				t := float64(k) / float64(n) * 2 * math.Pi
				nn := v3(math.Sin(t), slope, math.Cos(t)).Norm()
				v = append(v, vertex{pos: f3(r*math.Sin(t), geo.H/2*sy, r*math.Cos(t)), normal: f3(nn.X, nn.Y, nn.Z), color: white})
			}
		}
		for k := uint32(0); k < n; k++ {
			a, b, c, d := k, k+n+1, k+n+2, k+1
			ix = append(ix, a, b, d, b, c, d)
		}
		for _, cp := range [2]struct {
			top  bool
			r, y float64
		}{{true, geo.Top, geo.H / 2}, {false, geo.Bottom, -geo.H / 2}} {
			centre := uint32(len(v))
			ny := -1.0
			if cp.top {
				ny = 1
			}
			v = append(v, vertex{pos: f3(0, cp.y, 0), normal: f3(0, ny, 0), color: white})
			for k := uint32(0); k <= n; k++ {
				t := float64(k) / float64(n) * 2 * math.Pi
				v = append(v, vertex{pos: f3(cp.r*math.Sin(t), cp.y, cp.r*math.Cos(t)), normal: f3(0, ny, 0), color: white})
			}
			for k := uint32(0); k < n; k++ {
				if cp.top {
					ix = append(ix, centre, centre+1+k, centre+2+k)
				} else {
					ix = append(ix, centre, centre+2+k, centre+1+k)
				}
			}
		}
	case GeoRing:
		seg := uint32(geo.Seg)
		for k := uint32(0); k <= seg; k++ {
			t := float64(k) / float64(seg) * 2 * math.Pi
			for _, r := range [2]float64{geo.Inner, geo.Outer} {
				v = append(v, vertex{pos: f3(r*math.Cos(t), r*math.Sin(t), 0), normal: f3(0, 0, 1), color: white})
			}
		}
		for k := uint32(0); k < seg; k++ {
			a := k * 2
			ix = append(ix, a, a+1, a+3, a, a+3, a+2)
		}
	case GeoPlane, GeoSprite:
		pw, ph := 1.0, 1.0
		if geo.Kind == GeoPlane {
			pw, ph = geo.W, geo.H
		}
		for k, p := range [4][2]float64{{-0.5, 0.5}, {0.5, 0.5}, {-0.5, -0.5}, {0.5, -0.5}} {
			v = append(v, vertex{pos: f3(p[0]*pw, p[1]*ph, 0), normal: f3(0, 0, 1), color: white, uv: [2]float32{float32(k % 2), float32(k / 2)}})
		}
		ix = append(ix, 0, 2, 1, 2, 3, 1)
	case GeoQuad:
		uv := [4][2]float32{{0, 0}, {1, 0}, {1, 1}, {0, 1}}
		for k, q := range geo.Quad {
			v = append(v, vertex{pos: f3(q.X, q.Y, q.Z), normal: f3(0, 1, 0), color: white, uv: uv[k]})
		}
		ix = append(ix, 0, 3, 1, 1, 3, 2)
	case GeoPoints:
		for k, q := range geo.Pts {
			v = append(v, vertex{pos: f3(q.X, q.Y, q.Z), color: white})
			ix = append(ix, uint32(k))
		}
	}
	return v, ix
}

// pipeKey is a draw's pipeline kind: blend 0 opaque / 1 normal / 2 additive, topology 0
// triangles / 1 points, depth write, double sided.
type pipeKey struct {
	blend, topo uint8
	dw, double  bool
}

// Renderer draws the office offscreen on a wgpu device of its own.
type Renderer struct {
	inst    gpu.Instance
	adapter gpu.Adapter
	dev     *gpu.Device
	W, H    uint32

	color, depth         gpu.Texture
	colorView, depthView gpu.TextureView
	shadowView           gpu.TextureView
	frameBuf, drawBuf    gpu.Buffer
	g0, g0Shadow, g1     gpu.BindGroup
	texGroups            []gpu.BindGroup
	textures             []gpu.Texture
	pipes                map[pipeKey]gpu.RenderPipeline
	shadowPipe           gpu.RenderPipeline
	shader               gpu.ShaderModule
	layout               gpu.PipelineLayout
	meshes               map[string]mesh
	readback             gpu.Buffer
	// keys is each node's mesh name, by node (a node's geometry never changes once made).
	keys []string
	// draws is the frame's per-draw uniforms, kept for the next frame.
	draws []drawU
	// free is everything made once, released by Close.
	free []func()

	AdapterName string
	// Software: the adapter is the CPU (WARP, llvmpipe): every pixel costs CPU time, so
	// the office draws fewer of them (live.go).
	Software bool
}

// backendName is wgpu's own name for a backend, as the Rust office logs it.
func backendName(b gpu.BackendType) string {
	switch b {
	case gpu.BackendVulkan:
		return "Vulkan"
	case gpu.BackendMetal:
		return "Metal"
	case gpu.BackendD3D12:
		return "Dx12"
	case gpu.BackendOpenGL, gpu.BackendOpenGLES:
		return "Gl"
	}
	return "Noop"
}

func align(n uint32) uint32 { return (n + 255) / 256 * 256 }

// NewRenderer makes a device of its own (Vulkan or GL on Linux, DX12 on Windows, Metal
// on macOS), low power.
func NewRenderer(w, h uint32) (*Renderer, error) {
	inst, err := gpu.CreateInstance()
	if err != nil {
		return nil, err
	}
	adapter, err := inst.RequestAdapter(false)
	if err != nil {
		inst.Release()
		return nil, fmt.Errorf("no GPU adapter: %w", err)
	}
	info := adapter.Info()
	dev, err := inst.RequestDevice(adapter, "office")
	if err != nil {
		adapter.Release()
		inst.Release()
		return nil, err
	}
	r := &Renderer{inst: inst, adapter: adapter, dev: dev, W: w, H: h,
		pipes: map[pipeKey]gpu.RenderPipeline{}, meshes: map[string]mesh{},
		AdapterName: fmt.Sprintf("%s (%s)", info.Name, backendName(info.Backend)), Software: info.Type == gpu.AdapterCPU}
	keep := func(f func()) { r.free = append(r.free, f) }

	r.shader = dev.ShaderModule("office", officeWGSL)
	keep(r.shader.Release)
	r.frameTargets(w, h)
	shadow := dev.Texture(gpu.TextureDesc{W: shadowSize, H: shadowSize, Format: depthFmt, Usage: gpu.TextureRenderAttachmt | gpu.TextureBinding})
	keep(shadow.Release)
	r.shadowView = shadow.View()
	keep(r.shadowView.Release)
	r.frameBuf = dev.Buffer(gpu.BufferUniform|gpu.BufferCopyDst, uint64(unsafe.Sizeof(frameU{})))
	keep(r.frameBuf.Release)
	r.drawBuf = dev.Buffer(gpu.BufferUniform|gpu.BufferCopyDst, drawSize*maxDraws)
	keep(r.drawBuf.Release)
	vis := gpu.StageVertex | gpu.StageFragment
	g0Layout := dev.BindGroupLayout(
		gpu.LayoutEntry{Binding: 0, Visibility: vis, Uniform: true},
		gpu.LayoutEntry{Binding: 1, Visibility: vis, Texture: gpu.SampleDepth})
	keep(g0Layout.Release)
	g1Layout := dev.BindGroupLayout(gpu.LayoutEntry{Binding: 0, Visibility: vis, Uniform: true, Dynamic: true, MinSize: drawSize})
	keep(g1Layout.Release)
	g2Layout := dev.BindGroupLayout(
		gpu.LayoutEntry{Binding: 0, Visibility: vis, Texture: gpu.SampleFloat},
		gpu.LayoutEntry{Binding: 1, Visibility: vis, Sampler: true})
	keep(g2Layout.Release)
	r.g0 = dev.BindGroup(g0Layout, gpu.GroupEntry{Binding: 0, Buffer: r.frameBuf}, gpu.GroupEntry{Binding: 1, View: r.shadowView})
	keep(r.g0.Release)
	// The shadow pass writes the map, so it can't also be bound for reading there.
	standIn := dev.Texture(gpu.TextureDesc{W: 1, H: 1, Format: depthFmt, Usage: gpu.TextureBinding})
	keep(standIn.Release)
	siv := standIn.View()
	keep(siv.Release)
	r.g0Shadow = dev.BindGroup(g0Layout, gpu.GroupEntry{Binding: 0, Buffer: r.frameBuf}, gpu.GroupEntry{Binding: 1, View: siv})
	keep(r.g0Shadow.Release)
	r.g1 = dev.BindGroup(g1Layout, gpu.GroupEntry{Binding: 0, Buffer: r.drawBuf, Size: drawSize})
	keep(r.g1.Release)
	// The canvases (sky, TV, board, clock: sRGB, nearest when magnified), beam and patch
	// (data, linear), the glow sprite's radial texture, and white for untextured draws.
	sizes := [8][2]uint32{{128, 96}, {208, 118}, {480, 280}, {96, 44}, {4, 64}, {64, 64}, {64, 64}, {1, 1}}
	for i, s := range sizes {
		f, mag := gpu.FormatRGBA8UnormSrgb, gpu.FilterNearest
		if i >= 4 {
			f, mag = gpu.FormatRGBA8Unorm, gpu.FilterLinear
		}
		t := dev.Texture(gpu.TextureDesc{W: s[0], H: s[1], Format: f, Usage: gpu.TextureBinding | gpu.TextureCopyDst})
		keep(t.Release)
		smp := dev.Sampler(gpu.SamplerDesc{Mag: mag, Min: gpu.FilterLinear, AddressU: gpu.ClampToEdge, AddressV: gpu.ClampToEdge})
		keep(smp.Release)
		v := t.View()
		keep(v.Release)
		g := dev.BindGroup(g2Layout, gpu.GroupEntry{Binding: 0, View: v}, gpu.GroupEntry{Binding: 1, Sampler: smp})
		keep(g.Release)
		r.texGroups = append(r.texGroups, g)
		r.textures = append(r.textures, t)
	}
	r.layout = dev.PipelineLayout(g0Layout, g1Layout, g2Layout)
	keep(r.layout.Release)
	// The draw pipelines are made on first use: of the 24 (blend, topology, depth write,
	// sides) kinds only a few are drawn, and each one the GPU driver compiles keeps
	// memory of its own.
	// Shadows: back faces into the depth map (three's shadowSide for FrontSide materials).
	r.shadowPipe = dev.RenderPipeline(gpu.PipelineDesc{
		Label: "shadow", Layout: r.layout, Module: r.shader, VS: "vs_shadow",
		Buffers: []gpu.VertexLayout{vertexLayout}, Topology: gpu.TriangleList, Front: gpu.FrontCCW, Cull: gpu.CullFront,
		Depth: &gpu.DepthState{Format: depthFmt, Write: true, Compare: gpu.CompareLessEqual},
	})
	keep(r.shadowPipe.Release)
	// glowTex: white, alpha from 1 at the centre through .4 at 35 % to 0 at the edge.
	gc := NewCanvas(64, 64, nil)
	gc.GradientR(32, 32, 32, []stop4{{0, [4]float64{1, 1, 1, 1}}, {0.35, [4]float64{1, 1, 1, 0.4}}, {1, [4]float64{1, 1, 1, 0}}})
	r.upload(6, 64, 64, gc.RGBA())
	r.upload(7, 1, 1, []byte{255, 255, 255, 255})
	// wgpu-native reports a broken shader or pipeline to the device, not to the call.
	if e := dev.Errors(); len(e) > 0 {
		r.Close()
		return nil, errors.New("office: " + strings.Join(e, "; "))
	}
	return r, nil
}

var vertexLayout = gpu.VertexLayout{Stride: uint64(unsafe.Sizeof(vertex{})), Attrs: []gpu.VertexAttr{
	{Format: gpu.VertexFloat32x3, Offset: 0, Location: 0},
	{Format: gpu.VertexFloat32x3, Offset: 12, Location: 1},
	{Format: gpu.VertexFloat32x3, Offset: 24, Location: 2},
	{Format: gpu.VertexFloat32x2, Offset: 36, Location: 3},
}}

// frameTargets makes the colour and depth targets and the read-back buffer for w x h.
func (r *Renderer) frameTargets(w, h uint32) {
	r.color = r.dev.Texture(gpu.TextureDesc{W: w, H: h, Format: colorFmt, Usage: gpu.TextureRenderAttachmt | gpu.TextureCopySrc})
	r.colorView = r.color.View()
	r.depth = r.dev.Texture(gpu.TextureDesc{W: w, H: h, Format: depthFmt, Usage: gpu.TextureRenderAttachmt})
	r.depthView = r.depth.View()
	r.readback = r.dev.Buffer(gpu.BufferCopyDst|gpu.BufferMapRead, uint64(align(w*4)*h))
}

func (r *Renderer) releaseTargets() {
	r.colorView.Release()
	r.color.Release()
	r.depthView.Release()
	r.depth.Release()
	r.readback.Release()
}

// Close releases the device and everything made on it.
func (r *Renderer) Close() {
	for _, m := range r.meshes {
		m.vb.Release()
		m.ib.Release()
	}
	for _, p := range r.pipes {
		p.Release()
	}
	r.releaseTargets()
	for i := len(r.free) - 1; i >= 0; i-- {
		r.free[i]()
	}
	r.dev.Release()
	r.adapter.Release()
	r.inst.Release()
}

func (r *Renderer) upload(i int, w, h uint32, rgba []byte) {
	r.dev.Queue.WriteTexture(r.textures[i], rgba, w*4, w, h)
}

// Resize makes the frame's targets again for a new size.
func (r *Renderer) Resize(w, h uint32) {
	if (w == r.W && h == r.H) || w == 0 || h == 0 {
		return
	}
	r.releaseTargets()
	r.frameTargets(w, h)
	r.W, r.H = w, h
}

func (r *Renderer) mesh(key string, geo Geo, merged [][]VBox) {
	v, ix := geometry(geo, merged)
	if len(v) == 0 {
		v = []vertex{{}}
	}
	if len(ix) == 0 {
		ix = []uint32{0}
	}
	r.meshes[key] = mesh{vb: r.dev.BufferWith(gpu.BufferVertex, bytesOf(v)), ib: r.dev.BufferWith(gpu.BufferIndex, bytesOf(ix)), n: uint32(len(ix))}
}

// Render renders the office into the frame and reads it back: RGBA8, premultiplied (the
// canvas's own alpha), top row first.
func (r *Renderer) Render(o *Office) ([]byte, error) {
	var out []byte
	err := r.RenderInto(o, &out)
	return out, err
}

// RenderInto is Render, into a buffer the caller keeps between frames.
func (r *Renderer) RenderInto(o *Office, out *[]byte) error {
	enc := r.encode(o)
	row := align(r.W * 4)
	enc.CopyTextureToBuffer(r.color, r.readback, row, r.W, r.H)
	r.dev.Queue.Submit(enc.Finish())
	size := uint64(row * r.H)
	if err := r.readback.MapRead(r.dev, size); err != nil {
		return err
	}
	data := r.readback.Mapped(size)
	if data == nil {
		r.readback.Unmap()
		return errors.New("office: the frame has no mapping")
	}
	o2 := (*out)[:0]
	if cap(o2) < int(r.W*r.H*4) {
		o2 = make([]byte, 0, r.W*r.H*4)
	}
	for y := 0; y < int(r.H); y++ {
		o2 = append(o2, data[y*int(row):y*int(row)+int(r.W)*4]...)
	}
	*out = o2
	r.readback.Unmap()
	if e := r.dev.Errors(); len(e) > 0 {
		return errors.New("office: " + strings.Join(e, "; "))
	}
	return nil
}

type drawItem struct {
	node  int
	depth float64
	trans bool
}

// encode records the shadow map (only when something moved), then the opaque draws, then
// the transparent ones back to front, into the colour target. Nothing is submitted.
func (r *Renderer) encode(o *Office) gpu.CommandEncoder {
	for i, c := range o.Canvases {
		if o.Dirty[i] {
			r.upload(i, uint32(c.W), uint32(c.H), c.RGBA())
			o.Dirty[i] = false
		}
	}
	view, proj := o.Camera()
	lights := o.Lights()
	shadowProj := Ortho(-12, 12, 12, -12, 1, 40)
	shadowVP := shadowProj.Mul(lights.SunView)
	iso := v3(1, 0.86, 1).Norm()
	var pts [14][4]float32
	for k, p := range lights.Points {
		if k == 7 {
			break
		}
		pts[k*2] = [4]float32{float32(p.P.X), float32(p.P.Y), float32(p.P.Z), float32(p.Dist)}
		pts[k*2+1] = [4]float32{float32(p.C.R), float32(p.C.G), float32(p.C.B), float32(p.Decay)}
	}
	v4 := func(v V3) [4]float32 { return [4]float32{float32(v.X), float32(v.Y), float32(v.Z), 0} }
	c4 := func(c Rgb) [4]float32 { return [4]float32{float32(c.R), float32(c.G), float32(c.B), 0} }
	fu := []frameU{{
		viewProj: proj.Mul(view).F32(), view: view.F32(), proj: proj.F32(), shadow: shadowVP.F32(),
		viewDir: v4(iso), hemiSky: c4(lights.HemiSky), hemiGround: c4(lights.HemiGround), sunDir: v4(lights.SunDir), sun: c4(lights.Sun),
		fillDir: v4(lights.FillDir), fill: c4(lights.Fill), points: pts, misc: [4]float32{float32(lights.Exposure), shadowSize, -0.0004, 0.03},
	}}
	r.dev.Queue.WriteBuffer(r.frameBuf, 0, bytesOf(fu))

	// Draw list: what is shown, opaque first, then transparent back to front.
	world := o.G.World()
	shown := o.G.Shown()
	vp := proj.Mul(view)
	var list []drawItem
	for i := range o.G.Nodes {
		n := &o.G.Nodes[i]
		if n.Draw == nil || !shown[i] {
			continue
		}
		m := n.Draw.Mat
		if m.Kind != MatStd && m.Opacity <= 0 && m.Transparent() {
			continue
		}
		c := vp.Point(world[i].Point(V3{}))
		list = append(list, drawItem{i, c.Z, m.Transparent()})
	}
	sort.SliceStable(list, func(a, b int) bool {
		if list[a].trans != list[b].trans {
			return !list[a].trans
		}
		return list[a].trans && list[a].depth > list[b].depth
	})
	// The draw buffer holds maxDraws; with every desk's helpers out the room is well
	// under it, but a scene that grew past it drops the last (transparent) draws rather
	// than write out of the buffer.
	if len(list) > maxDraws {
		list = list[:maxDraws]
	}
	draws := r.draws[:0]
	merged := o.G.Merged
	for len(r.keys) < len(o.G.Nodes) {
		r.keys = append(r.keys, "")
	}
	b01 := func(b bool) float64 {
		if b {
			return 1
		}
		return 0
	}
	for _, d := range list {
		n := &o.G.Nodes[d.node]
		geo, m := n.Draw.Geo, n.Draw.Mat
		model := world[d.node]
		nm := model.Inverse()
		// The normal matrix: the inverse transpose.
		var t M4
		for c := 0; c < 4; c++ {
			for rr := 0; rr < 4; rr++ {
				t[c*4+rr] = nm[rr*4+c]
			}
		}
		var color, params, flags [4]float64
		switch m.Kind {
		case MatStd:
			color, params, flags = [4]float64{m.Color.R, m.Color.G, m.Color.B, 1}, [4]float64{0, m.Rough, m.Metal, 1}, [4]float64{b01(m.Vertex), b01(n.Receive), 0, 0}
		case MatBasic:
			color, params = [4]float64{m.Color.R, m.Color.G, m.Color.B, m.Opacity}, [4]float64{1 + b01(m.Tex >= 0), 0, 0, b01(m.Tone)}
		case MatGlow:
			color, params, flags = [4]float64{m.Color.R, m.Color.G, m.Color.B, m.Opacity}, [4]float64{3, 0, 0, 1}, [4]float64{0, 0, 0, 1}
		case MatPoints:
			color, params = [4]float64{m.Color.R, m.Color.G, m.Color.B, m.Opacity}, [4]float64{4, 0, 0, 1}
		}
		f4 := func(v [4]float64) [4]float32 {
			return [4]float32{float32(v[0]), float32(v[1]), float32(v[2]), float32(v[3])}
		}
		draws = append(draws, drawU{model: model.F32(), normal: t.F32(), color: f4(color), params: f4(params), flags: f4(flags)})
		// A node's geometry is set once, when it is made: its mesh's name is worked out
		// once too, not formatted again every frame.
		if r.keys[d.node] == "" {
			switch geo.Kind {
			case GeoMerged:
				r.keys[d.node] = fmt.Sprintf("m%d", geo.Index)
			case GeoQuad, GeoPoints:
				r.keys[d.node] = fmt.Sprintf("n%d", d.node)
			default:
				r.keys[d.node] = fmt.Sprintf("%d %v %v %v %v %v %v %d", geo.Kind, geo.Top, geo.Bottom, geo.H, geo.Inner, geo.Outer, geo.W, geo.Seg)
			}
		}
		key := r.keys[d.node]
		if _, ok := r.meshes[key]; !ok {
			r.mesh(key, geo, merged)
		}
		pk, _ := kind(geo, m)
		if _, ok := r.pipes[pk]; !ok {
			r.pipes[pk] = r.pipeline(pk)
		}
	}
	r.dev.Queue.WriteBuffer(r.drawBuf, 0, bytesOf(draws))
	r.draws = draws

	enc := r.dev.Encoder()
	if o.ShadowDirty {
		o.ShadowDirty = false
		pass := enc.RenderPass("shadow", nil, &gpu.DepthAttachment{View: r.shadowView, Clear: 1})
		pass.SetPipeline(r.shadowPipe)
		pass.SetBindGroup(0, r.g0Shadow)
		pass.SetBindGroup(2, r.texGroups[7])
		for k, d := range list {
			n := &o.G.Nodes[d.node]
			if !n.Cast || n.Draw.Mat.Kind != MatStd {
				continue
			}
			m := r.meshes[r.keys[d.node]]
			pass.SetBindGroup(1, r.g1, uint32(k*drawSize))
			pass.SetVertexBuffer(0, m.vb)
			pass.SetIndexBuffer(m.ib, gpu.IndexUint32)
			pass.DrawIndexed(m.n, 1, 0, 0, 0)
		}
		pass.End()
	}
	pass := enc.RenderPass("office", &gpu.ColorAttachment{View: r.colorView}, &gpu.DepthAttachment{View: r.depthView, Clear: 1})
	pass.SetBindGroup(0, r.g0)
	for k, d := range list {
		n := &o.G.Nodes[d.node]
		pk, tex := kind(n.Draw.Geo, n.Draw.Mat)
		m := r.meshes[r.keys[d.node]]
		pass.SetPipeline(r.pipes[pk])
		pass.SetBindGroup(1, r.g1, uint32(k*drawSize))
		pass.SetBindGroup(2, r.texGroups[tex])
		pass.SetVertexBuffer(0, m.vb)
		pass.SetIndexBuffer(m.ib, gpu.IndexUint32)
		pass.DrawIndexed(m.n, 1, 0, 0, 0)
	}
	pass.End()
	return enc
}

// kind is a draw's pipeline kind and its texture.
func kind(geo Geo, m Mat) (pipeKey, int) {
	var k pipeKey
	tex := 7
	switch m.Kind {
	case MatStd:
		k.dw = true
	case MatBasic:
		if m.Transparent() {
			k.blend = 1
			if m.Blend == BlendAdditive {
				k.blend = 2
			}
		}
		k.dw, k.double = m.DepthWrite, m.Double
		if m.Tex >= 0 {
			tex = m.Tex
		}
	case MatGlow:
		k.blend, tex = 2, 6
	case MatPoints:
		k.blend = 2
	}
	if geo.Kind == GeoPoints {
		k.topo = 1
	}
	return k, tex
}

func (r *Renderer) pipeline(k pipeKey) gpu.RenderPipeline {
	var b *gpu.Blend
	switch k.blend {
	case 1:
		b = &gpu.Blend{
			Color: gpu.BlendComponent{Op: gpu.BlendAdd, Src: gpu.FactorSrcAlpha, Dst: gpu.FactorOneMinusSrcAlpha},
			Alpha: gpu.BlendComponent{Op: gpu.BlendAdd, Src: gpu.FactorOne, Dst: gpu.FactorOneMinusSrcAlpha},
		}
	case 2:
		b = &gpu.Blend{
			Color: gpu.BlendComponent{Op: gpu.BlendAdd, Src: gpu.FactorSrcAlpha, Dst: gpu.FactorOne},
			Alpha: gpu.BlendComponent{Op: gpu.BlendAdd, Src: gpu.FactorSrcAlpha, Dst: gpu.FactorOne},
		}
	}
	topo, cull := gpu.TriangleList, gpu.CullBack
	if k.topo == 1 {
		topo = gpu.PointList
	}
	if k.double || k.topo == 1 {
		cull = gpu.CullNone
	}
	return r.dev.RenderPipeline(gpu.PipelineDesc{
		Layout: r.layout, Module: r.shader, VS: "vs", FS: "fs",
		Buffers: []gpu.VertexLayout{vertexLayout}, Targets: []gpu.Target{{Format: colorFmt, Blend: b}},
		Topology: topo, Front: gpu.FrontCCW, Cull: cull,
		Depth: &gpu.DepthState{Format: depthFmt, Write: k.dw, Compare: gpu.CompareLessEqual},
	})
}
