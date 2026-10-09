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

// Linux and macOS: libwgpu_native through goffi. System V passes a 16-byte string view
// in two registers, so a callback gets it as two arguments; a callback-info struct (40
// bytes) is a struct argument, described to goffi as one.

var (
	symbols [fnCount]unsafe.Pointer
	cifs    [fnCount]*types.CallInterface
	cifMu   sync.Mutex
)

func load() error {
	name := os.Getenv("WGPU_NATIVE_PATH")
	if name == "" {
		name = "libwgpu_native.so"
		if runtime.GOOS == "darwin" {
			name = "libwgpu_native.dylib"
		}
	}
	h, err := ffi.LoadLibrary(name)
	if err != nil {
		return fmt.Errorf("gpu: %w", err)
	}
	for i, n := range fnNames {
		p, err := ffi.GetSymbol(h, n)
		if err != nil {
			return fmt.Errorf("gpu: %s has no %s: %w", name, n, err)
		}
		symbols[i] = p
	}
	cbAdapter = ffi.NewCallback(func(status, adapter, msgData, msgLen, u1, _ uintptr) uintptr {
		onAdapter(status, adapter, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbDevice = ffi.NewCallback(func(status, device, msgData, msgLen, u1, _ uintptr) uintptr {
		onDevice(status, device, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbMap = ffi.NewCallback(func(status, msgData, msgLen, u1, _ uintptr) uintptr {
		onMap(status, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbPopErrorScope = ffi.NewCallback(func(status, typ, msgData, msgLen, u1, _ uintptr) uintptr {
		onPopErrorScope(status, typ, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbUncaptured = ffi.NewCallback(func(_, typ, msgData, msgLen, u1, _ uintptr) uintptr {
		onUncaptured(typ, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbDeviceLost = ffi.NewCallback(func(_, reason, msgData, msgLen, u1, _ uintptr) uintptr {
		onDeviceLost(reason, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	return nil
}

// infoType describes a callback-info struct to goffi.
var infoType = &types.TypeDescriptor{
	Size: 40, Alignment: 8, Kind: types.StructType,
	Members: []*types.TypeDescriptor{
		types.PointerTypeDescriptor,
		types.UInt32TypeDescriptor,
		types.PointerTypeDescriptor,
		types.PointerTypeDescriptor,
		types.PointerTypeDescriptor,
	},
}

// cif is the call interface for fn: n pointer-sized arguments, then the callback-info
// struct when withInfo. A function always takes the same arguments, so it is made once.
func cif(fn, n int, withInfo bool) *types.CallInterface {
	cifMu.Lock()
	defer cifMu.Unlock()
	if c := cifs[fn]; c != nil {
		return c
	}
	at := make([]*types.TypeDescriptor, n)
	for i := range at {
		at[i] = types.PointerTypeDescriptor
	}
	if withInfo {
		at = append(at, infoType)
	}
	c := &types.CallInterface{}
	if err := ffi.PrepareCallInterface(c, types.DefaultConvention(), types.PointerTypeDescriptor, at); err != nil {
		panic("gpu: " + fnNames[fn] + ": " + err.Error())
	}
	cifs[fn] = c
	return c
}

// The callers pass addresses of pinned memory only (see pins).
func call(fn int, args ...uintptr) uintptr {
	av := make([]unsafe.Pointer, len(args))
	for i := range args {
		av[i] = unsafe.Pointer(&args[i])
	}
	var ret uintptr
	if _, err := ffi.CallFunction(cif(fn, len(args), false), symbols[fn], unsafe.Pointer(&ret), av); err != nil {
		panic("gpu: " + fnNames[fn] + ": " + err.Error())
	}
	return ret
}

// callWithInfo calls a function whose last parameter is a callback-info struct by value;
// info is the pinned struct's address, and goffi copies its 40 bytes.
func callWithInfo(fn int, args []uintptr, info uintptr) uintptr {
	av := make([]unsafe.Pointer, len(args)+1)
	for i := range args {
		av[i] = unsafe.Pointer(&args[i])
	}
	av[len(args)] = unsafe.Pointer(cPtr[byte](info))
	var ret uintptr
	if _, err := ffi.CallFunction(cif(fn, len(args), true), symbols[fn], unsafe.Pointer(&ret), av); err != nil {
		panic("gpu: " + fnNames[fn] + ": " + err.Error())
	}
	return ret
}

// All backends (Vulkan or GL on Linux, Metal on a Mac).
func instanceBackends() uint64 { return 0 }
