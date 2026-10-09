//go:build windows

// Package win is app/src/win.rs and the windowing winit did for Slint, on Win32 with no C
// compiler: windows that Gio draws into (Direct3D 11, and DirectComposition for the
// notch's per-pixel alpha), the message loop and its hand-off from other goroutines, the
// notch's click-through and focus rules, the shortcut, the tray icon, the system's file
// dialogs and the clipboard.
package win

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	user32  = windows.NewLazySystemDLL("user32.dll")
	gdi32   = windows.NewLazySystemDLL("gdi32.dll")
	dwmapi  = windows.NewLazySystemDLL("dwmapi.dll")
	shcore  = windows.NewLazySystemDLL("shcore.dll")
	shell32 = windows.NewLazySystemDLL("shell32.dll")
	ole32   = windows.NewLazySystemDLL("ole32.dll")
	kernel  = windows.NewLazySystemDLL("kernel32.dll")
	wtsapi  = windows.NewLazySystemDLL("wtsapi32.dll")

	pGetModuleHandleW = kernel.NewProc("GetModuleHandleW")
	pGlobalAlloc      = kernel.NewProc("GlobalAlloc")
	pGlobalLock       = kernel.NewProc("GlobalLock")
	pGlobalUnlock     = kernel.NewProc("GlobalUnlock")
	pGlobalFree       = kernel.NewProc("GlobalFree")

	pRegisterClassExW            = user32.NewProc("RegisterClassExW")
	pCreateWindowExW             = user32.NewProc("CreateWindowExW")
	pDestroyWindow               = user32.NewProc("DestroyWindow")
	pDefWindowProcW              = user32.NewProc("DefWindowProcW")
	pGetMessageW                 = user32.NewProc("GetMessageW")
	pTranslateMessage            = user32.NewProc("TranslateMessage")
	pDispatchMessageW            = user32.NewProc("DispatchMessageW")
	pPostMessageW                = user32.NewProc("PostMessageW")
	pSendMessageW                = user32.NewProc("SendMessageW")
	pPostQuitMessage             = user32.NewProc("PostQuitMessage")
	pSetWindowPos                = user32.NewProc("SetWindowPos")
	pShowWindow                  = user32.NewProc("ShowWindow")
	pIsWindowVisible             = user32.NewProc("IsWindowVisible")
	pIsIconic                    = user32.NewProc("IsIconic")
	pIsZoomed                    = user32.NewProc("IsZoomed")
	pGetWindowLongPtrW           = user32.NewProc("GetWindowLongPtrW")
	pSetWindowLongPtrW           = user32.NewProc("SetWindowLongPtrW")
	pSetLayeredWindowAttributes  = user32.NewProc("SetLayeredWindowAttributes")
	pGetWindowRect               = user32.NewProc("GetWindowRect")
	pGetClientRect               = user32.NewProc("GetClientRect")
	pAdjustWindowRectExForDpi    = user32.NewProc("AdjustWindowRectExForDpi")
	pMonitorFromPoint            = user32.NewProc("MonitorFromPoint")
	pMonitorFromWindow           = user32.NewProc("MonitorFromWindow")
	pGetMonitorInfoW             = user32.NewProc("GetMonitorInfoW")
	pEnumDisplayMonitors         = user32.NewProc("EnumDisplayMonitors")
	pEnumDisplayDevicesW         = user32.NewProc("EnumDisplayDevicesW")
	pGetCursorPos                = user32.NewProc("GetCursorPos")
	pScreenToClient              = user32.NewProc("ScreenToClient")
	pGetAsyncKeyState            = user32.NewProc("GetAsyncKeyState")
	pGetKeyState                 = user32.NewProc("GetKeyState")
	pGetForegroundWindow         = user32.NewProc("GetForegroundWindow")
	pSetForegroundWindow         = user32.NewProc("SetForegroundWindow")
	pSetFocus                    = user32.NewProc("SetFocus")
	pIsWindow                    = user32.NewProc("IsWindow")
	pGetAncestor                 = user32.NewProc("GetAncestor")
	pRegisterHotKey              = user32.NewProc("RegisterHotKey")
	pUnregisterHotKey            = user32.NewProc("UnregisterHotKey")
	pLoadCursorW                 = user32.NewProc("LoadCursorW")
	pSetCursor                   = user32.NewProc("SetCursor")
	pSetCapture                  = user32.NewProc("SetCapture")
	pReleaseCapture              = user32.NewProc("ReleaseCapture")
	pTrackMouseEvent             = user32.NewProc("TrackMouseEvent")
	pSetProcessDpiAwareness      = user32.NewProc("SetProcessDpiAwarenessContext")
	pGetSystemMetrics            = user32.NewProc("GetSystemMetrics")
	pGetDpiForWindow             = user32.NewProc("GetDpiForWindow")
	pCreatePopupMenu             = user32.NewProc("CreatePopupMenu")
	pAppendMenuW                 = user32.NewProc("AppendMenuW")
	pTrackPopupMenu              = user32.NewProc("TrackPopupMenu")
	pDestroyMenu                 = user32.NewProc("DestroyMenu")
	pOpenClipboard               = user32.NewProc("OpenClipboard")
	pCloseClipboard              = user32.NewProc("CloseClipboard")
	pEmptyClipboard              = user32.NewProc("EmptyClipboard")
	pGetClipboardData            = user32.NewProc("GetClipboardData")
	pSetClipboardData            = user32.NewProc("SetClipboardData")
	pLookupIconIdFromDirectoryEx = user32.NewProc("LookupIconIdFromDirectoryEx")
	pCreateIconFromResourceEx    = user32.NewProc("CreateIconFromResourceEx")
	pLoadIconW                   = user32.NewProc("LoadIconW")
	pMessageBoxW                 = user32.NewProc("MessageBoxW")
	pSetTimer                    = user32.NewProc("SetTimer")
	pKillTimer                   = user32.NewProc("KillTimer")

	pDwmSetWindowAttribute = dwmapi.NewProc("DwmSetWindowAttribute")
	pGetDpiForMonitor      = shcore.NewProc("GetDpiForMonitor")
	pShellNotifyIconW      = shell32.NewProc("Shell_NotifyIconW")
	pShellExecuteW         = shell32.NewProc("ShellExecuteW")
	pCoCreateInstance      = ole32.NewProc("CoCreateInstance")
	pCoInitializeEx        = ole32.NewProc("CoInitializeEx")
	pCoTaskMemFree         = ole32.NewProc("CoTaskMemFree")
	pWTSRegisterSession    = wtsapi.NewProc("WTSRegisterSessionNotification")
	pWTSUnRegisterSession  = wtsapi.NewProc("WTSUnRegisterSessionNotification")
)

// Window styles and messages.
const (
	wsPopup       = 0x80000000
	wsOverlapped  = 0x00000000
	wsCaption     = 0x00C00000
	wsSysMenu     = 0x00080000
	wsThickFrame  = 0x00040000
	wsMinimizeBox = 0x00020000
	wsMaximizeBox = 0x00010000
	wsClipSibs    = 0x04000000
	wsClipKids    = 0x02000000

	wsExTopmost             = 0x8
	wsExTransparent         = 0x20
	wsExToolWindow          = 0x80
	wsExAppWindow           = 0x40000
	wsExLayered             = 0x80000
	wsExNoRedirectionBitmap = 0x200000
	wsExNoActivate          = 0x8000000

	swpNoSize     = 0x1
	swpNoMove     = 0x2
	swpNoZOrder   = 0x4
	swpNoActivate = 0x10
	swpFrame      = 0x20
	swpShowWindow = 0x40

	swHide        = 0
	swShowNormal  = 1
	swMinimize    = 6
	swShowNoAct   = 4
	swMaximize    = 3
	swRestore     = 9
	swShow        = 5
	lwaAlpha      = 0x2
	hwndMessage   = ^uintptr(2) // HWND_MESSAGE, -3
	gaRootOwner   = 3
	idcArrow      = 32512
	smCxSmIcon    = 49
	smCxIcon      = 11
	smCxSizeFrame = 32
	tpmReturnCmd  = 0x100
	tpmRightBtn   = 0x2
	tpmBottomAlgn = 0x20
	mfString      = 0x0
	mfSeparator   = 0x800
	mfChecked     = 0x8

	wmDestroy         = 0x2
	wmSize            = 0x5
	wmActivate        = 0x6
	wmSetFocus        = 0x7
	wmKillFocus       = 0x8
	wmClose           = 0x10
	wmEraseBkgnd      = 0x14
	wmShowWindow      = 0x18
	wmSetCursor       = 0x20
	wmGetMinMaxInfo   = 0x24
	wmNcCalcSize      = 0x83
	wmNcLButtonDown   = 0xA1
	wmKeyDown         = 0x100
	wmKeyUp           = 0x101
	wmChar            = 0x102
	wmSysKeyDown      = 0x104
	wmSysKeyUp        = 0x105
	wmUniChar         = 0x109
	wmTimer           = 0x113
	wmMouseMove       = 0x200
	wmLButtonDown     = 0x201
	wmLButtonUp       = 0x202
	wmLButtonDblClk   = 0x203
	wmRButtonDown     = 0x204
	wmRButtonUp       = 0x205
	wmMButtonDown     = 0x207
	wmMButtonUp       = 0x208
	wmMouseWheel      = 0x20A
	wmMouseHWheel     = 0x20E
	wmCaptureChanged  = 0x215
	wmMouseLeave      = 0x2A3
	wmHotKey          = 0x312
	wmDpiChanged      = 0x2E0
	wmPowerBroadcast  = 0x218
	wmWTSSession      = 0x2B1
	wmContextMenu     = 0x7B
	wmCancelMode      = 0x1F
	wmSetIcon         = 0x80
	wmApp             = 0x8000
	waInactive        = 0
	pbtResumeAuto     = 0x12
	wtsSessionUnlock  = 0x8
	notifyThisSession = 0
	tmeLeave          = 0x2
	mkLButton         = 0x1
	mkRButton         = 0x2
	mkMButton         = 0x10

	htClient      = 1
	htCaption     = 2
	htLeft        = 10
	htRight       = 11
	htTop         = 12
	htTopLeft     = 13
	htTopRight    = 14
	htBottom      = 15
	htBottomLeft  = 16
	htBottomRight = 17

	vkLButton = 0x01
	vkRButton = 0x02
	vkMButton = 0x04
	vkShift   = 0x10
	vkControl = 0x11
	vkMenu    = 0x12
	vkLWin    = 0x5B
	vkRWin    = 0x5C

	modAlt      = 0x1
	modControl  = 0x2
	modShift    = 0x4
	modWin      = 0x8
	modNoRepeat = 0x4000

	dwmaTransitionsOff = 3  // DWMWA_TRANSITIONS_FORCEDISABLED
	dwmaDarkMode       = 20 // DWMWA_USE_IMMERSIVE_DARK_MODE
	dwmaCaptionColor   = 35
	dwmaTextColor      = 36

	cfUnicodeText = 13
	gmemMoveable  = 0x2
)

var (
	dpiPerMonV2 = -4  // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
	gwlExStyle  = -20 // GWL_EXSTYLE
)

// ptr is a uintptr a message or the system gave as the memory it points at.
func ptr(v uintptr) unsafe.Pointer { return *(*unsafe.Pointer)(unsafe.Pointer(&v)) }

type point struct{ X, Y int32 }
type rect struct{ Left, Top, Right, Bottom int32 }

func (r rect) w() int { return int(r.Right - r.Left) }
func (r rect) h() int { return int(r.Bottom - r.Top) }

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

type monitorInfoEx struct {
	Size    uint32
	Monitor rect
	Work    rect
	Flags   uint32
	Device  [32]uint16
}

type displayDevice struct {
	Cb         uint32
	DeviceName [32]uint16
	DeviceStr  [128]uint16
	StateFlags uint32
	DeviceID   [128]uint16
	DeviceKey  [128]uint16
}

type minMaxInfo struct {
	Reserved, MaxSize, MaxPosition, MinTrack, MaxTrack point
}

type nccalcsizeParams struct {
	Rgrc [3]rect
	Pos  uintptr
}

type trackMouse struct {
	Size, Flags uint32
	Track       uintptr
	HoverTime   uint32
}

// NOTIFYICONDATAW, version 5 layout (976 bytes on amd64): Go lays the fields out as C
// does.
type notifyIconData struct {
	Size             uint32
	Wnd              uintptr
	ID               uint32
	Flags            uint32
	CallbackMessage  uint32
	Icon             uintptr
	Tip              [128]uint16
	State, StateMask uint32
	Info             [256]uint16
	Timeout          uint32
	InfoTitle        [64]uint16
	InfoFlags        uint32
	Guid             windows.GUID
	BalloonIcon      uintptr
}

func call(p *windows.LazyProc, a ...uintptr) uintptr {
	r, _, _ := p.Call(a...)
	return r
}

func moduleHandle() uintptr { return call(pGetModuleHandleW, 0) }

func w16(s string) *uint16 {
	p, _ := windows.UTF16PtrFromString(s)
	return p
}

// wide fills a fixed UTF-16 field, cut to n-1 units (the C code's szTip).
func wide(dst []uint16, s string) {
	u, _ := windows.UTF16FromString(s)
	if len(u) > len(dst) {
		u = u[:len(dst)]
		u[len(u)-1] = 0
	}
	copy(dst, u)
}

func lo16(v uintptr) int { return int(int16(v & 0xFFFF)) }
func hi16(v uintptr) int { return int(int16(v >> 16 & 0xFFFF)) }

func init() {
	// The structs stand in for C's: a wrong size makes the call fail without a crash.
	if unsafe.Sizeof(notifyIconData{}) != 976 {
		panic("NOTIFYICONDATAW is 976 bytes on amd64")
	}
}
