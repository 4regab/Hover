package core

import (
	"io"
	"os"

	"golang.org/x/sys/windows"
)

// ReadFile is os.ReadFile, opened the way Rust's std::fs::read opens: letting others rename
// or delete the file while it is read. Go opens a file with only FILE_SHARE_READ and
// FILE_SHARE_WRITE, so on Windows a save that renames a new file over one being read fails
// ("being used by another process") and the save is lost. Use it for files that Hover also
// replaces by renaming (the history, the sealed stores).
func ReadFile(path string) ([]byte, error) {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	f := os.NewFile(uintptr(h), path)
	defer f.Close()
	return io.ReadAll(f)
}
