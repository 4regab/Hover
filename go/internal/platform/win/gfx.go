//go:build windows

package win

// The pixels: one Direct3D 11 device for every window (the Rust build shares one wgpu
// device the same way, and each extra one cost 100 to 200 MB), and for each window a
// swap chain. The notch's is made for composition with premultiplied alpha and shown by a
// DirectComposition visual: the only DXGI path that keeps per-pixel alpha (an HWND swap
// chain shows black where the notch is empty). The app window's is an ordinary HWND one.
// Gio draws into the swap chain's buffer through its gpu package.
//
// The COM method numbers were read from Wine's dxgi.idl, dxgi1_2.idl, d3d11.idl and
// dcomp.idl: IUnknown's three come first, then each base interface's in order.

import (
	"errors"
	"fmt"
	"image"
	"image/color"
	"sync"
	"syscall"
	"unsafe"

	"gioui.org/gpu"
	"gioui.org/op"
	"golang.org/x/sys/windows"
)

var (
	d3d11dll = windows.NewLazySystemDLL("d3d11.dll")
	dcompdll = windows.NewLazySystemDLL("dcomp.dll")
	dxgidll  = windows.NewLazySystemDLL("dxgi.dll")

	pD3D11CreateDevice        = d3d11dll.NewProc("D3D11CreateDevice")
	pDCompositionCreateDevice = dcompdll.NewProc("DCompositionCreateDevice")
	pCreateDXGIFactory1       = dxgidll.NewProc("CreateDXGIFactory1")

	iidIDXGIDevice         = guid("{54ec77fa-1377-44e6-8c32-88fd5f44c84c}")
	iidIDXGIFactory1       = guid("{770aae78-f26f-4dba-a829-253c83d1b387}")
	iidIDXGIFactory2       = guid("{50c83a1c-e072-4c48-87b0-3630fa36a6d0}")
	iidID3D11Texture2D     = guid("{6f15aaf2-d208-4e89-9ab4-489535d34f9c}")
	iidIDCompositionDevice = guid("{c37ea93a-e7aa-450d-b16f-9746cb0407f3}")
)

const (
	driverUnknown  = 0
	driverHardware = 1
	driverWARP     = 5
	bgraSupport    = 0x20
	sdkVersion     = 7

	// DXGI_FORMAT_B8G8R8A8_UNORM: a swap chain's buffer, and the view Gio draws into. The
	// patched Gio (third_party) writes sRGB-encoded, premultiplied colour, which is what
	// DirectComposition takes.
	formatBGRA8    = 87
	usageRenderOut = 0x20
	flipSequential = 3
	alphaPremul    = 1

	dxgiErrDeviceRemoved = 0x887A0005
	dxgiErrDeviceReset   = 0x887A0007
	dxgiStatusOccluded   = 0x087A0001
)

// com is a COM object: a pointer to its method table.
type com struct{ vtbl *[160]uintptr }

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
		return &hrError{what, uint32(hr)}
	}
	return nil
}

type hrError struct {
	what string
	hr   uint32
}

func (e *hrError) Error() string { return fmt.Sprintf("%s: HRESULT %#x", e.what, e.hr) }

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

type adapterDesc1 struct {
	Description                                   [128]uint16
	VendorID, DeviceID, SubSysID, Revision        uint32
	DedicatedVideo, DedicatedSystem, SharedSystem uintptr
	LuidLow                                       uint32
	LuidHigh                                      int32
	Flags                                         uint32
}

// device is the shared Direct3D 11 device and what hangs off it.
type device struct {
	dev, dxgi, factory *com
	driver, adapter    string
	gen                int
}

var shared struct {
	mu  sync.Mutex
	d   *device
	gen int
}

// sharedDevice is the one device: made on first use, on the card that drives the main
// display (as the Rust build picks it: left to the system, a PC whose screens hang off the
// discrete card got the integrated one, and every frame crossed between them), else the
// default, else Windows' own software rasteriser (a CI runner, a VM).
func sharedDevice() (*device, error) {
	shared.mu.Lock()
	defer shared.mu.Unlock()
	if shared.d != nil {
		return shared.d, nil
	}
	d := &device{driver: "hardware", gen: shared.gen}
	var adapter *com
	want := primaryDisplayAdapter()
	if want != "" {
		adapter = findAdapter(want)
	}
	var hr uintptr
	if adapter != nil {
		hr = call(pD3D11CreateDevice, uintptr(unsafe.Pointer(adapter)), driverUnknown, 0, bgraSupport, 0, 0, sdkVersion, uintptr(unsafe.Pointer(&d.dev)), 0, 0)
		d.adapter = want
		adapter.release()
	}
	if adapter == nil || int32(hr) < 0 {
		d.adapter = ""
		hr = call(pD3D11CreateDevice, 0, driverHardware, 0, bgraSupport, 0, 0, sdkVersion, uintptr(unsafe.Pointer(&d.dev)), 0, 0)
	}
	if int32(hr) < 0 {
		// No GPU: Windows' own software rasteriser.
		d.driver = "WARP"
		hr = call(pD3D11CreateDevice, 0, driverWARP, 0, bgraSupport, 0, 0, sdkVersion, uintptr(unsafe.Pointer(&d.dev)), 0, 0)
	}
	if err := failed(hr, "D3D11CreateDevice"); err != nil {
		return nil, err
	}
	var adapterOf *com
	if err := failed(d.dev.call(0, uintptr(unsafe.Pointer(&iidIDXGIDevice)), uintptr(unsafe.Pointer(&d.dxgi))), "IDXGIDevice"); err != nil {
		return nil, err
	}
	if err := failed(d.dxgi.call(7, uintptr(unsafe.Pointer(&adapterOf))), "IDXGIDevice.GetAdapter"); err != nil {
		return nil, err
	}
	defer adapterOf.release()
	if err := failed(adapterOf.call(6, uintptr(unsafe.Pointer(&iidIDXGIFactory2)), uintptr(unsafe.Pointer(&d.factory))), "IDXGIAdapter.GetParent"); err != nil {
		return nil, err
	}
	shared.d = d
	return d, nil
}

// findAdapter is the DXGI adapter with this description, or nil.
func findAdapter(name string) *com {
	var factory *com
	if int32(call(pCreateDXGIFactory1, uintptr(unsafe.Pointer(&iidIDXGIFactory1)), uintptr(unsafe.Pointer(&factory)))) < 0 {
		return nil
	}
	defer factory.release()
	for i := 0; ; i++ {
		var a *com
		if int32(factory.call(12, uintptr(i), uintptr(unsafe.Pointer(&a)))) < 0 {
			return nil
		}
		var desc adapterDesc1
		if int32(a.call(10, uintptr(unsafe.Pointer(&desc)))) >= 0 && windows.UTF16ToString(desc.Description[:]) == name {
			return a
		}
		a.release()
	}
}

// lose drops the device after DXGI said it was removed: every window makes its target
// again on its next frame.
func lose(d *device) {
	shared.mu.Lock()
	if shared.d == d {
		shared.d = nil
		shared.gen++
	}
	shared.mu.Unlock()
}

// target is a window's swap chain, its render target view and the Gio gpu drawing into it.
type target struct {
	d           *device
	hwnd        uintptr
	swap, rtv   *com
	dcomp, root *com
	visual      *com
	gpu         gpu.GPU
	size        image.Point
	composition bool
}

// ErrLost says the device was lost: make the target again.
var ErrLost = errors.New("win: the graphics device was lost")

func newTarget(hwnd uintptr, w, h int, composition bool) (*target, error) {
	d, err := sharedDevice()
	if err != nil {
		return nil, err
	}
	t := &target{d: d, hwnd: hwnd, size: image.Pt(w, h), composition: composition}
	desc := swapChainDesc1{
		Width: uint32(w), Height: uint32(h), Format: formatBGRA8, SampleCount: 1,
		BufferUsage: usageRenderOut, Buffers: 2, SwapEffect: flipSequential,
	}
	if composition {
		desc.AlphaMode = alphaPremul
		err = failed(d.factory.call(24, uintptr(unsafe.Pointer(d.dev)), uintptr(unsafe.Pointer(&desc)), 0, uintptr(unsafe.Pointer(&t.swap))), "CreateSwapChainForComposition")
	} else {
		err = failed(d.factory.call(15, uintptr(unsafe.Pointer(d.dev)), hwnd, uintptr(unsafe.Pointer(&desc)), 0, 0, uintptr(unsafe.Pointer(&t.swap))), "CreateSwapChainForHwnd")
	}
	if err != nil {
		return nil, err
	}
	if err := t.view(); err != nil {
		t.release()
		return nil, err
	}
	if composition {
		if err := t.compose(); err != nil {
			t.release()
			return nil, err
		}
	}
	gp, err := gpu.New(gpu.Direct3D11{Device: unsafe.Pointer(d.dev)})
	if err != nil {
		t.release()
		return nil, fmt.Errorf("gio gpu: %w", err)
	}
	t.gpu = gp
	return t, nil
}

// view makes the render target view of buffer 0 (in Direct3D 11 it is always the one to
// draw into, so one view lasts until the buffers are resized).
func (t *target) view() error {
	var tex *com
	if err := failed(t.swap.call(9, 0, uintptr(unsafe.Pointer(&iidID3D11Texture2D)), uintptr(unsafe.Pointer(&tex))), "IDXGISwapChain.GetBuffer"); err != nil {
		return err
	}
	rd := rtvDesc{Format: formatBGRA8, Dimension: 4}
	hr := t.d.dev.call(9, uintptr(unsafe.Pointer(tex)), uintptr(unsafe.Pointer(&rd)), uintptr(unsafe.Pointer(&t.rtv)))
	tex.release()
	return failed(hr, "CreateRenderTargetView")
}

// compose shows the swap chain in the window through DirectComposition.
func (t *target) compose() error {
	if err := failed(call(pDCompositionCreateDevice, uintptr(unsafe.Pointer(t.d.dxgi)), uintptr(unsafe.Pointer(&iidIDCompositionDevice)), uintptr(unsafe.Pointer(&t.dcomp))), "DCompositionCreateDevice"); err != nil {
		return err
	}
	if err := failed(t.dcomp.call(6, t.hwnd, 1, uintptr(unsafe.Pointer(&t.root))), "CreateTargetForHwnd"); err != nil {
		return err
	}
	if err := failed(t.dcomp.call(7, uintptr(unsafe.Pointer(&t.visual))), "CreateVisual"); err != nil {
		return err
	}
	if err := failed(t.visual.call(15, uintptr(unsafe.Pointer(t.swap))), "Visual.SetContent"); err != nil {
		return err
	}
	if err := failed(t.root.call(3, uintptr(unsafe.Pointer(t.visual))), "Target.SetRoot"); err != nil {
		return err
	}
	return failed(t.dcomp.call(3), "Commit")
}

// resize is Gio's own way (app/d3d11_windows.go): let go of the view, resize the buffers,
// make the view again. A window's buffers take the client area's size; the composition
// swap chain is told.
func (t *target) resize(w, h int) error {
	if t.size == image.Pt(w, h) {
		return nil
	}
	t.rtv.release()
	t.rtv = nil
	var aw, ah uintptr
	if t.composition {
		aw, ah = uintptr(w), uintptr(h)
	}
	if err := failed(t.swap.call(13, 0, aw, ah, 0, 0), "ResizeBuffers"); err != nil {
		return err
	}
	t.size = image.Pt(w, h)
	return t.view()
}

// present draws one frame of Gio operations, cleared to fully transparent, and shows it.
func (t *target) present(ops *op.Ops) error {
	t.gpu.Clear(color.NRGBA{})
	if err := t.gpu.Frame(ops, gpu.Direct3D11RenderTarget{RenderTarget: unsafe.Pointer(t.rtv)}, t.size); err != nil {
		return err
	}
	hr := t.swap.call(8, 1, 0)
	switch uint32(hr) {
	case dxgiErrDeviceRemoved, dxgiErrDeviceReset:
		return ErrLost
	case dxgiStatusOccluded:
		return nil
	}
	return failed(hr, "Present")
}

func (t *target) release() {
	if t.gpu != nil {
		t.gpu.Release()
		t.gpu = nil
	}
	t.visual.release()
	t.root.release()
	t.dcomp.release()
	t.rtv.release()
	t.swap.release()
	t.visual, t.root, t.dcomp, t.rtv, t.swap = nil, nil, nil, nil, nil
}
