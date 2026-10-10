//go:build windows

package main

// The notch's pixels: a Direct3D 11 device, a swap chain made for composition with
// premultiplied alpha, and a DirectComposition visual that shows it in the window. This
// is what notch-proto gets from wgpu's DxgiFromVisual: the only DXGI path that keeps
// per-pixel alpha (an HWND swap chain shows black where the notch is empty). Gio draws
// into the swap chain's buffer through its gpu package.
//
// The COM method numbers were read from Wine's dxgi.idl, dxgi1_2.idl, d3d11.idl and
// dcomp.idl: IUnknown's three come first, then each base interface's in order.

import (
	"fmt"
	"image"
	"image/color"
	"syscall"
	"unsafe"

	"gioui.org/gpu"
	"gioui.org/op"
	"golang.org/x/sys/windows"
)

var (
	d3d11dll = windows.NewLazySystemDLL("d3d11.dll")
	dcompdll = windows.NewLazySystemDLL("dcomp.dll")

	pD3D11CreateDevice        = d3d11dll.NewProc("D3D11CreateDevice")
	pDCompositionCreateDevice = dcompdll.NewProc("DCompositionCreateDevice")

	iidIDXGIDevice         = guid("{54ec77fa-1377-44e6-8c32-88fd5f44c84c}")
	iidIDXGIFactory2       = guid("{50c83a1c-e072-4c48-87b0-3630fa36a6d0}")
	iidID3D11Texture2D     = guid("{6f15aaf2-d208-4e89-9ab4-489535d34f9c}")
	iidIDCompositionDevice = guid("{c37ea93a-e7aa-450d-b16f-9746cb0407f3}")
)

const (
	driverHardware = 1
	driverWARP     = 5
	bgraSupport    = 0x20
	sdkVersion     = 7

	// DXGI_FORMAT_B8G8R8A8_UNORM: a composition swap chain's buffer, and the view Gio draws
	// into. The patched Gio (third_party) writes sRGB-encoded, premultiplied colour, which
	// is what DirectComposition takes.
	formatBGRA8    = 87
	usageRenderOut = 0x20
	flipSequential = 3
	alphaPremul    = 1
	rtvTexture2D   = 4
)

// com is a COM object: a pointer to its method table.
type com struct{ vtbl *[64]uintptr }

func (o *com) call(method int, args ...uintptr) uintptr {
	r, _, _ := syscall.SyscallN(o.vtbl[method], append([]uintptr{uintptr(unsafe.Pointer(o))}, args...)...)
	return r
}

func (o *com) release() {
	if o != nil {
		o.call(2)
	}
}

func guid(s string) windows.GUID {
	g, err := windows.GUIDFromString(s)
	if err != nil {
		panic(err)
	}
	return g
}

func failed(hr uintptr, what string) error {
	if int32(hr) < 0 {
		return fmt.Errorf("%s: HRESULT %#x", what, uint32(hr))
	}
	return nil
}

type swapChainDesc1 struct {
	Width, Height, Format uint32
	Stereo                int32
	SampleCount, SampleQ  uint32
	BufferUsage, Buffers  uint32
	Scaling, SwapEffect   uint32
	AlphaMode, Flags      uint32
}

type rtvDesc struct {
	Format, Dimension, MipSlice uint32
	_                           [2]uint32 // the union's largest member is 12 bytes
}

type graphics struct {
	dev, swap, rtv *com
	dcomp, target  *com
	visual         *com
	gpu            gpu.GPU
	size           image.Point
	driver         string
}

func newGraphics(hwnd uintptr, w, h int) (*graphics, error) {
	g := &graphics{size: image.Pt(w, h), driver: "hardware"}
	hr := call(pD3D11CreateDevice, 0, driverHardware, 0, bgraSupport, 0, 0, sdkVersion, uintptr(unsafe.Pointer(&g.dev)), 0, 0)
	if int32(hr) < 0 {
		// No GPU (a CI runner): Windows' own software rasteriser.
		g.driver = "WARP"
		hr = call(pD3D11CreateDevice, 0, driverWARP, 0, bgraSupport, 0, 0, sdkVersion, uintptr(unsafe.Pointer(&g.dev)), 0, 0)
	}
	if err := failed(hr, "D3D11CreateDevice"); err != nil {
		return nil, err
	}
	var dxgiDev, adapter, factory *com
	if err := failed(g.dev.call(0, uintptr(unsafe.Pointer(&iidIDXGIDevice)), uintptr(unsafe.Pointer(&dxgiDev))), "IDXGIDevice"); err != nil {
		return nil, err
	}
	defer dxgiDev.release()
	if err := failed(dxgiDev.call(7, uintptr(unsafe.Pointer(&adapter))), "IDXGIDevice.GetAdapter"); err != nil {
		return nil, err
	}
	defer adapter.release()
	if err := failed(adapter.call(6, uintptr(unsafe.Pointer(&iidIDXGIFactory2)), uintptr(unsafe.Pointer(&factory))), "IDXGIAdapter.GetParent"); err != nil {
		return nil, err
	}
	defer factory.release()
	desc := swapChainDesc1{
		Width: uint32(w), Height: uint32(h), Format: formatBGRA8, SampleCount: 1,
		BufferUsage: usageRenderOut, Buffers: 2, SwapEffect: flipSequential, AlphaMode: alphaPremul,
	}
	if err := failed(factory.call(24, uintptr(unsafe.Pointer(g.dev)), uintptr(unsafe.Pointer(&desc)), 0, uintptr(unsafe.Pointer(&g.swap))), "CreateSwapChainForComposition"); err != nil {
		return nil, err
	}
	// In Direct3D 11 buffer 0 is always the one to draw into, so one view lasts.
	var tex *com
	if err := failed(g.swap.call(9, 0, uintptr(unsafe.Pointer(&iidID3D11Texture2D)), uintptr(unsafe.Pointer(&tex))), "IDXGISwapChain.GetBuffer"); err != nil {
		return nil, err
	}
	rd := rtvDesc{Format: formatBGRA8, Dimension: rtvTexture2D}
	hr = g.dev.call(9, uintptr(unsafe.Pointer(tex)), uintptr(unsafe.Pointer(&rd)), uintptr(unsafe.Pointer(&g.rtv)))
	tex.release()
	if err := failed(hr, "CreateRenderTargetView"); err != nil {
		return nil, err
	}
	if err := failed(call(pDCompositionCreateDevice, uintptr(unsafe.Pointer(dxgiDev)), uintptr(unsafe.Pointer(&iidIDCompositionDevice)), uintptr(unsafe.Pointer(&g.dcomp))), "DCompositionCreateDevice"); err != nil {
		return nil, err
	}
	if err := failed(g.dcomp.call(6, hwnd, 1, uintptr(unsafe.Pointer(&g.target))), "CreateTargetForHwnd"); err != nil {
		return nil, err
	}
	if err := failed(g.dcomp.call(7, uintptr(unsafe.Pointer(&g.visual))), "CreateVisual"); err != nil {
		return nil, err
	}
	if err := failed(g.visual.call(15, uintptr(unsafe.Pointer(g.swap))), "Visual.SetContent"); err != nil {
		return nil, err
	}
	if err := failed(g.target.call(3, uintptr(unsafe.Pointer(g.visual))), "Target.SetRoot"); err != nil {
		return nil, err
	}
	if err := failed(g.dcomp.call(3), "Commit"); err != nil {
		return nil, err
	}
	gp, err := gpu.New(gpu.Direct3D11{Device: unsafe.Pointer(g.dev)})
	if err != nil {
		return nil, fmt.Errorf("gio gpu: %w", err)
	}
	g.gpu = gp
	return g, nil
}

// present draws one frame of Gio operations, cleared to fully transparent, and shows it.
func (g *graphics) present(ops *op.Ops) error {
	g.gpu.Clear(color.NRGBA{})
	if err := g.gpu.Frame(ops, gpu.Direct3D11RenderTarget{RenderTarget: unsafe.Pointer(g.rtv)}, g.size); err != nil {
		return err
	}
	return failed(g.swap.call(8, 1, 0), "Present")
}
