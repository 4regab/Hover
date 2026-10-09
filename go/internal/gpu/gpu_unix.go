//go:build linux || darwin

package gpu

import (
	"fmt"
	"os"
	"runtime"
	"sync"
	"unsafe"

	"github.com/go-webgpu/goffi/ffi"
	"github.com/go-webgpu/goffi/types"
)

// On Linux and macOS: goffi for calling and for callbacks (already a dependency through
// Gio). The library is loaded once at Init().

type proc struct {
	ptr unsafe.Pointer
	cif types.CallInterface
	mu  sync.Mutex
	ok  bool
}

type library struct {
	handle unsafe.Pointer
	procs  sync.Map
}

func loadLib() (library, error) {
	name := "libwgpu_native.so"
	if runtime.GOOS == "darwin" {
		name = "libwgpu_native.dylib"
	}
	if p := os.Getenv("WGPU_NATIVE_PATH"); p != "" {
		name = p
	}
	h, err := ffi.LoadLibrary(name)
	if err != nil {
		return library{}, fmt.Errorf("gpu: %w", err)
	}
	return library{handle: h}, nil
}

// callbackInfoType is the type descriptor for WCallbackInfo (40 bytes, 5 fields of 8 bytes).
// On System V AMD64 a struct > 16 bytes is passed via hidden pointer, so the CIF must
// know the third argument is a struct, not a pointer.
var callbackInfoType = &types.TypeDescriptor{
	Size: 40, Alignment: 8, Kind: types.StructType,
	Members: []*types.TypeDescriptor{
		types.PointerTypeDescriptor,                     // nextInChain
		{Size: 4, Alignment: 4, Kind: types.UInt32Type}, // mode (+ 4 pad)
		types.PointerTypeDescriptor,                     // callback
		types.PointerTypeDescriptor,                     // userdata1
		types.PointerTypeDescriptor,                     // userdata2
	},
}

func call(name string, args ...uintptr) uintptr {
	p := lib.getProc(name)
	p.mu.Lock()
	if !p.ok {
		at := make([]*types.TypeDescriptor, len(args))
		for i := range at {
			at[i] = types.PointerTypeDescriptor
		}
		if err := ffi.PrepareCallInterface(&p.cif, types.DefaultConvention(), types.PointerTypeDescriptor, at); err != nil {
			panic("gpu: PrepareCallInterface " + name + ": " + err.Error())
		}
		p.ok = true
	}
	p.mu.Unlock()
	ap := make([]unsafe.Pointer, len(args))
	for i := range args {
		ap[i] = unsafe.Pointer(&args[i])
	}
	var ret uintptr
	ffi.CallFunction(&p.cif, p.ptr, unsafe.Pointer(&ret), ap)
	return ret
}

// callWithInfo calls a function whose last argument is a WCallbackInfo struct by value.
// The ptrArgs are pointer-valued arguments; the struct is the last argument and goffi
// reads its 40 bytes to pass them by value (on SysV AMD64 >16 bytes → hidden pointer).
func callWithInfo(name string, ptrArgs []uintptr, info *WCallbackInfo) uintptr {
	p := lib.getProc(name)
	n := len(ptrArgs) + 1
	at := make([]*types.TypeDescriptor, n)
	for i := range ptrArgs {
		at[i] = types.PointerTypeDescriptor
	}
	at[n-1] = callbackInfoType
	var cif types.CallInterface
	if err := ffi.PrepareCallInterface(&cif, types.DefaultConvention(), types.PointerTypeDescriptor, at); err != nil {
		panic("gpu: PrepareCallInterface " + name + ": " + err.Error())
	}
	ap := make([]unsafe.Pointer, n)
	for i := range ptrArgs {
		ap[i] = unsafe.Pointer(&ptrArgs[i])
	}
	// For a struct arg, avalue[i] points to the struct's first byte (not to a pointer to it).
	ap[n-1] = unsafe.Pointer(info)
	var ret uintptr
	ffi.CallFunction(&cif, p.ptr, unsafe.Pointer(&ret), ap)
	return ret
}

func (l *library) getProc(name string) *proc {
	if v, ok := l.procs.Load(name); ok {
		return v.(*proc)
	}
	ptr, err := ffi.GetSymbol(l.handle, name)
	if err != nil {
		panic("gpu: " + name + ": " + err.Error())
	}
	p := &proc{ptr: ptr}
	l.procs.Store(name, p)
	return p
}

// ---- callbacks (goffi) ---------------------------------------------------------------

func requestAdapter(inst Instance, opts *WRequestAdapterOptions) (Adapter, error) {
	type result struct {
		status  RequestStatus
		adapter Handle
		msg     StringView
		done    bool
	}
	var res result
	cb := ffi.NewCallback(func(status uintptr, adapter uintptr, msg0 uintptr, msg1 uintptr, u1 uintptr, u2 uintptr) uintptr {
		r := (*result)(noescape(u1)) //nolint:govet
		r.status = RequestStatus(status)
		r.adapter = adapter
		r.msg = StringView{msg0, msg1}
		r.done = true
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	var op uintptr
	if opts != nil {
		op = uintptr(unsafe.Pointer(opts))
	}
	callWithInfo("wgpuInstanceRequestAdapter", []uintptr{inst.h, op}, &info)
	for i := 0; i < 200 && !res.done; i++ {
		inst.ProcessEvents()
		runtime.Gosched()
	}
	runtime.KeepAlive(&res)
	if !res.done {
		return Adapter{}, fmt.Errorf("gpu: adapter callback never fired after 200 ProcessEvents calls")
	}
	if res.status != RequestStatusSuccess {
		return Adapter{}, fmt.Errorf("gpu: adapter: status %d: %s", res.status, InfoString(res.msg))
	}
	return Adapter{res.adapter}, nil
}

func requestDevice(inst Instance, a Adapter, desc *WDeviceDescriptor) (Device, Queue, error) {
	type result struct {
		status RequestStatus
		device Handle
		msg    StringView
		done   bool
	}
	var res result
	cb := ffi.NewCallback(func(status uintptr, device uintptr, msg0 uintptr, msg1 uintptr, u1 uintptr, u2 uintptr) uintptr {
		r := (*result)(unsafe.Pointer(u1))
		r.status = RequestStatus(status)
		r.device = device
		r.msg = StringView{msg0, msg1}
		r.done = true
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	d := DeviceDescriptorInit()
	if desc != nil {
		d = *desc
	}
	callWithInfoDesc("wgpuAdapterRequestDevice", a.h, &d, &info)
	runtime.KeepAlive(&d)
	for i := 0; i < 200 && !res.done; i++ {
		inst.ProcessEvents()
		runtime.Gosched()
	}
	if !res.done {
		return Device{}, Queue{}, fmt.Errorf("gpu: device callback never fired")
	}
	if res.status != RequestStatusSuccess {
		return Device{}, Queue{}, fmt.Errorf("gpu: device: status %d: %s", res.status, InfoString(res.msg))
	}
	q := call("wgpuDeviceGetQueue", res.device)
	return Device{res.device}, Queue{q}, nil
}

func bufferMapAsync(b Buffer, dev Device, mode MapMode, offset, size uint64) error {
	type result struct {
		status MapAsyncStatus
		msg    StringView
		done   bool
	}
	var res result
	cb := ffi.NewCallback(func(status uintptr, msg0 uintptr, msg1 uintptr, u1 uintptr, u2 uintptr) uintptr {
		r := (*result)(noescape(u1)) //nolint:govet
		r.status = MapAsyncStatus(status)
		r.msg = StringView{msg0, msg1}
		r.done = true
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	callWithInfo("wgpuBufferMapAsync", []uintptr{b.h, uintptr(mode), uintptr(offset), uintptr(size)}, &info)
	dev.Poll(true)
	for !res.done {
		dev.Poll(true)
		runtime.Gosched()
	}
	if res.status != MapAsyncStatusSuccess {
		return fmt.Errorf("gpu: map: status %d: %s", res.status, InfoString(res.msg))
	}
	return nil
}

func popErrorScope(d Device, inst Instance) (ErrorType, string) {
	type result struct {
		status PopErrorStatus
		typ    ErrorType
		msg    StringView
		done   bool
	}
	var res result
	cb := ffi.NewCallback(func(status uintptr, typ uintptr, msg0 uintptr, msg1 uintptr, u1 uintptr, u2 uintptr) uintptr {
		r := (*result)(noescape(u1)) //nolint:govet
		r.status = PopErrorStatus(status)
		r.typ = ErrorType(typ)
		r.msg = StringView{msg0, msg1}
		r.done = true
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	callWithInfo("wgpuDevicePopErrorScope", []uintptr{d.h}, &info)
	inst.ProcessEvents()
	for !res.done {
		inst.ProcessEvents()
		runtime.Gosched()
	}
	return res.typ, InfoString(res.msg)
}

// callWithInfoDesc calls wgpuAdapterRequestDevice with the descriptor pointer and
// callback info as struct. The descriptor pointer is kept as a Go pointer until the
// call completes, to prevent the GC from moving it.
//
//go:noinline
func callWithInfoDesc(name string, handle uintptr, desc *WDeviceDescriptor, info *WCallbackInfo) {
	dp := uintptr(unsafe.Pointer(desc))
	p := lib.getProc(name)
	at := []*types.TypeDescriptor{types.PointerTypeDescriptor, types.PointerTypeDescriptor, callbackInfoType}
	var cif types.CallInterface
	if err := ffi.PrepareCallInterface(&cif, types.DefaultConvention(), types.PointerTypeDescriptor, at); err != nil {
		panic("gpu: " + name + ": " + err.Error())
	}
	ap := []unsafe.Pointer{unsafe.Pointer(&handle), unsafe.Pointer(&dp), unsafe.Pointer(info)}
	var ret uintptr
	ffi.CallFunction(&cif, p.ptr, unsafe.Pointer(&ret), ap)
	runtime.KeepAlive(desc)
}
