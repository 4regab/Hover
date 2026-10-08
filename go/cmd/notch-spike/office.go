//go:build windows

package main

// The office stand-in on wgpu-native through go-webgpu (no C compiler): notch-proto's
// shader, drawn offscreen once, read back and handed to Gio as an image. That is the
// Rust office's own design (render, read back, compose on the CPU). It also compiles
// the real office shaders, so a WGSL feature wgpu-native v29 refuses shows up now.

import (
	"fmt"
	"image"
	"os"
	"path/filepath"
	"unsafe"

	"github.com/go-webgpu/webgpu/wgpu"
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
	// ponytail: wgpu_native.dll next to the exe is found by hand; the installer will put it there.
	if os.Getenv("WGPU_NATIVE_PATH") == "" {
		if exe, e := os.Executable(); e == nil {
			if p := filepath.Join(filepath.Dir(exe), "wgpu_native.dll"); fileExists(p) {
				os.Setenv("WGPU_NATIVE_PATH", p)
			}
		}
	}
	if err = wgpu.Init(); err != nil {
		return res, err
	}
	inst, err := wgpu.CreateInstance(nil)
	if err != nil {
		return res, err
	}
	defer inst.Release()
	adapter, err := inst.RequestAdapter(&wgpu.RequestAdapterOptions{PowerPreference: wgpu.PowerPreferenceLowPower})
	if err != nil {
		// No GPU (a CI runner): the software adapter, as notch-proto gets WARP there.
		if adapter, err = inst.RequestAdapter(&wgpu.RequestAdapterOptions{ForceFallbackAdapter: true}); err != nil {
			return res, err
		}
	}
	defer adapter.Release()
	if info, e := adapter.Info(); e == nil {
		res.adapter = fmt.Sprintf("%s (backend %v, type %v)", info.Device, info.BackendType, info.AdapterType)
	}
	dev, err := adapter.RequestDevice(nil)
	if err != nil {
		return res, err
	}
	defer dev.Release()
	queue := dev.Queue()
	defer queue.Release()

	for _, name := range []string{"office.wgsl", "page.wgsl"} {
		if wgslDir == "" {
			break
		}
		src, e := os.ReadFile(filepath.Join(wgslDir, name))
		if e != nil {
			res.shaders[name] = e.Error()
			continue
		}
		dev.PushErrorScope(wgpu.ErrorFilterValidation)
		m, e := dev.CreateShaderModuleWGSL(string(src))
		typ, text, pe := dev.PopErrorScopeAsync(inst)
		switch {
		case e != nil:
			res.shaders[name] = e.Error()
		case pe != nil:
			res.shaders[name] = pe.Error()
		case typ != wgpu.ErrorTypeNoError:
			res.shaders[name] = text
		default:
			res.shaders[name] = ""
		}
		if m != nil {
			m.Release()
		}
	}

	const w, h = 1104, 424
	tex, err := dev.CreateTexture(&wgpu.TextureDescriptor{
		Label: "office stand-in", Usage: wgpu.TextureUsageRenderAttachment | wgpu.TextureUsageCopySrc,
		Dimension: wgpu.TextureDimension2D, Size: wgpu.Extent3D{Width: w, Height: h, DepthOrArrayLayers: 1},
		Format: wgpu.TextureFormatRGBA8Unorm, MipLevelCount: 1, SampleCount: 1,
	})
	if err != nil {
		return res, err
	}
	defer tex.Release()
	view, err := tex.CreateView(nil)
	if err != nil {
		return res, err
	}
	defer view.Release()
	module, err := dev.CreateShaderModuleWGSL(sceneWGSL)
	if err != nil {
		return res, err
	}
	defer module.Release()
	pipe, err := dev.CreateRenderPipelineSimple(nil, module, "vs", module, "fs", wgpu.TextureFormatRGBA8Unorm)
	if err != nil {
		return res, err
	}
	defer pipe.Release()

	const row = (w*4 + 255) / 256 * 256 // rows of a texture copy are 256-byte aligned
	buf, err := dev.CreateBuffer(&wgpu.BufferDescriptor{Usage: wgpu.BufferUsageMapRead | wgpu.BufferUsageCopyDst, Size: row * h})
	if err != nil {
		return res, err
	}
	defer buf.Release()

	enc, err := dev.CreateCommandEncoder(nil)
	if err != nil {
		return res, err
	}
	pass, err := enc.BeginRenderPass(&wgpu.RenderPassDescriptor{ColorAttachments: []wgpu.RenderPassColorAttachment{{
		View: view, LoadOp: wgpu.LoadOpClear, StoreOp: wgpu.StoreOpStore, ClearValue: wgpu.Color{A: 1},
	}}})
	if err != nil {
		return res, err
	}
	pass.SetPipeline(pipe)
	pass.Draw(3, 1, 0, 0)
	pass.End()
	pass.Release()
	enc.CopyTextureToBuffer(tex, buf, []wgpu.BufferTextureCopy{{
		BufferLayout: wgpu.ImageDataLayout{BytesPerRow: row, RowsPerImage: h},
		TextureBase:  wgpu.ImageCopyTexture{Texture: tex, Aspect: wgpu.TextureAspectAll},
		Size:         wgpu.Extent3D{Width: w, Height: h, DepthOrArrayLayers: 1},
	}})
	cmd, err := enc.Finish()
	enc.Release()
	if err != nil {
		return res, err
	}
	if _, err = queue.Submit(cmd); err != nil {
		return res, err
	}
	cmd.Release()
	if err = buf.MapAsyncBlocking(dev, wgpu.MapModeRead, 0, row*h); err != nil {
		return res, err
	}
	p := buf.GetMappedRange(0, row*h)
	if p == nil {
		return res, fmt.Errorf("the read-back buffer has no mapping")
	}
	raw := unsafe.Slice((*byte)(p), row*h)
	res.img = image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		copy(res.img.Pix[y*w*4:(y+1)*w*4], raw[y*row:y*row+w*4])
	}
	return res, buf.Unmap()
}

func fileExists(p string) bool { _, err := os.Stat(p); return err == nil }
