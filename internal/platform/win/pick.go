//go:build windows

package win

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	clsidFileOpenDialog = guid("{DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7}")
	iidIFileOpenDialog  = guid("{d57c7288-d4ad-4768-be02-9d969532d960}")
)

const (
	fosPickFolders    = 0x20
	fosForceFileSys   = 0x40
	sigdnFileSysPath  = 0x80058000
	coinitApartment   = 0x2
	clsctxInprocServe = 0x1
)

type filterSpec struct{ Name, Spec *uint16 }

// fileDialog is the system's IFileOpenDialog: a folder, or a file of the kinds given as
// (name, pattern) pairs. The path chosen, and false when it was cancelled.
func fileDialog(owner uintptr, folder bool, kinds [][2]string) (string, bool) {
	call(pCoInitializeEx, 0, coinitApartment)
	var d *com
	if int32(call(pCoCreateInstance, uintptr(unsafe.Pointer(&clsidFileOpenDialog)), 0, clsctxInprocServe, uintptr(unsafe.Pointer(&iidIFileOpenDialog)), uintptr(unsafe.Pointer(&d)))) < 0 {
		return "", false
	}
	defer d.release()
	if folder {
		var opts uint32
		d.call(10, uintptr(unsafe.Pointer(&opts))) // GetOptions
		d.call(9, uintptr(opts|fosPickFolders|fosForceFileSys))
	} else if len(kinds) > 0 {
		specs := make([]filterSpec, len(kinds))
		for i, k := range kinds {
			specs[i] = filterSpec{w16(k[0]), w16(k[1])}
		}
		d.call(4, uintptr(len(specs)), uintptr(unsafe.Pointer(&specs[0]))) // SetFileTypes
	}
	if int32(d.call(3, owner)) < 0 { // Show: cancelled, or failed
		return "", false
	}
	var item *com
	if int32(d.call(20, uintptr(unsafe.Pointer(&item)))) < 0 { // GetResult
		return "", false
	}
	defer item.release()
	var name *uint16
	if int32(item.call(5, sigdnFileSysPath, uintptr(unsafe.Pointer(&name)))) < 0 { // GetDisplayName
		return "", false
	}
	s := windows.UTF16PtrToString(name)
	call(pCoTaskMemFree, uintptr(unsafe.Pointer(name)))
	return s, true
}

// PickFolder is the folder picker (KiroPage.ChooseFolder).
func PickFolder(owner uintptr) (string, bool) { return fileDialog(owner, true, nil) }

// PickThemeFile is the theme file dialog (Microsoft.Win32.OpenFileDialog).
func PickThemeFile(owner uintptr) (string, bool) {
	return fileDialog(owner, false, [][2]string{{"VS Code colour theme (*.json)", "*.json"}, {"All files", "*.*"}})
}

// PickImage is an image to attach (the page's file input: PNG, JPEG, GIF, WebP).
func PickImage(owner uintptr) (string, bool) {
	return fileDialog(owner, false, [][2]string{{"Images", "*.png;*.jpg;*.jpeg;*.gif;*.webp"}})
}
