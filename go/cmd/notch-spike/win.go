//go:build windows

package main

// The Windows calls the spike needs, as tools/notch-proto/src/win.rs makes them, plus
// what the self-test uses to watch the notch from outside: screen capture, synthetic
// input and a helper window in another process standing in for "the app underneath".

import (
	"image"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	user32 = windows.NewLazySystemDLL("user32.dll")
	gdi32  = windows.NewLazySystemDLL("gdi32.dll")
	dwmapi = windows.NewLazySystemDLL("dwmapi.dll")
	shcore = windows.NewLazySystemDLL("shcore.dll")
	psapi  = windows.NewLazySystemDLL("psapi.dll")
	kernel = windows.NewLazySystemDLL("kernel32.dll")

	pGetModuleHandleW = kernel.NewProc("GetModuleHandleW")

	pRegisterClassExW           = user32.NewProc("RegisterClassExW")
	pCreateWindowExW            = user32.NewProc("CreateWindowExW")
	pDefWindowProcW             = user32.NewProc("DefWindowProcW")
	pGetMessageW                = user32.NewProc("GetMessageW")
	pTranslateMessage           = user32.NewProc("TranslateMessage")
	pDispatchMessageW           = user32.NewProc("DispatchMessageW")
	pPostQuitMessage            = user32.NewProc("PostQuitMessage")
	pSetWindowPos               = user32.NewProc("SetWindowPos")
	pShowWindow                 = user32.NewProc("ShowWindow")
	pGetWindowLongPtrW          = user32.NewProc("GetWindowLongPtrW")
	pSetWindowLongPtrW          = user32.NewProc("SetWindowLongPtrW")
	pSetLayeredWindowAttributes = user32.NewProc("SetLayeredWindowAttributes")
	pGetWindowRect              = user32.NewProc("GetWindowRect")
	pGetClientRect              = user32.NewProc("GetClientRect")
	pMonitorFromPoint           = user32.NewProc("MonitorFromPoint")
	pGetMonitorInfoW            = user32.NewProc("GetMonitorInfoW")
	pGetCursorPos               = user32.NewProc("GetCursorPos")
	pSetCursorPos               = user32.NewProc("SetCursorPos")
	pGetAsyncKeyState           = user32.NewProc("GetAsyncKeyState")
	pGetForegroundWindow        = user32.NewProc("GetForegroundWindow")
	pSetForegroundWindow        = user32.NewProc("SetForegroundWindow")
	pIsWindow                   = user32.NewProc("IsWindow")
	pGetAncestor                = user32.NewProc("GetAncestor")
	pWindowFromPoint            = user32.NewProc("WindowFromPoint")
	pRegisterHotKey             = user32.NewProc("RegisterHotKey")
	pSetTimer                   = user32.NewProc("SetTimer")
	pKillTimer                  = user32.NewProc("KillTimer")
	pSendInput                  = user32.NewProc("SendInput")
	pGetDC                      = user32.NewProc("GetDC")
	pReleaseDC                  = user32.NewProc("ReleaseDC")
	pLoadCursorW                = user32.NewProc("LoadCursorW")
	pFillRect                   = user32.NewProc("FillRect")
	pBeginPaint                 = user32.NewProc("BeginPaint")
	pEndPaint                   = user32.NewProc("EndPaint")
	pSetDpiAwarenessContext     = user32.NewProc("SetProcessDpiAwarenessContext")

	pCreateCompatibleDC = gdi32.NewProc("CreateCompatibleDC")
	pCreateDIBSection   = gdi32.NewProc("CreateDIBSection")
	pSelectObject       = gdi32.NewProc("SelectObject")
	pBitBlt             = gdi32.NewProc("BitBlt")
	pDeleteObject       = gdi32.NewProc("DeleteObject")
	pDeleteDC           = gdi32.NewProc("DeleteDC")
	pCreateSolidBrush   = gdi32.NewProc("CreateSolidBrush")

	pDwmSetWindowAttribute = dwmapi.NewProc("DwmSetWindowAttribute")
	pGetDpiForMonitor      = shcore.NewProc("GetDpiForMonitor")
	pGetProcessMemoryInfo  = psapi.NewProc("GetProcessMemoryInfo")
)

const (
	wsPopup = 0x80000000

	wsExTopmost             = 0x8
	wsExTransparent         = 0x20
	wsExToolWindow          = 0x80
	wsExLayered             = 0x80000
	wsExNoRedirectionBitmap = 0x200000
	wsExNoActivate          = 0x8000000

	swpNoSize       = 0x1
	swpNoMove       = 0x2
	swpNoActivate   = 0x10
	swpShowWindow   = 0x40
	swShow          = 5
	swShowNoActivat = 4
	lwaAlpha        = 0x2

	wmDestroy     = 0x2
	wmActivate    = 0x6
	wmPaint       = 0xF
	wmEraseBkgnd  = 0x14
	wmKeyDown     = 0x100
	wmChar        = 0x102
	wmTimer       = 0x113
	wmMouseMove   = 0x200
	wmLButtonDown = 0x201
	wmLButtonUp   = 0x202
	wmHotKey      = 0x312
	waInactive    = 0

	modAlt      = 0x1
	modNoRepeat = 0x4000

	vkLButton = 0x01
	vkRButton = 0x02
	vkMButton = 0x04
	vkMenu    = 0x12
	vkEscape  = 0x1B

	inputMouse       = 0
	inputKeyboard    = 1
	mouseLeftDown    = 0x2
	mouseLeftUp      = 0x4
	keyEventKeyUp    = 0x2
	keyEventUnicode  = 0x4
	dwmTransitionsOf = 3 // DWMWA_TRANSITIONS_FORCEDISABLED
	gaRootOwner      = 3
	idcArrow         = 32512
	srcCopy          = 0x00CC0020
	captureBlt       = 0x40000000
)

// The values Win32 passes as negative handles or indexes.
var (
	gwlExStyle  = -20
	hwndTopmost = -1
	dpiPerMonV2 = -4 // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
)

type point struct{ X, Y int32 }
type rect struct{ Left, Top, Right, Bottom int32 }

func (r rect) toImage() image.Rectangle {
	return image.Rect(int(r.Left), int(r.Top), int(r.Right), int(r.Bottom))
}

type wndClassEx struct {
	Size, Style         uint32
	WndProc             uintptr
	ClsExtra, WndExtra  int32
	Instance, Icon      uintptr
	Cursor, Background  uintptr
	MenuName, ClassName *uint16
	IconSm              uintptr
}

type msg struct {
	Hwnd    uintptr
	Message uint32
	WParam  uintptr
	LParam  uintptr
	Time    uint32
	Pt      point
	Private uint32
}

type monitorInfo struct {
	Size    uint32
	Monitor rect
	Work    rect
	Flags   uint32
}

type paintStruct struct {
	Hdc       uintptr
	Erase     int32
	Paint     rect
	Restore   int32
	IncUpdate int32
	Reserved  [32]byte
}

// INPUT is 40 bytes on amd64: the type, then a union whose largest member
// (MOUSEINPUT) is 32 bytes and 8-aligned.
type mouseInput struct {
	Typ   uint32
	_     uint32 // the union starts at 8
	Dx    int32
	Dy    int32
	Data  uint32
	Flags uint32
	Time  uint32
	_     uint32
	Extra uintptr
}

type keyInput struct {
	Typ   uint32
	_     uint32
	Vk    uint16
	Scan  uint16
	Flags uint32
	Time  uint32
	_     uint32
	Extra uintptr
	_     [8]byte
}

type bitmapInfoHeader struct {
	Size                   uint32
	Width, Height          int32
	Planes, BitCount       uint16
	Compression, SizeImage uint32
	XPels, YPels           int32
	ClrUsed, ClrImportant  uint32
}

type processMemoryCountersEx struct {
	Cb                         uint32
	PageFaultCount             uint32
	PeakWorkingSetSize         uintptr
	WorkingSetSize             uintptr
	QuotaPeakPagedPoolUsage    uintptr
	QuotaPagedPoolUsage        uintptr
	QuotaPeakNonPagedPoolUsage uintptr
	QuotaNonPagedPoolUsage     uintptr
	PagefileUsage              uintptr
	PeakPagefileUsage          uintptr
	PrivateUsage               uintptr
}

func call(p *windows.LazyProc, a ...uintptr) uintptr {
	r, _, _ := p.Call(a...)
	return r
}

func ptOf(x, y int) uintptr { return uintptr(uint32(int32(x))) | uintptr(uint32(int32(y)))<<32 }

func moduleHandle() uintptr { return call(pGetModuleHandleW, 0) }

func setDpiAware() { call(pSetDpiAwarenessContext, uintptr(dpiPerMonV2)) }

func exStyle(h uintptr) uint32 { return uint32(call(pGetWindowLongPtrW, h, uintptr(gwlExStyle))) }

// applyStyles is win.rs apply_styles (its Layered hit mode): WS_EX_TOOLWINDOW always;
// WS_EX_NOACTIVATE unless the office takes keys; the click-through bits while the
// pointer is off the shape.
func applyStyles(h uintptr, acceptsKeys, clickThrough bool) {
	before := exStyle(h)
	ex := before | wsExToolWindow | wsExLayered
	if acceptsKeys {
		ex &^= wsExNoActivate
	} else {
		ex |= wsExNoActivate
	}
	if clickThrough {
		ex |= wsExTransparent
	} else {
		ex &^= wsExTransparent
	}
	if ex == before {
		return
	}
	call(pSetWindowLongPtrW, h, uintptr(gwlExStyle), uintptr(ex))
	if before&wsExLayered == 0 {
		// A layered window with no attributes is never drawn; fully opaque keeps the
		// DirectComposition content (and its per-pixel alpha) as it is.
		call(pSetLayeredWindowAttributes, h, 0, 255, lwaAlpha)
	}
}

func disableTransitions(h uintptr) {
	on := int32(1)
	call(pDwmSetWindowAttribute, h, dwmTransitionsOf, uintptr(unsafe.Pointer(&on)), 4)
}

func place(h uintptr, r rect) {
	call(pSetWindowPos, h, uintptr(hwndTopmost), uintptr(r.Left), uintptr(r.Top),
		uintptr(r.Right-r.Left), uintptr(r.Bottom-r.Top), swpNoActivate|swpShowWindow)
}

func raise(h uintptr) {
	call(pSetWindowPos, h, uintptr(hwndTopmost), 0, 0, 0, 0, swpNoMove|swpNoSize|swpNoActivate|swpShowWindow)
}

func windowRect(h uintptr) rect {
	var r rect
	call(pGetWindowRect, h, uintptr(unsafe.Pointer(&r)))
	return r
}

type monitor struct {
	bounds, work rect
	scale        float64
}

// ponytail: the primary monitor only, read once. notch-proto re-places the window when
// the monitor layout changes (a 2 s check); the port needs that, the spike doesn't.
func primary() monitor {
	m := call(pMonitorFromPoint, 0, 1) // MONITOR_DEFAULTTOPRIMARY: the primary holds 0,0
	info := monitorInfo{Size: uint32(unsafe.Sizeof(monitorInfo{}))}
	call(pGetMonitorInfoW, m, uintptr(unsafe.Pointer(&info)))
	var dx, dy uint32 = 96, 96
	scale := 1.0
	if call(pGetDpiForMonitor, m, 0, uintptr(unsafe.Pointer(&dx)), uintptr(unsafe.Pointer(&dy))) == 0 {
		scale = float64(dx) / 96
	}
	return monitor{info.Monitor, info.Work, scale}
}

func cursor() (int, int) {
	var p point
	call(pGetCursorPos, uintptr(unsafe.Pointer(&p)))
	return int(p.X), int(p.Y)
}

func buttonsDown() bool {
	for _, k := range []uintptr{vkLButton, vkRButton, vkMButton} {
		if uint16(call(pGetAsyncKeyState, k))&0x8000 != 0 {
			return true
		}
	}
	return false
}

func foreground() uintptr          { return call(pGetForegroundWindow) }
func setForeground(h uintptr) bool { return call(pSetForegroundWindow, h) != 0 }
func isWindow(h uintptr) bool      { return call(pIsWindow, h) != 0 }

// isOurs: our window, or one it owns (a menu, a dialog).
func isOurs(h, ours uintptr) bool { return h == ours || call(pGetAncestor, h, gaRootOwner) == ours }

func windowFromPoint(x, y int) uintptr { return call(pWindowFromPoint, ptOf(x, y)) }

func registerAltN(h uintptr) bool {
	return call(pRegisterHotKey, h, 1, modAlt|modNoRepeat, 'N') != 0
}

func cpuMS() float64 {
	var c, e, k, u windows.Filetime
	if windows.GetProcessTimes(windows.CurrentProcess(), &c, &e, &k, &u) != nil {
		return 0
	}
	t := func(f windows.Filetime) float64 {
		return float64(uint64(f.HighDateTime)<<32|uint64(f.LowDateTime)) / 10_000
	}
	return t(k) + t(u)
}

// privateBytes is private commit (PrivateUsage), the counter profiling.md reports.
func privateBytes() uint64 {
	pmc := processMemoryCountersEx{Cb: uint32(unsafe.Sizeof(processMemoryCountersEx{}))}
	call(pGetProcessMemoryInfo, uintptr(windows.CurrentProcess()), uintptr(unsafe.Pointer(&pmc)), uintptr(pmc.Cb))
	return uint64(pmc.PrivateUsage)
}

// capture is the composed desktop in a rectangle (device px) through the screen DC:
// DWM gives it with every layered and DirectComposition window drawn in.
func capture(r rect) *image.RGBA {
	w, h := max(r.Right-r.Left, 1), max(r.Bottom-r.Top, 1)
	screen := call(pGetDC, 0)
	mem := call(pCreateCompatibleDC, screen)
	bi := struct {
		H      bitmapInfoHeader
		Colors [1]uint32
	}{H: bitmapInfoHeader{Size: 40, Width: w, Height: -h, Planes: 1, BitCount: 32}}
	var bits unsafe.Pointer // the DIB's pixels, which GDI owns
	bmp := call(pCreateDIBSection, mem, uintptr(unsafe.Pointer(&bi)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	old := call(pSelectObject, mem, bmp)
	call(pBitBlt, mem, 0, 0, uintptr(w), uintptr(h), screen, uintptr(r.Left), uintptr(r.Top), srcCopy|captureBlt)
	img := image.NewRGBA(image.Rect(0, 0, int(w), int(h)))
	if bits != nil {
		raw := unsafe.Slice((*byte)(bits), int(w*h*4))
		for i := 0; i < len(raw); i += 4 {
			img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = raw[i+2], raw[i+1], raw[i], 255
		}
	}
	call(pSelectObject, mem, old)
	call(pDeleteObject, bmp)
	call(pDeleteDC, mem)
	call(pReleaseDC, 0, screen)
	return img
}

func sendMouse(in ...mouseInput) {
	call(pSendInput, uintptr(len(in)), uintptr(unsafe.Pointer(&in[0])), unsafe.Sizeof(in[0]))
}

func sendKeys(in ...keyInput) {
	call(pSendInput, uintptr(len(in)), uintptr(unsafe.Pointer(&in[0])), unsafe.Sizeof(in[0]))
}

func moveTo(x, y int) { call(pSetCursorPos, uintptr(x), uintptr(y)) }

func vkey(vk uint16, up bool) keyInput {
	k := keyInput{Typ: inputKeyboard, Vk: vk}
	if up {
		k.Flags = keyEventKeyUp
	}
	return k
}

func altN()   { sendKeys(vkey(vkMenu, false), vkey('N', false), vkey('N', true), vkey(vkMenu, true)) }
func escape() { sendKeys(vkey(vkEscape, false), vkey(vkEscape, true)) }

func typeText(s string) {
	var in []keyInput
	for _, u := range windows.StringToUTF16(s) {
		if u == 0 {
			continue
		}
		for _, up := range []bool{false, true} {
			k := keyInput{Typ: inputKeyboard, Scan: u, Flags: keyEventUnicode}
			if up {
				k.Flags |= keyEventKeyUp
			}
			in = append(in, k)
		}
	}
	sendKeys(in...)
}

func init() {
	// The INPUT layout the structs above stand in for. A wrong size makes SendInput
	// refuse every input, which would read as a failed check, not a crash.
	if unsafe.Sizeof(mouseInput{}) != 40 || unsafe.Sizeof(keyInput{}) != 40 ||
		unsafe.Offsetof(mouseInput{}.Dx) != 8 || unsafe.Offsetof(keyInput{}.Vk) != 8 {
		panic("INPUT is 40 bytes on amd64, its union at 8")
	}
}
