//go:build windows

package screen

import (
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"os"
	"path/filepath"
	"sync"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"

	"github.com/4regab/Hover/go/internal/core"
)

var (
	user32 = windows.NewLazySystemDLL("user32.dll")
	gdi32  = windows.NewLazySystemDLL("gdi32.dll")

	pGetSystemMetrics    = user32.NewProc("GetSystemMetrics")
	pSystemParametersInf = user32.NewProc("SystemParametersInfoW")
	pGetSysColor         = user32.NewProc("GetSysColor")
	pEnumWindows         = user32.NewProc("EnumWindows")
	pIsWindowVisible     = user32.NewProc("IsWindowVisible")
	pIsIconic            = user32.NewProc("IsIconic")
	pGetWindowThreadPID  = user32.NewProc("GetWindowThreadProcessId")
	pGetWindowRect       = user32.NewProc("GetWindowRect")
	pPrintWindow         = user32.NewProc("PrintWindow")
	pGetDC               = user32.NewProc("GetDC")
	pReleaseDC           = user32.NewProc("ReleaseDC")
	pCreateCompatibleDC  = gdi32.NewProc("CreateCompatibleDC")
	pCreateDIBSection    = gdi32.NewProc("CreateDIBSection")
	pSelectObject        = gdi32.NewProc("SelectObject")
	pDeleteObject        = gdi32.NewProc("DeleteObject")
	pDeleteDC            = gdi32.NewProc("DeleteDC")
	pBitBlt              = gdi32.NewProc("BitBlt")
)

const (
	smCxScreen         = 0
	smCyScreen         = 1
	spiGetDeskWallpape = 0x0073
	colorDesktop       = 1
	srcCopy            = 0x00CC0020
	captureBlt         = 0x40000000
	pwRenderFull       = 2
)

type rect struct{ Left, Top, Right, Bottom int32 }

type bitmapInfoHeader struct {
	Size          uint32
	Width, Height int32
	Planes, Bits  uint16
	Compression   uint32
	SizeImage     uint32
	XPels, YPels  int32
	ClrUsed       uint32
	ClrImportant  uint32
}

func supported() bool { return true }

func screenSize() (int, int) {
	w, _, _ := pGetSystemMetrics.Call(smCxScreen)
	h, _, _ := pGetSystemMetrics.Call(smCyScreen)
	return max(int(w), 1), max(int(h), 1)
}

// desktop is the desktop picture (or Windows' own copy of it), drawn to fill the screen; the
// desktop's colour when there is none to read.
func desktop() (*image.RGBA, error) {
	w, h := screenSize()
	var buf [520]uint16
	path := ""
	if r, _, _ := pSystemParametersInf.Call(spiGetDeskWallpape, uintptr(len(buf)), uintptr(unsafe.Pointer(&buf[0])), 0); r != 0 {
		path = windows.UTF16ToString(buf[:])
	}
	var files []string
	files = append(files, path)
	if a := os.Getenv("APPDATA"); a != "" {
		files = append(files, filepath.Join(a, "Microsoft", "Windows", "Themes", "TranscodedWallpaper"))
	}
	for _, f := range files {
		if f == "" {
			continue
		}
		fh, err := os.Open(f)
		if err != nil {
			continue
		}
		img, _, err := image.Decode(fh)
		fh.Close()
		if err != nil {
			core.Logf("screen: couldn’t read the desktop picture — %v", err)
			continue
		}
		return Cover(img, w, h), nil
	}
	c, _, _ := pGetSysColor.Call(colorDesktop)
	return Plain(w, h, [3]uint8{uint8(c), uint8(c >> 8), uint8(c >> 16)}), nil
}

// dib makes a top-down 32-bit bitmap w x h to draw into, and gives its pixels and a way to
// let it go.
func dib(dc uintptr, w, h int) (bmp uintptr, bits unsafe.Pointer) {
	hdr := bitmapInfoHeader{Size: uint32(unsafe.Sizeof(bitmapInfoHeader{})), Width: int32(w), Height: -int32(h), Planes: 1, Bits: 32}
	bmp, _, _ = pCreateDIBSection.Call(dc, uintptr(unsafe.Pointer(&hdr)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	return bmp, bits
}

func rgbaOf(bits unsafe.Pointer, w, h int) *image.RGBA {
	src := unsafe.Slice((*byte)(bits), w*h*4)
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for i := 0; i+3 < len(src); i += 4 {
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = src[i+2], src[i+1], src[i], 255
	}
	return img
}

// grab is one window's own picture, whatever is over it (PW_RENDERFULLCONTENT).
func grab(hwnd uintptr, w, h int) *image.RGBA {
	screen, _, _ := pGetDC.Call(0)
	defer pReleaseDC.Call(0, screen)
	mem, _, _ := pCreateCompatibleDC.Call(screen)
	defer pDeleteDC.Call(mem)
	bmp, bits := dib(mem, w, h)
	if bmp == 0 || bits == nil {
		return nil
	}
	defer pDeleteObject.Call(bmp)
	old, _, _ := pSelectObject.Call(mem, bmp)
	ok, _, _ := pPrintWindow.Call(hwnd, mem, pwRenderFull)
	var out *image.RGBA
	if ok != 0 {
		out = rgbaOf(bits, w, h)
	}
	pSelectObject.Call(mem, old)
	return out
}

// whole is the primary display from the screen's own picture (BitBlt with CAPTUREBLT, so
// layered windows are in it too).
func whole() (*image.RGBA, error) {
	w, h := screenSize()
	screen, _, _ := pGetDC.Call(0)
	defer pReleaseDC.Call(0, screen)
	mem, _, _ := pCreateCompatibleDC.Call(screen)
	defer pDeleteDC.Call(mem)
	bmp, bits := dib(mem, w, h)
	if bmp == 0 || bits == nil {
		return nil, errNoPicture
	}
	defer pDeleteObject.Call(bmp)
	old, _, _ := pSelectObject.Call(mem, bmp)
	defer pSelectObject.Call(mem, old)
	if ok, _, _ := pBitBlt.Call(mem, 0, 0, uintptr(w), uintptr(h), screen, 0, 0, srcCopy|captureBlt); ok == 0 {
		return nil, errNoPicture
	}
	return rgbaOf(bits, w, h), nil
}

type errString string

func (e errString) Error() string { return string(e) }

const errNoPicture = errString("Windows didn’t give a picture of the screen.")

type hit struct {
	hwnd uintptr
	r    rect
}

var enumMu sync.Mutex

func capture(apps Apps) (*image.RGBA, error) {
	canvas, err := desktop()
	if err != nil {
		return nil, err
	}
	if len(apps.Pids) == 0 {
		return canvas, nil
	}
	// EnumWindows takes a callback with no room for our own data: one capture at a time.
	enumMu.Lock()
	var hits []hit
	cb := syscall.NewCallback(func(hwnd, _ uintptr) uintptr {
		if v, _, _ := pIsWindowVisible.Call(hwnd); v == 0 {
			return 1
		}
		if v, _, _ := pIsIconic.Call(hwnd); v != 0 {
			return 1
		}
		var pid uint32
		pGetWindowThreadPID.Call(hwnd, uintptr(unsafe.Pointer(&pid)))
		var r rect
		for _, p := range apps.Pids {
			if p == pid {
				if ok, _, _ := pGetWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&r))); ok != 0 && r.Right > r.Left && r.Bottom > r.Top {
					hits = append(hits, hit{hwnd, r})
				}
				break
			}
		}
		return 1
	})
	pEnumWindows.Call(cb, 0)
	enumMu.Unlock()
	// EnumWindows runs front to back; the back ones are drawn first.
	for i := len(hits) - 1; i >= 0; i-- {
		h := hits[i]
		if img := grab(h.hwnd, int(h.r.Right-h.r.Left), int(h.r.Bottom-h.r.Top)); img != nil {
			Paste(canvas, img, int(h.r.Left), int(h.r.Top))
		}
	}
	return canvas, nil
}
