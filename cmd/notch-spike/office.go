//go:build windows

package main

// The office stand-in on wgpu-native through internal/gpu (no C compiler): notch-proto's
// shader, drawn offscreen once, read back and handed to Gio as an image. That is the
// Rust office's own design (render, read back, compose on the CPU). It also compiles
// the real office shaders, so a WGSL feature wgpu-native v29 refuses shows up now.

import (
	"errors"
	"image"
	"os"
	"path/filepath"
	"strings"

	"github.com/4regab/Hover/internal/gpu"
)

// sceneWGSL is notch-proto's SCENE_WGSL: #office's radial background and an isometric
// floor grid.
const sceneWGSL = `
struct V { @builtin(position) pos: vec4f, @location(0) uv: vec2f };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    var p = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    var o: V;
    o.pos = vec4f(p[i], 0.0, 1.0);
    o.uv = p[i] * vec2f(0.5, -0.5) + vec2f(0.5, 0.5);
    return o;
}
@fragment fn fs(v: V) -> @location(0) vec4f {
    let d = length((v.uv - vec2f(0.5, 0.45)) * vec2f(1.0, 1.25));
    var c = mix(vec3f(0.165, 0.094, 0.141), vec3f(0.027, 0.020, 0.039), clamp(d * 1.6, 0.0, 1.0));
    let g = abs(fract((v.uv.x + v.uv.y * 2.4) * 18.0) - 0.5) + abs(fract((v.uv.x - v.uv.y * 2.4) * 18.0) - 0.5);
    let floor = step(0.55, v.uv.y) * (1.0 - smoothstep(0.0, 0.06, min(g, 1.0 - g)));
    c = c + vec3f(0.35, 0.24, 0.20) * floor * 0.5;
    return vec4f(c, 1.0);
}
`

type officeResult struct {
	img     *image.RGBA
	adapter string
	shaders map[string]string // file name -> "" when it compiled, else wgpu's message
}

func renderOffice(wgslDir string) (res officeResult, err error) {
	res.shaders = map[string]string{}
	inst, err := gpu.CreateInstance()
	if err != nil {
		return res, err
	}
	defer inst.Release()
	adapter, err := inst.RequestAdapter(false)
	if err != nil {
		// No GPU (a CI runner): the software adapter, as notch-proto gets WARP there.
		if adapter, err = inst.RequestAdapter(true); err != nil {
			return res, err
		}
	}
	defer adapter.Release()
	res.adapter = adapter.Info().String()
	dev, err := inst.RequestDevice(adapter, "office stand-in")
	if err != nil {
		return res, err
	}
	defer dev.Release()

	for _, name := range []string{"office.wgsl", "page.wgsl"} {
		if wgslDir == "" {
			break
		}
		src, e := os.ReadFile(filepath.Join(wgslDir, name))
		if e != nil {
			res.shaders[name] = e.Error()
			continue
		}
		res.shaders[name] = dev.CheckShader(inst, name, string(src))
	}

	const w, h = 1104, 424
	tex := dev.Texture(gpu.TextureDesc{Label: "office stand-in", W: w, H: h, Format: gpu.FormatRGBA8Unorm,
		Usage: gpu.TextureRenderAttachmt | gpu.TextureCopySrc})
	defer tex.Release()
	view := tex.View()
	defer view.Release()
	module := dev.ShaderModule("scene", sceneWGSL)
	defer module.Release()
	pipe := dev.RenderPipeline(gpu.PipelineDesc{Label: "scene", Module: module, VS: "vs", FS: "fs",
		Targets: []gpu.Target{{Format: gpu.FormatRGBA8Unorm}}})
	defer pipe.Release()

	const row = (w*4 + 255) / 256 * 256 // rows of a texture copy are 256-byte aligned
	buf := dev.Buffer(gpu.BufferMapRead|gpu.BufferCopyDst, row*h)
	defer buf.Release()

	enc := dev.Encoder()
	pass := enc.RenderPass("scene", &gpu.ColorAttachment{View: view, Clear: gpu.Color{A: 1}}, nil)
	pass.SetPipeline(pipe)
	pass.Draw(3, 1, 0, 0)
	pass.End()
	enc.CopyTextureToBuffer(tex, buf, row, w, h)
	dev.Queue.Submit(enc.Finish())
	if err = buf.MapRead(dev, row*h); err != nil {
		return res, err
	}
	raw := buf.Mapped(row * h)
	if raw == nil {
		return res, errors.New("the read-back buffer has no mapping")
	}
	res.img = image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		copy(res.img.Pix[y*w*4:(y+1)*w*4], raw[y*row:y*row+w*4])
	}
	buf.Unmap()
	if e := dev.Errors(); len(e) > 0 {
		return res, errors.New(strings.Join(e, "; "))
	}
	return res, nil
}
