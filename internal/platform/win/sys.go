//go:build windows

package win

import (
	_ "embed"
	"encoding/binary"
	"fmt"
	"unsafe"

	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"golang.org/x/sys/windows"

	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/notch"
)

func logf(format string, a ...any) { core.Logf(format, a...) }

// MARK: Monitors

// Monitor is a display: its device name, whether it is the primary, its bounds and work
// area in physical pixels, and its scale.
type Monitor struct {
	Device       string
	Primary      bool
	Bounds, Work notch.Rect
	scale        float64
	work         rect
}

func (m Monitor) Scale() float64 { return m.scale }

func toNotch(r rect) notch.Rect {
	return notch.Rect{Left: int(r.Left), Top: int(r.Top), Right: int(r.Right), Bottom: int(r.Bottom)}
}

func info(h uintptr) (Monitor, bool) {
	mi := monitorInfoEx{Size: uint32(unsafe.Sizeof(monitorInfoEx{}))}
	if call(pGetMonitorInfoW, h, uintptr(unsafe.Pointer(&mi))) == 0 {
		return Monitor{}, false
	}
	var dx, dy uint32 = 96, 96
	scale := 1.0
	if call(pGetDpiForMonitor, h, 0, uintptr(unsafe.Pointer(&dx)), uintptr(unsafe.Pointer(&dy))) == 0 {
		scale = float64(dx) / 96
	}
	return Monitor{
		Device: windows.UTF16ToString(mi.Device[:]), Primary: mi.Flags&1 != 0,
		Bounds: toNotch(mi.Monitor), Work: toNotch(mi.Work), scale: scale, work: mi.Work,
	}, true
}

// Monitors lists the displays.
func Monitors() []Monitor {
	var list []Monitor
	cb := windows.NewCallback(func(h, _, _, _ uintptr) uintptr {
		if m, ok := info(h); ok {
			list = append(list, m)
		}
		return 1
	})
	call(pEnumDisplayMonitors, 0, 0, cb, 0)
	return list
}

func primaryMonitor() Monitor {
	// MONITOR_DEFAULTTOPRIMARY: the primary holds 0,0.
	if m, ok := info(call(pMonitorFromPoint, 0, 1)); ok {
		return m
	}
	return Monitor{Primary: true, Bounds: notch.Rect{Right: 1920, Bottom: 1080}, Work: notch.Rect{Right: 1920, Bottom: 1040}, scale: 1,
		work: rect{0, 0, 1920, 1040}}
}

func monitorOf(h uintptr) Monitor {
	// MONITOR_DEFAULTTONEAREST.
	if m, ok := info(call(pMonitorFromWindow, h, 2)); ok {
		return m
	}
	return primaryMonitor()
}

// Primary is the main display.
func Primary() Monitor { return primaryMonitor() }

// primaryDisplayAdapter is the name of the graphics card driving the main display
// ("NVIDIA GeForce GTX 1060 6GB"), as DXGI names the same adapter.
func primaryDisplayAdapter() string {
	for i := 0; ; i++ {
		d := displayDevice{Cb: uint32(unsafe.Sizeof(displayDevice{}))}
		if call(pEnumDisplayDevicesW, 0, uintptr(i), uintptr(unsafe.Pointer(&d)), 0) == 0 {
			return ""
		}
		// DISPLAY_DEVICE_PRIMARY_DEVICE
		if d.StateFlags&4 != 0 {
			return windows.UTF16ToString(d.DeviceStr[:])
		}
	}
}

// MARK: The pointer and the keyboard

// Cursor is the pointer's position on the screen.
func Cursor() (int, int) {
	var p point
	call(pGetCursorPos, uintptr(unsafe.Pointer(&p)))
	return int(p.X), int(p.Y)
}

// ButtonsDown says a mouse button is down.
func ButtonsDown() bool {
	for _, k := range []uintptr{vkLButton, vkRButton, vkMButton} {
		if uint16(call(pGetAsyncKeyState, k))&0x8000 != 0 {
			return true
		}
	}
	return false
}

// KeyDown says a virtual key is down now (the hold-to-talk poll).
func KeyDown(vk uint16) bool { return uint16(call(pGetAsyncKeyState, uintptr(vk)))&0x8000 != 0 }

var cursors = map[pointer.Cursor]uintptr{
	pointer.CursorDefault: 32512, pointer.CursorText: 32513, pointer.CursorWait: 32514, pointer.CursorCrosshair: 32515,
	pointer.CursorNotAllowed: 32648, pointer.CursorPointer: 32649, pointer.CursorProgress: 32650,
	pointer.CursorRowResize: 32645, pointer.CursorColResize: 32644, pointer.CursorAllScroll: 32646,
	pointer.CursorNorthResize: 32645, pointer.CursorSouthResize: 32645, pointer.CursorEastResize: 32644, pointer.CursorWestResize: 32644,
	pointer.CursorNorthEastResize: 32643, pointer.CursorSouthWestResize: 32643, pointer.CursorNorthWestResize: 32642, pointer.CursorSouthEastResize: 32642,
	pointer.CursorNorthSouthResize: 32645, pointer.CursorEastWestResize: 32644,
	pointer.CursorNorthEastSouthWestResize: 32643, pointer.CursorNorthWestSouthEastResize: 32642,
	pointer.CursorGrab: 32649, pointer.CursorGrabbing: 32646,
}

func cursorHandle(c pointer.Cursor) uintptr {
	if c == pointer.CursorNone {
		return 0
	}
	id, ok := cursors[c]
	if !ok {
		id = idcArrow
	}
	return call(pLoadCursorW, 0, id)
}

// keyName is Gio's name for a virtual key, as its own Windows window gives them, plus
// the keys it has none for (a shortcut can be Insert or F13).
func keyName(vk uintptr) (key_ key.Name, ok bool) {
	if '0' <= vk && vk <= '9' || 'A' <= vk && vk <= 'Z' {
		return key.Name(rune(vk)), true
	}
	if 0x60 <= vk && vk <= 0x69 {
		return key.Name('0' + rune(vk-0x60)), true
	}
	if 0x7C <= vk && vk <= 0x87 {
		return key.Name(fmt.Sprintf("F%d", vk-0x7C+13)), true
	}
	n, ok := vkNames[vk]
	return n, ok
}

// ponytail: Gio's own table, and no more. Keys the layout moves (the Oem ones) are named
// as on a US keyboard, which is what the shortcut recorder and RegisterHotKey expect.
var vkNames = map[uintptr]key.Name{
	0x6B: "+", 0x6D: "-", 0x6A: "*", 0x6F: "/", 0x6E: ".",
	0x1B: "⎋", 0x25: "←", 0x27: "→", 0x0D: "⏎", 0x26: "↑", 0x28: "↓", 0x24: "⇱", 0x23: "⇲",
	0x08: "⌫", 0x2E: "⌦", 0x21: "⇞", 0x22: "⇟",
	0x70: "F1", 0x71: "F2", 0x72: "F3", 0x73: "F4", 0x74: "F5", 0x75: "F6", 0x76: "F7", 0x77: "F8", 0x78: "F9", 0x79: "F10", 0x7A: "F11", 0x7B: "F12",
	0x09: "Tab", 0x20: "Space",
	0xBA: ";", 0xBB: "+", 0xBC: ",", 0xBD: "-", 0xBE: ".", 0xBF: "/", 0xC0: "`", 0xDB: "[", 0xDC: "\\", 0xE2: "\\", 0xDD: "]", 0xDE: "'",
	0x11: "Ctrl", 0x10: "Shift", 0x12: "Alt", 0x5B: "Super", 0x5C: "Super",
	0x14: "CapsLock", 0x2D: "Insert", 0x13: "Pause", 0x2C: "PrintScreen", 0x5D: "Apps", 0x91: "ScrollLock",
}

// MARK: The application's icon

//go:embed hover.ico
var hoverICO []byte

// icon is the app's icon at the size asked for (hover.ico has a frame for each).
func icon(size int) uintptr {
	b := hoverICO
	off := int(call(pLookupIconIdFromDirectoryEx, uintptr(unsafe.Pointer(&b[0])), 1, uintptr(size), uintptr(size), 0))
	if off <= 0 {
		return call(pLoadIconW, 0, 32512)
	}
	// The directory entry's offset and length, then the image itself.
	count := int(b[4]) | int(b[5])<<8
	for i := 0; i < count; i++ {
		e := b[6+i*16 : 6+i*16+16]
		n := int(e[8]) | int(e[9])<<8 | int(e[10])<<16 | int(e[11])<<24
		at := int(e[12]) | int(e[13])<<8 | int(e[14])<<16 | int(e[15])<<24
		if at == off {
			if h := call(pCreateIconFromResourceEx, uintptr(unsafe.Pointer(&b[at])), uintptr(n), 1, 0x00030000, uintptr(size), uintptr(size), 0); h != 0 {
				return h
			}
		}
	}
	return call(pLoadIconW, 0, 32512)
}

// MARK: The clipboard

// ClipboardText is the clipboard's text, "" for none.
func ClipboardText() string {
	if call(pOpenClipboard, loop.hub) == 0 {
		return ""
	}
	defer call(pCloseClipboard)
	mem := call(pGetClipboardData, cfUnicodeText)
	if mem == 0 {
		return ""
	}
	p := call(pGlobalLock, mem)
	if p == 0 {
		return ""
	}
	defer call(pGlobalUnlock, mem)
	return windows.UTF16PtrToString((*uint16)(ptr(p)))
}

// ClipboardImage is a picture on the clipboard as RGBA (the page's paste handler, arboard's
// get_image). Windows makes CF_DIB of any picture put there.
// ponytail: 24 and 32 bits per pixel, which is what screenshots and copied images are; a
// 32-bit DIB's alpha byte is unused in BI_RGB, so those come out opaque.
func ClipboardImage() (w, h int, rgba []byte, ok bool) {
	const cfDIB = 8
	if call(pOpenClipboard, loop.hub) == 0 {
		return 0, 0, nil, false
	}
	defer call(pCloseClipboard)
	mem := call(pGetClipboardData, cfDIB)
	if mem == 0 {
		return 0, 0, nil, false
	}
	size := int(call(pGlobalSize, mem))
	p := call(pGlobalLock, mem)
	if p == 0 || size < 40 {
		return 0, 0, nil, false
	}
	defer call(pGlobalUnlock, mem)
	b := unsafe.Slice((*byte)(ptr(p)), size)
	hdr := binary.LittleEndian.Uint32(b[0:])
	width := int(int32(binary.LittleEndian.Uint32(b[4:])))
	height := int(int32(binary.LittleEndian.Uint32(b[8:])))
	bits := int(binary.LittleEndian.Uint16(b[14:]))
	comp := binary.LittleEndian.Uint32(b[16:])
	used := binary.LittleEndian.Uint32(b[32:])
	if (bits != 24 && bits != 32) || (comp != 0 && comp != 3) || width <= 0 || height == 0 || int(hdr) > size {
		return 0, 0, nil, false
	}
	off := int(hdr)
	// BI_BITFIELDS after a plain header: three masks, which for 32 bits are the usual BGRA.
	if comp == 3 && hdr == 40 {
		off += 12
	}
	off += int(used) * 4
	top := height < 0
	if top {
		height = -height
	}
	stride := (width*bits/8 + 3) &^ 3
	if off+stride*height > size || width > 1<<15 || height > 1<<15 {
		return 0, 0, nil, false
	}
	out := make([]byte, width*height*4)
	for y := 0; y < height; y++ {
		sy := y
		if !top {
			sy = height - 1 - y
		}
		row := b[off+sy*stride:]
		for x := 0; x < width; x++ {
			px := row[x*bits/8:]
			o := (y*width + x) * 4
			out[o], out[o+1], out[o+2], out[o+3] = px[2], px[1], px[0], 255
		}
	}
	return width, height, out, true
}

// SetClipboardText puts text on the clipboard.
func SetClipboardText(s string) bool {
	u, err := windows.UTF16FromString(s)
	if err != nil || call(pOpenClipboard, loop.hub) == 0 {
		return false
	}
	defer call(pCloseClipboard)
	call(pEmptyClipboard)
	mem := call(pGlobalAlloc, gmemMoveable, uintptr(len(u)*2))
	if mem == 0 {
		return false
	}
	p := call(pGlobalLock, mem)
	if p == 0 {
		call(pGlobalFree, mem)
		return false
	}
	copy(unsafe.Slice((*uint16)(ptr(p)), len(u)), u)
	call(pGlobalUnlock, mem)
	if call(pSetClipboardData, cfUnicodeText, mem) == 0 {
		call(pGlobalFree, mem)
		return false
	}
	return true
}

// OpenURL opens a link in the browser (rundll32 url.dll,FileProtocolHandler in the Rust
// build; ShellExecute is the same call).
func OpenURL(url string) {
	if int(call(pShellExecuteW, 0, uintptr(unsafe.Pointer(w16("open"))), uintptr(unsafe.Pointer(w16(url))), 0, 0, swShowNormal)) <= 32 {
		logf("couldn't open %s", url)
	}
}

// MARK: The notch's platform (notch.rs's Plat on Win32)

// NotchPlat is what the notch asks of Windows: where the main display is, where the
// pointer is, and who has the keyboard.
type NotchPlat struct {
	W        *Window
	previous uintptr
	through  bool
	keys     bool
}

func NewNotchPlat(w *Window) *NotchPlat { return &NotchPlat{W: w, through: true} }

func (p *NotchPlat) Primary() (notch.Rect, float64) {
	m := primaryMonitor()
	return m.Work, m.scale
}

func (p *NotchPlat) Signature() string {
	s := ""
	for _, m := range Monitors() {
		s += fmt.Sprintf("%s%v%v%v%v", m.Device, m.Primary, m.Bounds, m.Work, m.scale)
	}
	return s
}

func (p *NotchPlat) Cursor() (int, int) { return Cursor() }
func (p *NotchPlat) Buttons() bool      { return ButtonsDown() }
func (p *NotchPlat) Place(r notch.Rect) { p.W.Place(r.Left, r.Top, r.Right, r.Bottom) }
func (p *NotchPlat) Raise()             { p.W.Raise() }

func (p *NotchPlat) SetAcceptsKeys(on bool) {
	p.keys = on
	p.W.ApplyStyles(on, p.through)
}

func (p *NotchPlat) RememberForeground() { p.previous = Foreground() }

func (p *NotchPlat) RestoreForeground() {
	if Foreground() == p.W.HWND && p.previous != 0 && IsWindow(p.previous) {
		SetForeground(p.previous)
	}
}

func (p *NotchPlat) Focus() { SetForeground(p.W.HWND) }

// SetHit: the window takes the pointer only over the shape; WS_EX_TRANSPARENT otherwise.
func (p *NotchPlat) SetHit(over bool) {
	if through := !over; through != p.through {
		p.through = through
		p.W.ApplyStyles(p.keys, through)
	}
}

func (p *NotchPlat) ForegroundIsOurs() bool { return IsOurs(Foreground(), p.W.HWND) }

// MARK: The shortcut

// Hotkey modifiers for RegisterHotkey.
const (
	ModAlt     = modAlt
	ModControl = modControl
	ModShift   = modShift
	ModWin     = modWin
)

// RegisterHotkey is HotKeys.Register: MOD_NOREPEAT and the chord's modifiers. The error
// carries Windows' code when it refuses (reserved, or another app has it).
func (w *Window) RegisterHotkey(id int, vk uint16, mods uint32) error {
	if call(pRegisterHotKey, w.HWND, uintptr(id), uintptr(mods|modNoRepeat), uintptr(vk)) != 0 {
		return nil
	}
	_, _, e := pRegisterHotKey.Call(w.HWND, uintptr(id), uintptr(mods|modNoRepeat), uintptr(vk))
	code, _ := e.(windows.Errno)
	return HotkeyError{Code: uint32(code)}
}

// HotkeyError is RegisterHotKey's refusal; 1409 is "already registered".
type HotkeyError struct{ Code uint32 }

func (e HotkeyError) Error() string { return fmt.Sprintf("Win32 error %d", e.Code) }

// UnregisterHotkey lets go of a chord.
func (w *Window) UnregisterHotkey(id int) { call(pUnregisterHotKey, w.HWND, uintptr(id)) }

// MARK: The notch window's messages: the shortcut, activation, resume and unlock, the tray

// Hooks are what the notch window passes on from messages the loop doesn't.
type Hooks struct {
	// Hotkey: a registered chord was pressed (its id).
	Hotkey func(id int)
	// Deactivated: another window took the foreground.
	Deactivated func()
	// Greet: a resume or an unlock (NotchManager.OnPower, OnSession).
	Greet func()
	// TrayLeft: the tray icon was clicked; TrayMenu: an item of its menu was picked.
	TrayLeft func()
	TrayMenu func(i int)
}

const wmTray = wmApp + 3

// Hook sets w.Msg to deliver these. Each runs after the message, on the UI thread.
func (w *Window) Hook(h Hooks) {
	do := func(f func()) {
		if f != nil {
			UIDo(f)
		}
	}
	w.Msg = func(m uint32, wp, lp uintptr) (uintptr, bool) {
		switch m {
		case wmHotKey:
			id := int(wp)
			if h.Hotkey != nil {
				do(func() { h.Hotkey(id) })
			}
			return 0, true
		case wmActivate:
			if wp&0xFFFF == waInactive {
				do(h.Deactivated)
			}
		case wmPowerBroadcast:
			if wp == pbtResumeAuto {
				do(h.Greet)
			}
		case wmWTSSession:
			if wp == wtsSessionUnlock {
				do(h.Greet)
			}
		case wmTray:
			switch lp & 0xFFFF {
			case wmLButtonUp:
				do(h.TrayLeft)
			case wmRButtonUp, wmContextMenu:
				if i, ok := trayPopup(w.HWND); ok && h.TrayMenu != nil {
					do(func() { h.TrayMenu(i) })
				}
			}
			return 0, true
		}
		return 0, false
	}
}

// MARK: The tray icon

// MenuItem is an item of the tray's menu: a separator, or a label with an optional tick.
type MenuItem struct {
	Sep   bool
	Label string
	Check *bool
}

var trayMenu []MenuItem

// SetTrayMenu is the menu the next right click shows.
func SetTrayMenu(m []MenuItem) { trayMenu = m }

func (w *Window) trayData() notifyIconData {
	return notifyIconData{Size: uint32(unsafe.Sizeof(notifyIconData{})), Wnd: w.HWND, ID: 1, CallbackMessage: wmTray}
}

// TrayStart is Shell_NotifyIcon: the icon, "Hover" as its tip, messages to the window.
func (w *Window) TrayStart() {
	d := w.trayData()
	d.Flags = 0x1 | 0x2 | 0x4 // NIF_MESSAGE | NIF_ICON | NIF_TIP
	d.Icon = icon(int(call(pGetSystemMetrics, smCxSmIcon)))
	wide(d.Tip[:], "Hover")
	call(pShellNotifyIconW, 0, uintptr(unsafe.Pointer(&d))) // NIM_ADD
}

func (w *Window) TrayStop() {
	d := w.trayData()
	call(pShellNotifyIconW, 2, uintptr(unsafe.Pointer(&d))) // NIM_DELETE
}

// TrayNotify is TrayIcon.Notify: a balloon of six seconds.
func (w *Window) TrayNotify(title, text string) {
	d := w.trayData()
	d.Flags = 0x10 // NIF_INFO
	wide(d.InfoTitle[:], title)
	wide(d.Info[:], text)
	d.Timeout = 6000
	call(pShellNotifyIconW, 1, uintptr(unsafe.Pointer(&d))) // NIM_MODIFY
}

// trayPopup shows the menu at the pointer, as the C# opened its ContextMenu there, and
// returns the item picked.
func trayPopup(h uintptr) (int, bool) {
	menu := call(pCreatePopupMenu)
	if menu == 0 {
		return 0, false
	}
	defer call(pDestroyMenu, menu)
	for i, it := range trayMenu {
		if it.Sep {
			call(pAppendMenuW, menu, mfSeparator, 0, 0)
			continue
		}
		flags := uintptr(mfString)
		if it.Check != nil && *it.Check {
			flags |= mfChecked
		}
		label := replaceEllipsis(it.Label)
		call(pAppendMenuW, menu, flags, uintptr(i+1), uintptr(unsafe.Pointer(w16(label))))
	}
	x, y := Cursor()
	// The menu closes when clicked away only if the window is in the foreground.
	call(pSetForegroundWindow, h)
	picked := call(pTrackPopupMenu, menu, tpmReturnCmd|tpmRightBtn, uintptr(x), uintptr(y), 0, h, 0)
	return int(picked) - 1, picked > 0
}

func replaceEllipsis(s string) string {
	out := []rune{}
	for _, r := range s {
		if r == '…' {
			out = append(out, '.', '.', '.')
			continue
		}
		out = append(out, r)
	}
	return string(out)
}
