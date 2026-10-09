package core

import (
	"errors"
	"io"
	"os"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

// On Windows Hover keeps files by writing a temporary file and renaming it over the real
// one (the history, the stores, settings.json). Rust's std does this where Go's does not:
//
//   - it opens a file letting others rename or delete it meanwhile (Go opens with only
//     FILE_SHARE_READ and FILE_SHARE_WRITE), and
//   - when MoveFileExW answers "access denied" it renames again with POSIX semantics, which
//     works while another handle is open (os.Rename has no second try).
//
// Without them a save that met a reader was lost: "rename ...index.dat.tmp: The process
// cannot access the file because it is being used by another process", and, once reads
// allowed it, "Access is denied". readfile_windows_test.go shows both.

// Open is os.Open, as Rust's File::open opens: others may rename or delete the file.
func Open(path string) (*os.File, error) {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	return os.NewFile(uintptr(h), path), nil
}

// ReadFile is os.ReadFile, opened with Open.
func ReadFile(path string) ([]byte, error) {
	f, err := Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	return io.ReadAll(f)
}

// Rename is std::fs::rename: MoveFileExW replacing what is there, and when that is refused
// ("access denied": the file is open elsewhere, or read only) a rename with POSIX
// semantics, which does not mind.
//
// Both are tried again for up to two seconds while the file stays locked: an antivirus
// scanning the temporary file just written holds it without FILE_SHARE_DELETE, and then
// neither rename can work (CI's Windows runner lost a settings.json write that way).
// Go's own toolchain retries the same way (cmd/go/internal/robustio). Rust doesn't, so
// here the Go build keeps a write the Rust one would lose.
func Rename(from, to string) error {
	var err error
	for wait := time.Millisecond; ; wait *= 2 {
		if err = renameOnce(from, to); err == nil || !locked(err) || wait > time.Second {
			return err
		}
		time.Sleep(wait)
	}
}

func renameOnce(from, to string) error {
	err := os.Rename(from, to)
	if err == nil || !errors.Is(err, windows.ERROR_ACCESS_DENIED) {
		return err
	}
	if posixRename(from, to) == nil {
		return nil
	}
	return err
}

// locked: the error is one another handle on the file causes.
func locked(err error) bool {
	return errors.Is(err, windows.ERROR_ACCESS_DENIED) || errors.Is(err, windows.ERROR_SHARING_VIOLATION)
}

// posixRename renames with FileRenameInfoEx and POSIX semantics, as Rust's rename does
// after MoveFileExW is refused.
func posixRename(from, to string) error {
	old, err := windows.UTF16PtrFromString(from)
	if err != nil {
		return err
	}
	h, err := windows.CreateFile(old, windows.DELETE, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT|windows.FILE_FLAG_BACKUP_SEMANTICS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(h)
	name, err := windows.UTF16FromString(to)
	if err != nil {
		return err
	}
	name = name[:len(name)-1] // without the NUL: FileNameLength says how long it is
	// FILE_RENAME_INFO: Flags (4 bytes, padded to 8), RootDirectory (a handle), FileNameLength
	// (4 bytes), then the name. The name starts at the offset of the first field after those.
	type renameInfo struct {
		Flags          uint32
		RootDirectory  windows.Handle
		FileNameLength uint32
		FileName       [1]uint16
	}
	offset := unsafe.Offsetof(renameInfo{}.FileName)
	size := int(offset) + len(name)*2 + 2
	// Words, not bytes, so the struct that starts it is aligned.
	mem := make([]uint64, (size+7)/8)
	info := (*renameInfo)(unsafe.Pointer(&mem[0]))
	info.Flags = windows.FILE_RENAME_REPLACE_IF_EXISTS | windows.FILE_RENAME_POSIX_SEMANTICS
	info.FileNameLength = uint32(len(name) * 2)
	copy(unsafe.Slice((*uint16)(unsafe.Add(unsafe.Pointer(&mem[0]), offset)), len(name)), name)
	return windows.SetFileInformationByHandle(h, windows.FileRenameInfoEx, (*byte)(unsafe.Pointer(&mem[0])), uint32(size))
}
