package gpu

import (
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"syscall"
	"unsafe"
)

// On Windows: syscall.LoadDLL for wgpu_native.dll (no C compiler, no cgo).

type library struct {
	dll   *syscall.DLL
	procs sync.Map
}

func loadLib() (library, error) {
	// Look beside the exe first, then WGPU_NATIVE_PATH.
	name := "wgpu_native.dll"
	if p := os.Getenv("WGPU_NATIVE_PATH"); p != "" {
		name = p
	} else if exe, err := os.Executable(); err == nil {
		if p := filepath.Join(filepath.Dir(exe), "wgpu_native.dll"); fileExists(p) {
			name = p
		}
	}
	dll, err := syscall.LoadDLL(name)
	if err != nil {
		return library{}, fmt.Errorf("gpu: %w", err)
	}
	return library{dll: dll}, nil
}

func fileExists(p string) bool { _, err := os.Stat(p); return err == nil }

//go:uintptrescapes
func call(name string, args ...uintptr) uintptr {
	p := lib.proc(name)
	r, _, _ := p.Call(args...)
	return r
}

func (l *library) proc(name string) *syscall.Proc {
	if v, ok := l.procs.Load(name); ok {
		return v.(*syscall.Proc)
	}
	p, err := l.dll.FindProc(name)
	if err != nil {
		panic("gpu: " + name + ": " + err.Error())
	}
	l.procs.Store(name, p)
	return p
}

// ---- callbacks (Windows: syscall.NewCallback) ----------------------------------------

func requestAdapter(inst Instance, opts *WRequestAdapterOptions) (Adapter, error) {
	type result struct {
		status  RequestStatus
		adapter Handle
		msg     StringView
	}
	var res result
	cb := syscall.NewCallback(func(status RequestStatus, adapter uintptr, msg StringView, u1, u2 uintptr) uintptr {
		r := (*result)(unsafe.Pointer(u1))
		r.status, r.adapter, r.msg = status, adapter, msg
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	var op uintptr
	if opts != nil {
		op = uintptr(unsafe.Pointer(opts))
	}
	call("wgpuInstanceRequestAdapter", inst.h, op, uintptr(unsafe.Pointer(&info)))
	for res.adapter == 0 && res.status == 0 {
		inst.ProcessEvents()
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
	}
	var res result
	cb := syscall.NewCallback(func(status RequestStatus, device uintptr, msg StringView, u1, u2 uintptr) uintptr {
		r := (*result)(unsafe.Pointer(u1))
		r.status, r.device, r.msg = status, device, msg
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	var dp uintptr
	if desc != nil {
		dp = uintptr(unsafe.Pointer(desc))
	} else {
		d := DeviceDescriptorInit()
		dp = uintptr(unsafe.Pointer(&d))
	}
	call("wgpuAdapterRequestDevice", a.h, dp, uintptr(unsafe.Pointer(&info)))
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
	}
	var res result
	cb := syscall.NewCallback(func(status MapAsyncStatus, msg StringView, u1, u2 uintptr) uintptr {
		r := (*result)(unsafe.Pointer(u1))
		r.status, r.msg = status, msg
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	call("wgpuBufferMapAsync", b.h, uintptr(mode), uintptr(offset), uintptr(size), uintptr(unsafe.Pointer(&info)))
	dev.Poll(true)
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
	}
	var res result
	cb := syscall.NewCallback(func(status PopErrorStatus, typ ErrorType, msg StringView, u1, u2 uintptr) uintptr {
		r := (*result)(unsafe.Pointer(u1))
		r.status, r.typ, r.msg = status, typ, msg
		return 0
	})
	info := WCallbackInfo{Mode: CallbackModeAllowEvents, Callback: cb, Userdata1: uintptr(unsafe.Pointer(&res))}
	call("wgpuDevicePopErrorScope", d.h, uintptr(unsafe.Pointer(&info)))
	inst.ProcessEvents()
	return res.typ, InfoString(res.msg)
}
