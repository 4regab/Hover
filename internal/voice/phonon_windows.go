//go:build windows

package voice

import (
	"os"
	"path/filepath"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	kernel32         = windows.NewLazySystemDLL("kernel32.dll")
	pIsWow64Process2 = kernel32.NewProc("IsWow64Process2")
	pDiskFree        = kernel32.NewProc("GetDiskFreeSpaceExW")
)

// arm64 : an x64 build under Windows on Arm's emulation still says amd64; the machine doesn't.
func arm64() bool {
	if pIsWow64Process2.Find() != nil {
		return false
	}
	var p, n uint16
	h, _ := windows.GetCurrentProcess()
	r, _, _ := pIsWow64Process2.Call(uintptr(h), uintptr(unsafe.Pointer(&p)), uintptr(unsafe.Pointer(&n)))
	return r != 0 && n == 0xAA64
}

func freeOn(dir string) (uint64, bool) {
	w, err := windows.UTF16PtrFromString(dir)
	if err != nil {
		return 0, false
	}
	var avail uint64
	r, _, _ := pDiskFree.Call(uintptr(unsafe.Pointer(w)), uintptr(unsafe.Pointer(&avail)), 0, 0)
	return avail, r != 0
}

// osUnsupported is why Phonon cannot run on this Windows, before anything is downloaded.
func osUnsupported() string {
	if arm64() {
		return "Phonon doesn’t run on Windows on Arm."
	}
	root := os.Getenv("SystemRoot")
	if root == "" {
		root = `C:\Windows`
	}
	sys := filepath.Join(root, "System32")
	for _, d := range []string{"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"} {
		if st, err := os.Stat(filepath.Join(sys, d)); err != nil || !st.Mode().IsRegular() {
			return vcMsg
		}
	}
	return ""
}

func pythonExe(home string) string { return filepath.Join(home, "python", "python.exe") }

const nullFile = "nul"
