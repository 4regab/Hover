package gpu

import (
	"encoding/binary"
	"math"
	"testing"
)

// TestLive runs the binding on a real adapter: wgpu-native must be loadable (beside the
// test or at WGPU_NATIVE_PATH), else it skips. CI's Windows job runs it on WARP.
func TestLive(t *testing.T) {
	if err := Init(); err != nil {
		t.Skip(err)
	}
	inst, err := CreateInstance()
	if err != nil {
		t.Fatal(err)
	}
	defer inst.Release()
	a, err := inst.RequestAdapter(false)
	if err != nil {
		if a, err = inst.RequestAdapter(true); err != nil {
			t.Fatal(err)
		}
	}
	defer a.Release()
	t.Log("adapter:", a.Info())
	d, err := inst.RequestDevice(a, "gpu test")
	if err != nil {
		t.Fatal(err)
	}
	defer d.Release()

	if msg := d.CheckShader(inst, "bad", "fn main( {"); msg == "" {
		t.Error("broken WGSL compiled")
	}
	const wgsl = `
struct U { color: vec4<f32> }
@group(0) @binding(0) var<uniform> u: U;
@vertex fn vs(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4<f32>(p, 0.5, 1.0); }
@fragment fn fs() -> @location(0) vec4<f32> { return u.color; }
`
	if msg := d.CheckShader(inst, "good", wgsl); msg != "" {
		t.Fatal("good WGSL:", msg)
	}

	// A red triangle over the middle of a 64x64 target, its colour from a uniform.
	const w, h = 64, 64
	mod := d.ShaderModule("triangle", wgsl)
	defer mod.Release()
	bgl := d.BindGroupLayout(LayoutEntry{Binding: 0, Visibility: StageFragment, Uniform: true, MinSize: 16})
	defer bgl.Release()
	pl := d.PipelineLayout(bgl)
	defer pl.Release()
	pipe := d.RenderPipeline(PipelineDesc{
		Label: "triangle", Layout: pl, Module: mod, VS: "vs", FS: "fs",
		Buffers: []VertexLayout{{Stride: 8, Attrs: []VertexAttr{{Format: VertexFloat32x2, Location: 0}}}},
		Targets: []Target{{Format: FormatRGBA8Unorm}},
		Depth:   &DepthState{Format: FormatDepth32Float, Write: true, Compare: CompareLessEqual},
	})
	defer pipe.Release()
	verts := d.BufferWith(BufferVertex, f32s(-1, -1, 3, -1, -1, 3))
	defer verts.Release()
	ubuf := d.BufferWith(BufferUniform, f32s(1, 0, 0, 1))
	defer ubuf.Release()
	group := d.BindGroup(bgl, GroupEntry{Binding: 0, Buffer: ubuf})
	defer group.Release()
	target := d.Texture(TextureDesc{Label: "target", W: w, H: h, Format: FormatRGBA8Unorm, Usage: TextureRenderAttachmt | TextureCopySrc})
	defer target.Release()
	tv := target.View()
	defer tv.Release()
	depth := d.Texture(TextureDesc{Label: "depth", W: w, H: h, Format: FormatDepth32Float, Usage: TextureRenderAttachmt})
	defer depth.Release()
	dv := depth.View()
	defer dv.Release()
	const bpr = 256 // w*4 rounded up to 256
	read := d.Buffer(BufferMapRead|BufferCopyDst, bpr*h)
	defer read.Release()

	// A depth-only pass first (the shadow map's kind), then the colour pass.
	shadow := d.RenderPipeline(PipelineDesc{
		Label: "depth only", Layout: pl, Module: mod, VS: "vs",
		Buffers: []VertexLayout{{Stride: 8, Attrs: []VertexAttr{{Format: VertexFloat32x2, Location: 0}}}},
		Depth:   &DepthState{Format: FormatDepth32Float, Write: true, Compare: CompareLessEqual},
	})
	defer shadow.Release()
	enc := d.Encoder()
	p := enc.RenderPass("depth", nil, &DepthAttachment{View: dv, Clear: 1})
	p.SetPipeline(shadow)
	p.SetBindGroup(0, group)
	p.SetVertexBuffer(0, verts)
	p.Draw(3, 1, 0, 0)
	p.End()
	p = enc.RenderPass("colour", &ColorAttachment{View: tv, Clear: Color{0, 0, 1, 1}}, &DepthAttachment{View: dv, Clear: 1})
	p.SetPipeline(pipe)
	p.SetBindGroup(0, group)
	p.SetVertexBuffer(0, verts)
	p.Draw(3, 1, 0, 0)
	p.End()
	enc.CopyTextureToBuffer(target, read, bpr, w, h)
	d.Queue.Submit(enc.Finish())
	if err := read.MapRead(d, bpr*h); err != nil {
		t.Fatal(err)
	}
	px := read.Mapped(bpr * h)
	mid := px[32*bpr+32*4:][:4]
	got := [4]byte{mid[0], mid[1], mid[2], mid[3]}
	read.Unmap()
	if got != [4]byte{255, 0, 0, 255} {
		t.Errorf("middle pixel %v, want red", got)
	}
	if e := d.Errors(); len(e) > 0 {
		t.Errorf("device errors: %q", e)
	}
}

func f32s(v ...float32) []byte {
	b := make([]byte, 4*len(v))
	for i, f := range v {
		binary.LittleEndian.PutUint32(b[4*i:], math.Float32bits(f))
	}
	return b
}
