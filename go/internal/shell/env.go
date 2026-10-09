// Package shell is app/src/main.rs and notch.rs: Hover's one process. It owns the notch
// window and the app window, the island the notch shows at rest, Settings over the office,
// the timers, the shortcut and the tray's menu, and it asks the desktop for the rest
// through Env, which each operating system fills in (internal/platform/win, and later
// the X11 one).
package shell

import (
	"time"

	"gioui.org/io/input"
	"gioui.org/layout"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
)

// Timer is what After and Every hand back.
type Timer interface {
	Stop()
	Running() bool
}

// Handlers are a window's callbacks (each may be nil).
type Handlers struct {
	OnClose func() bool
	OnFocus func(bool)
	OnState func()
	OnPress func()
}

// Window is a window the shell draws into. Everything on it runs on the UI thread, except
// Invalidate.
type Window interface {
	// SetDraw sets what builds a frame: the Gio context (its constraints are the window's
	// size in physical pixels) and the scale. It returns true while something still moves.
	SetDraw(func(gtx layout.Context, scale float32) bool)
	SetHandlers(Handlers)
	Invalidate()
	Show()
	Hide()
	Close()
	Gone() bool
	Visible() bool
	Minimized() bool
	Maximized() bool
	Focused() bool
	Minimize()
	ToggleMaximize()
	DragMove()
	ResizeFrom(edge int)
	// Caption is the title bar's colours following the panel's.
	Caption(dark bool, panel uint32)
	Execute(input.Command)
	// Frames is how many frames have been shown.
	Frames() uint64
	// Size is the client area in physical pixels; Scale the pixels per logical pixel.
	Size() (w, h int)
	Scale() float64
}

// NotchPlat is notch.rs's Plat: what differs per platform for the notch's window. The
// notch's size and place are set through Place.
type NotchPlat interface {
	// Primary is the main display's work area in device pixels, and its scale.
	Primary() (notch.Rect, float64)
	// Signature changes when the displays do (checked every 2 s).
	Signature() string
	Cursor() (int, int)
	Buttons() bool
	Place(notch.Rect)
	Raise()
	// SetAcceptsKeys: the open office needs the keyboard; the resting notch must never take it.
	SetAcceptsKeys(bool)
	RememberForeground()
	// RestoreForeground hands the keyboard back to whatever had it before the notch took it.
	RestoreForeground()
	Focus()
	// SetHit: everything outside the shape passes the pointer through; over says the
	// pointer is on the part that takes it.
	SetHit(over bool)
	// ForegroundIsOurs: focus went to something that isn't ours (the click-away rule).
	ForegroundIsOurs() bool
}

// SysHooks are what the desktop reports to the shell, on the UI thread.
type SysHooks struct {
	// Hotkey: a registered chord was pressed (its id: 1 is the office's).
	Hotkey func(id int)
	// Deactivated: another window took the foreground from the notch.
	Deactivated func()
	// TrayLeft: the tray icon was clicked; TrayMenu: an item of its menu was picked.
	TrayLeft func()
	TrayMenu func(i int)
}

// Env is the desktop as the shell uses it.
type Env struct {
	// UIDo runs f on the UI thread, from any goroutine.
	UIDo  func(func())
	After func(d time.Duration, f func()) Timer
	Every func(d time.Duration, f func()) Timer
	Quit  func()

	OpenURL       func(url string)
	PickFolder    func() (string, bool)
	PickThemeFile func() (string, bool)
	PickImage     func() (string, bool)
	// ClipboardImage is a picture on the clipboard as RGBA.
	ClipboardImage func() (w, h int, rgba []byte, ok bool)

	// NewNotch makes the notch's window (hidden, off screen until placed). NewDashboard the
	// app window, NewWarning a small dialog (w and h in logical pixels, its client area).
	NewNotch     func() (Window, NotchPlat, error)
	NewDashboard func() (Window, error)
	NewWarning   func(title string, w, h float32) (Window, error)

	// Bind hands the OS the shell's callbacks for what it reports: the shortcut, the
	// foreground changing, the tray. TrayStart and TrayStop show and take away the icon.
	Bind                func(SysHooks)
	TrayStart, TrayStop func()

	// Hotkey takes the notch's shortcut, letting go of the last; false when the system
	// refuses it. Nil: none (no display).
	Hotkey func(core.Shortcut) bool
	// SetTrayMenu is the menu the tray shows; Notify a system notification.
	SetTrayMenu func(app.Menu)
	Notify      func(title, body string)
	// Headless: nothing is grabbed, placed or announced outside the process.
	Headless bool
}
