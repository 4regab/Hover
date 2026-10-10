//go:build windows

package voice

import (
	"os"
	"syscall"
)

// openShared opens the file so it can still be deleted while open (Windows): a cancelled
// upload that hasn't let go yet never keeps the recording on disk.
func openShared(p string) (*os.File, error) {
	name, err := syscall.UTF16PtrFromString(p)
	if err != nil {
		return nil, err
	}
	// FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
	h, err := syscall.CreateFile(name, syscall.GENERIC_READ, 0x1|0x2|0x4, nil, syscall.OPEN_EXISTING, syscall.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return nil, err
	}
	return os.NewFile(uintptr(h), p), nil
}
