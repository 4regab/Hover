//go:build linux || darwin

package gpu

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"unsafe"

	"github.com/ebitengine/purego"
)

// Linux and macOS: libwgpu_native through purego, which works with or without cgo (the
// Linux app is a cgo build, as Gio's EGL needs; goffi cannot link into one). System V
// passes a 16-byte string view in two registers, so a callback gets it as two arguments; a
// callback-info struct (40 bytes) is passed in memory, on the stack, which is where the
// arguments after the sixth go: so it is sent as five more arguments, after enough zeros to
// fill the six registers.

var symbols [fnCount]uintptr

func load() error {
	name := os.Getenv("WGPU_NATIVE_PATH")
	if name == "" {
		name = "libwgpu_native.so"
		if runtime.GOOS == "darwin" {
			name = "libwgpu_native.dylib"
		}
	}
	// Without WGPU_NATIVE_PATH: beside the program, in the package's lib folder, then the
	// system's.
	if os.Getenv("WGPU_NATIVE_PATH") == "" {
		if exe, err := os.Executable(); err == nil {
			dir := filepath.Dir(exe)
			for _, p := range []string{filepath.Join(dir, name), filepath.Join(dir, "..", "lib", "hover", name)} {
				if _, err := os.Stat(p); err == nil {
					name = p
					break
				}
			}
		}
	}
	h, err := purego.Dlopen(name, purego.RTLD_NOW|purego.RTLD_GLOBAL)
	if err != nil {
		return fmt.Errorf("gpu: %w", err)
	}
	for i, n := range fnNames {
		p, err := purego.Dlsym(h, n)
		if err != nil {
			return fmt.Errorf("gpu: %s has no %s: %w", name, n, err)
		}
		symbols[i] = p
	}
	cbAdapter = purego.NewCallback(func(status, adapter, msgData, msgLen, u1, _ uintptr) uintptr {
		onAdapter(status, adapter, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbDevice = purego.NewCallback(func(status, device, msgData, msgLen, u1, _ uintptr) uintptr {
		onDevice(status, device, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbMap = purego.NewCallback(func(status, msgData, msgLen, u1, _ uintptr) uintptr {
		onMap(status, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbPopErrorScope = purego.NewCallback(func(status, typ, msgData, msgLen, u1, _ uintptr) uintptr {
		onPopErrorScope(status, typ, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbUncaptured = purego.NewCallback(func(_, typ, msgData, msgLen, u1, _ uintptr) uintptr {
		onUncaptured(typ, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	cbDeviceLost = purego.NewCallback(func(_, reason, msgData, msgLen, u1, _ uintptr) uintptr {
		onDeviceLost(reason, goString(stringView{msgData, msgLen}), u1)
		return 0
	})
	return nil
}

// The callers pass addresses of pinned memory only (see pins).
func call(fn int, args ...uintptr) uintptr {
	r, _, _ := purego.SyscallN(symbols[fn], args...)
	return r
}

// callWithInfo calls a function whose last parameter is a callback-info struct by value;
// info is the pinned struct's address.
func callWithInfo(fn int, args []uintptr, info uintptr) uintptr {
	if len(args) > 6 {
		panic("gpu: " + fnNames[fn] + ": too many arguments for a struct on the stack")
	}
	all := make([]uintptr, 6, 11)
	copy(all, args)
	words := (*[5]uintptr)(unsafe.Pointer(cPtr[byte](info)))
	all = append(all, words[:]...)
	r, _, _ := purego.SyscallN(symbols[fn], all...)
	return r
}

// All backends (Vulkan or GL on Linux, Metal on a Mac).
func instanceBackends() uint64 { return 0 }
