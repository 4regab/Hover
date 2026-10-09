//go:build !windows

package voice

import (
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
)

func freeOn(dir string) (uint64, bool) {
	var s syscall.Statfs_t
	if syscall.Statfs(dir, &s) != nil {
		return 0, false
	}
	return uint64(s.Bavail) * uint64(s.Bsize), true
}

// glibc is the C library's version; PyTorch's Linux wheels are manylinux_2_28.
func glibc() (int, int) {
	out, err := exec.Command("getconf", "GNU_LIBC_VERSION").Output()
	if err != nil {
		return 99, 0 // musl or unknown: let the check below the install say so
	}
	f := strings.Fields(string(out))
	if len(f) < 2 {
		return 99, 0
	}
	p := strings.Split(f[1], ".")
	a, _ := strconv.Atoi(p[0])
	b := 0
	if len(p) > 1 {
		b, _ = strconv.Atoi(p[1])
	}
	return a, b
}

func osUnsupported() string {
	if runtime.GOOS == "linux" {
		if a, b := glibc(); a < 2 || (a == 2 && b < 28) {
			return "Phonon needs glibc 2.28 or newer (Ubuntu 20.04, Debian 10, Fedora 29 or later)."
		}
	}
	return ""
}

func pythonExe(home string) string { return filepath.Join(home, "python", "bin", "python3") }

const nullFile = "/dev/null"
