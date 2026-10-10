package gpu

import (
	"fmt"
	"os"
	"path/filepath"
	"syscall"
)

// Windows: wgpu_native.dll through syscall. The x64 calling convention passes a struct
// that isn't 1, 2, 4 or 8 bytes by reference, so a callback-info struct goes as its
// address, and a callback's string view arrives as a pointer to one.

var procs [fnCount]uintptr

func load() error {
	name := os.Getenv("WGPU_NATIVE_PATH")
	if name == "" {
		name = "wgpu_native.dll"
		if exe, err := os.Executable(); err == nil {
			if p := filepath.Join(filepath.Dir(exe), "wgpu_native.dll"); exists(p) {
				name = p
			}
		}
	}
	dll, err := syscall.LoadDLL(name)
	if err != nil {
		return fmt.Errorf("gpu: %w", err)
	}
	for i, n := range fnNames {
		p, err := dll.FindProc(n)
		if err != nil {
			return fmt.Errorf("gpu: %s has no %s: %w", name, n, err)
		}
		procs[i] = p.Addr()
	}
	cbAdapter = syscall.NewCallback(func(status, adapter, msg, u1, _ uintptr) uintptr {
		onAdapter(status, adapter, viewAt(msg), u1)
		return 0
	})
	cbDevice = syscall.NewCallback(func(status, device, msg, u1, _ uintptr) uintptr {
		onDevice(status, device, viewAt(msg), u1)
		return 0
	})
	cbMap = syscall.NewCallback(func(status, msg, u1, _ uintptr) uintptr {
		onMap(status, viewAt(msg), u1)
		return 0
	})
	cbPopErrorScope = syscall.NewCallback(func(status, typ, msg, u1, _ uintptr) uintptr {
		onPopErrorScope(status, typ, viewAt(msg), u1)
		return 0
	})
	cbUncaptured = syscall.NewCallback(func(_, typ, msg, u1, _ uintptr) uintptr {
		onUncaptured(typ, viewAt(msg), u1)
		return 0
	})
	cbDeviceLost = syscall.NewCallback(func(_, reason, msg, u1, _ uintptr) uintptr {
		onDeviceLost(reason, viewAt(msg), u1)
		return 0
	})
	return nil
}

func exists(p string) bool { _, err := os.Stat(p); return err == nil }

// viewAt reads the string view a callback was handed the address of.
func viewAt(a uintptr) string {
	if a == 0 {
		return ""
	}
	return goString(*cPtr[stringView](a))
}

// The callers pass addresses of pinned memory only (see pins), so a uintptr here never
// stands for a Go object the GC could move or free during the call.
func call(fn int, args ...uintptr) uintptr {
	r, _, _ := syscall.SyscallN(procs[fn], args...)
	return r
}

// callWithInfo calls a function whose last parameter is a callback-info struct by value:
// on x64 Windows that is its address.
func callWithInfo(fn int, args []uintptr, info uintptr) uintptr {
	return call(fn, append(args, info)...)
}

func instanceBackends() uint64 { return instanceBackendDX12 }
