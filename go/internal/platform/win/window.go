//go:build windows

package win

import (
	"errors"
	"image"
	"io"
	"os"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"
	"unsafe"

	"gioui.org/f32"
	"gioui.org/io/input"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/io/transfer"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/unit"
	"golang.org/x/sys/windows"
)

// Kind is what a window is for.
type Kind uint8

const (
	// KindNotch is the notch: borderless, topmost, never in the taskbar, see-through, and
	// not taking the keyboard unless asked.
	KindNotch Kind = iota
	// KindFrame is the app window: no frame of its own (Hover draws the title bar), resized
	// and moved by messages the title bar and its border send.
	KindFrame
	// KindDialog is an ordinary small window with the system's title bar.
	KindDialog
)

// Options are what a window is made with. Size is the client area in logical pixels.
type Options struct {
	Kind         Kind
	Title, Class string
	W, H         float32
	MinW, MinH   float32
	// Hidden: made but not shown (the notch is placed first).
	Hidden bool
	// Icon: the app's, in the title bar and the taskbar.
	Icon bool
}

// Window is one Win32 window that Gio draws into. Everything on it runs on the UI thread
// except Invalidate, which any goroutine may call.
type Window struct {
	HWND uintptr
	Kind Kind
	// Draw builds one frame's operations in the Gio context (its constraints are the window's
	// size in physical pixels) at the window's scale (physical pixels per logical one); it
	// returns true while something still moves (the next frame comes 16 ms later).
	Draw func(gtx layout.Context, scale float32) bool
	// Msg sees a message before the window does; handled says it is dealt with.
	Msg func(m uint32, wp, lp uintptr) (r uintptr, handled bool)
	// OnClose is asked when the user closes the window; false keeps it.
	OnClose func() bool
	// OnFocus is told when the window gains or loses the keyboard.
	OnFocus func(focused bool)
	// OnState is told when the window is resized, maximised, minimised or restored.
	OnState func()
	// OnPress is told when a mouse button goes down in the window, before Gio sees it.
	OnPress func()
	// ScaleOverride, when set, is the window's scale in place of its monitor's (the notch is
	// laid out for the primary monitor).
	ScaleOverride float64

	opts    Options
	tgt     *target
	gen     int
	router  input.Router
	ops     op.Ops
	size    image.Point
	t0      time.Time
	pending atomic.Bool
	cursor  pointer.Cursor
	cursorH uintptr
	inside  bool
	tracked bool
	downs   int
	surr    uint16
	ime     input.EditorState
	buttons pointer.Buttons
	focused bool
	gone    bool
	frames  atomic.Uint64
	minW    int
	minH    int
}

var (
	loop struct {
		hub   uintptr
		mu    sync.Mutex
		queue []func()
		wins  map[uintptr]*Window
		class sync.Once
		err   error
	}
	wndProcPtr = windows.NewCallback(wndProc)
)

const (
	wmDo    = wmApp + 1 // the hub: run what UIDo queued
	wmPaint = 0xF
	wmFrame = wmApp + 2 // a window: draw a frame
)

// Init locks the calling goroutine to its thread (Win32 sends a window's messages to the
// thread that made it), makes the process DPI aware and makes the hub window that
// UIDo posts to. Call it first, from main.
func Init() error {
	runtime.LockOSThread()
	call(pSetProcessDpiAwareness, uintptr(dpiPerMonV2))
	loop.wins = map[uintptr]*Window{}
	if err := registerClasses(); err != nil {
		return err
	}
	loop.hub = call(pCreateWindowExW, 0, uintptr(unsafe.Pointer(w16("HoverHub"))), 0, 0, 0, 0, 0, 0, hwndMessage, 0, moduleHandle(), 0)
	if loop.hub == 0 {
		return errors.New("win: the hub window couldn't be made")
	}
	return nil
}

var classes = map[string]bool{}

func registerClass(name string, style uint32, cursor bool) error {
	if classes[name] {
		return nil
	}
	wc := wndClassEx{
		Size: uint32(unsafe.Sizeof(wndClassEx{})), Style: style, WndProc: wndProcPtr,
		Instance: moduleHandle(), ClassName: w16(name),
	}
	if cursor {
		wc.Cursor = call(pLoadCursorW, 0, idcArrow)
	}
	if call(pRegisterClassExW, uintptr(unsafe.Pointer(&wc))) == 0 {
		return errors.New("win: RegisterClassEx failed for " + name)
	}
	classes[name] = true
	return nil
}

func registerClasses() error {
	for _, n := range []string{"HoverHub", "HoverNotch", "HoverApp", "HoverDialog"} {
		if err := registerClass(n, 0, n != "HoverHub"); err != nil {
			return err
		}
	}
	return nil
}

// Run is the message loop; it returns after Quit.
func Run() {
	var m msg
	for int32(call(pGetMessageW, uintptr(unsafe.Pointer(&m)), 0, 0, 0)) > 0 {
		call(pTranslateMessage, uintptr(unsafe.Pointer(&m)))
		call(pDispatchMessageW, uintptr(unsafe.Pointer(&m)))
	}
}

// Quit ends Run. Any goroutine.
func Quit() { UIDo(func() { call(pPostQuitMessage, 0) }) }

// UIDo runs f on the UI thread, soon. Any goroutine (the hooks of the agents and the
// quota poller fire off it).
func UIDo(f func()) {
	loop.mu.Lock()
	loop.queue = append(loop.queue, f)
	loop.mu.Unlock()
	call(pPostMessageW, loop.hub, wmDo, 0, 0)
}

func drain() {
	loop.mu.Lock()
	q := loop.queue
	loop.queue = nil
	loop.mu.Unlock()
	for _, f := range q {
		f()
	}
}

// Timer runs a function on the UI thread, once or every period, until stopped.
type Timer struct {
	t      *time.Timer
	tk     *time.Ticker
	stop   chan struct{}
	live   atomic.Bool
	period time.Duration
}

// After runs f once, d from now.
func After(d time.Duration, f func()) *Timer {
	t := &Timer{}
	t.live.Store(true)
	t.t = time.AfterFunc(d, func() {
		UIDo(func() {
			if t.live.CompareAndSwap(true, false) {
				f()
			}
		})
	})
	return t
}

// Every runs f every d until stopped.
func Every(d time.Duration, f func()) *Timer {
	t := &Timer{tk: time.NewTicker(d), stop: make(chan struct{}), period: d}
	t.live.Store(true)
	go func() {
		for {
			select {
			case <-t.tk.C:
				UIDo(func() {
					if t.live.Load() {
						f()
					}
				})
			case <-t.stop:
				return
			}
		}
	}()
	return t
}

// Stop stops it; one already queued does not run. Safe on nil.
func (t *Timer) Stop() {
	if t == nil || !t.live.Swap(false) {
		return
	}
	if t.t != nil {
		t.t.Stop()
	}
	if t.tk != nil {
		t.tk.Stop()
		close(t.stop)
	}
}

// Running says it has not run or been stopped yet.
func (t *Timer) Running() bool { return t != nil && t.live.Load() }

// NewWindow makes the window, its swap chain and its first frame's place. Draw is set by
// the caller before the first Invalidate.
func NewWindow(o Options) (*Window, error) {
	w := &Window{Kind: o.Kind, opts: o, t0: time.Now(), cursor: pointer.CursorDefault, minW: int(o.MinW), minH: int(o.MinH)}
	scale := 1.0
	if mon := primaryMonitor(); mon.scale > 0 {
		scale = mon.scale
	}
	pw, ph := int(float64(o.W)*scale+0.5), int(float64(o.H)*scale+0.5)
	var style, ex uintptr
	class := o.Class
	switch o.Kind {
	case KindNotch:
		style, ex = wsPopup, wsExNoRedirectionBitmap|wsExToolWindow|wsExTopmost|wsExNoActivate
		if class == "" {
			class = "HoverNotch"
		}
	case KindFrame:
		style, ex = wsPopup|wsThickFrame|wsMinimizeBox|wsMaximizeBox|wsSysMenu|wsClipSibs|wsClipKids, wsExAppWindow
		if class == "" {
			class = "HoverApp"
		}
	default:
		style, ex = wsCaption|wsSysMenu|wsClipSibs, wsExAppWindow
		if class == "" {
			class = "HoverDialog"
		}
		// The client area is what was asked for; the title bar comes on top of it.
		r := rect{0, 0, int32(pw), int32(ph)}
		call(pAdjustWindowRectExForDpi, uintptr(unsafe.Pointer(&r)), style, 0, ex, uintptr(96*scale+0.5))
		pw, ph = r.w(), r.h()
	}
	x, y := -32000, -32000
	if o.Kind != KindNotch {
		m := primaryMonitor()
		x, y = int(m.work.Left)+(m.work.w()-pw)/2, int(m.work.Top)+(m.work.h()-ph)/2
	}
	if err := registerClass(class, 0, true); err != nil {
		return nil, err
	}
	w.HWND = call(pCreateWindowExW, ex, uintptr(unsafe.Pointer(w16(class))), uintptr(unsafe.Pointer(w16(o.Title))), style,
		uintptr(x), uintptr(y), uintptr(pw), uintptr(ph), 0, 0, moduleHandle(), 0)
	if w.HWND == 0 {
		return nil, errors.New("win: CreateWindowEx failed")
	}
	loop.wins[w.HWND] = w
	if o.Icon {
		w.setIcons()
	}
	cr := w.clientRect()
	w.size = image.Pt(max(cr.w(), 1), max(cr.h(), 1))
	t, err := newTarget(w.HWND, w.size.X, w.size.Y, o.Kind == KindNotch)
	if err != nil {
		call(pDestroyWindow, w.HWND)
		delete(loop.wins, w.HWND)
		return nil, err
	}
	w.tgt, w.gen = t, t.d.gen
	if o.Kind == KindNotch {
		// DirectComposition owns the pixels; there is nothing for the system to draw.
		w.ApplyStyles(false, true)
		w.disableTransitions()
	}
	call(pWTSRegisterSession, w.HWND, notifyThisSession)
	if !o.Hidden {
		w.Show()
	}
	return w, nil
}

// Close destroys the window.
func (w *Window) Close() {
	if w.gone {
		return
	}
	call(pDestroyWindow, w.HWND)
}

func (w *Window) release() {
	w.gone = true
	call(pWTSUnRegisterSession, w.HWND)
	if w.tgt != nil {
		w.tgt.release()
		w.tgt = nil
	}
	delete(loop.wins, w.HWND)
}

// SetDraw, SetHandlers: the callbacks, for whoever holds the window as an interface.
func (w *Window) SetDraw(f func(gtx layout.Context, scale float32) bool) { w.Draw = f }

// Handlers are a window's callbacks (each may be nil).
type Handlers struct {
	OnClose func() bool
	OnFocus func(bool)
	OnState func()
	OnPress func()
}

func (w *Window) SetHandlers(h Handlers) {
	w.OnClose, w.OnFocus, w.OnState, w.OnPress = h.OnClose, h.OnFocus, h.OnState, h.OnPress
}

// Gone says the window has been destroyed.
func (w *Window) Gone() bool { return w.gone }

// Frames is how many frames have been shown (the self-test's idle check).
func (w *Window) Frames() uint64 { return w.frames.Load() }

func (w *Window) clientRect() rect {
	var r rect
	call(pGetClientRect, w.HWND, uintptr(unsafe.Pointer(&r)))
	return r
}

// Rect is the window's outer rectangle on the screen.
func (w *Window) Rect() (l, t, r, b int) {
	var rc rect
	call(pGetWindowRect, w.HWND, uintptr(unsafe.Pointer(&rc)))
	return int(rc.Left), int(rc.Top), int(rc.Right), int(rc.Bottom)
}

// Scale is physical pixels per logical pixel.
func (w *Window) Scale() float64 {
	if w.ScaleOverride > 0 {
		return w.ScaleOverride
	}
	if dpi := call(pGetDpiForWindow, w.HWND); dpi > 0 {
		return float64(dpi) / 96
	}
	return 1
}

// Show shows the window and, unless it is the notch, gives it the keyboard.
func (w *Window) Show() {
	if w.Kind == KindNotch {
		call(pShowWindow, w.HWND, swShowNoAct)
	} else {
		call(pShowWindow, w.HWND, swShowNormal)
		call(pSetForegroundWindow, w.HWND)
	}
	w.Invalidate()
}

// Hide hides it.
func (w *Window) Hide() { call(pShowWindow, w.HWND, swHide) }

func (w *Window) Visible() bool   { return call(pIsWindowVisible, w.HWND) != 0 }
func (w *Window) Minimized() bool { return call(pIsIconic, w.HWND) != 0 }
func (w *Window) Maximized() bool { return call(pIsZoomed, w.HWND) != 0 }
func (w *Window) Focused() bool   { return call(pGetForegroundWindow) == w.HWND || w.focused }

// Minimize, ToggleMaximize.
func (w *Window) Minimize() { call(pShowWindow, w.HWND, swMinimize) }
func (w *Window) ToggleMaximize() {
	if w.Maximized() {
		call(pShowWindow, w.HWND, swRestore)
	} else {
		call(pShowWindow, w.HWND, swMaximize)
	}
}

// DragMove starts moving the window with the mouse, as a caption press would
// (winit's drag_window).
func (w *Window) DragMove() { w.nc(htCaption) }

// Edge names for ResizeFrom: 1 top, 2 bottom, 3 left, 4 right, then the corners
// top-left, top-right, bottom-left, bottom-right (app.slint's resize callback).
func (w *Window) ResizeFrom(edge int) {
	ht := [...]int{0, htTop, htBottom, htLeft, htRight, htTopLeft, htTopRight, htBottomLeft, htBottomRight}
	if edge >= 1 && edge < len(ht) {
		w.nc(ht[edge])
	}
}

func (w *Window) nc(hit int) {
	// The system's loop takes the pointer from here, and the release never comes to us.
	w.downs = 0
	w.router.Queue(pointer.Event{Kind: pointer.Cancel})
	w.Invalidate()
	call(pReleaseCapture)
	// lParam is the pointer's place on the screen, which the system's sizing loop measures from.
	x, y := Cursor()
	call(pPostMessageW, w.HWND, wmNcLButtonDown, uintptr(hit), uintptr(uint32(uint16(x))|uint32(uint16(y))<<16))
}

// SetSize sets the client area in physical pixels (the notch keeps one size for all its
// states; it changes with Settings → Office size).
func (w *Window) SetSize(pw, ph int) {
	l, t, _, _ := w.Rect()
	call(pSetWindowPos, w.HWND, 0, uintptr(l), uintptr(t), uintptr(pw), uintptr(ph), swpNoZOrder|swpNoActivate)
	w.sync()
}

// Place puts the notch at the rectangle (physical pixels), topmost, without taking the
// keyboard.
func (w *Window) Place(l, t, r, b int) {
	call(pSetWindowPos, w.HWND, ^uintptr(0), uintptr(l), uintptr(t), uintptr(r-l), uintptr(b-t), swpNoActivate|swpShowWindow)
	w.trace("placed at %d,%d %dx%d", l, t, r-l, b-t)
	w.sync()
}

// Raise puts it back on top of the other topmost windows.
func (w *Window) Raise() {
	call(pSetWindowPos, w.HWND, ^uintptr(0), 0, 0, 0, 0, swpNoMove|swpNoSize|swpNoActivate|swpShowWindow)
}

// sync follows a resize at once, so the next frame is the right size.
func (w *Window) sync() {
	if w.Minimized() {
		return
	}
	cr := w.clientRect()
	n := image.Pt(max(cr.w(), 1), max(cr.h(), 1))
	if n == w.size || w.tgt == nil {
		return
	}
	w.size = n
	if err := w.tgt.resize(n.X, n.Y); err != nil {
		logf("window %s: resize to %v: %v; making the swap chain again", w.opts.Class, n, err)
		w.rebuild(err)
	}
}

// ApplyStyles is win.rs apply_styles: WS_EX_TOOLWINDOW always; WS_EX_NOACTIVATE unless the
// office takes keys; the click-through bits while the pointer is off the shape.
func (w *Window) ApplyStyles(acceptsKeys, clickThrough bool) {
	before := uint32(call(pGetWindowLongPtrW, w.HWND, idxExStyle()))
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
	call(pSetWindowLongPtrW, w.HWND, idxExStyle(), uintptr(ex))
	if before&wsExLayered == 0 {
		// A layered window with no attributes is never drawn; fully opaque keeps the
		// DirectComposition content (and its per-pixel alpha) as it is.
		call(pSetLayeredWindowAttributes, w.HWND, 0, 255, lwaAlpha)
	}
}

// idxExStyle is GWL_EXSTYLE, -20, as the unsigned argument Win32 reads it as.
func idxExStyle() uintptr { i := gwlExStyle; return uintptr(i) }

func (w *Window) disableTransitions() {
	on := int32(1)
	call(pDwmSetWindowAttribute, w.HWND, dwmaTransitionsOff, uintptr(unsafe.Pointer(&on)), 4)
}

// Caption is DashboardWindow.ApplyTheme: the title bar's colours follow the panel (dark or
// light, and its colour as 0xAARRGGBB) so bar and window read as one surface. Older
// Windows ignores these and keeps its own.
func (w *Window) Caption(dark bool, panel uint32) {
	set := func(attr int, v uint32) {
		call(pDwmSetWindowAttribute, w.HWND, uintptr(attr), uintptr(unsafe.Pointer(&v)), 4)
	}
	d := uint32(0)
	if dark {
		d = 1
	}
	set(dwmaDarkMode, d)
	// COLORREF is 0x00BBGGRR.
	set(dwmaCaptionColor, (panel&0xFF)<<16|panel&0xFF00|panel>>16&0xFF)
	if dark {
		set(dwmaTextColor, 0x00FFFFFF)
	} else {
		set(dwmaTextColor, 0)
	}
}

// Foreground, SetForeground, IsOurs: the notch's focus rules.
func Foreground() uintptr          { return call(pGetForegroundWindow) }
func SetForeground(h uintptr) bool { return call(pSetForegroundWindow, h) != 0 }
func IsWindow(h uintptr) bool      { return call(pIsWindow, h) != 0 }

// IsOurs: the window, or one it owns (a menu, a dialog).
func IsOurs(h, ours uintptr) bool { return h == ours || call(pGetAncestor, h, gaRootOwner) == ours }

// Focus gives the window the foreground.
func (w *Window) Focus() { call(pSetForegroundWindow, w.HWND); call(pSetFocus, w.HWND) }

// Execute runs a Gio command (a focus request) on the window's input.
func (w *Window) Execute(c input.Command) { w.router.Source().Execute(c) }

// Invalidate asks for a frame. Any goroutine; asks made before it is drawn count once.
func (w *Window) Invalidate() {
	if w.pending.Swap(true) {
		return
	}
	call(pPostMessageW, w.HWND, wmFrame, 0, 0)
}

// rebuild makes the swap chain again after the device was lost (or a resize failed).
func (w *Window) rebuild(cause error) {
	if errors.Is(cause, ErrLost) && w.tgt != nil {
		lose(w.tgt.d)
	}
	if w.tgt != nil {
		w.tgt.release()
		w.tgt = nil
	}
	t, err := newTarget(w.HWND, w.size.X, w.size.Y, w.Kind == KindNotch)
	if err != nil {
		logf("window graphics: %v", err)
		return
	}
	w.tgt, w.gen = t, t.d.gen
}

// paint draws one frame and shows it.
func (w *Window) paint() {
	w.pending.Store(false)
	if w.gone || w.Draw == nil {
		return
	}
	if w.Minimized() {
		return
	}
	if w.tgt == nil || w.gen != shared.gen {
		w.rebuild(nil)
		if w.tgt == nil {
			return
		}
	}
	w.sync()
	if tracing && w.frames.Load() < 3 {
		w.trace("frame %d %v", w.frames.Load(), w.size)
	}
	scale := float32(w.Scale())
	w.ops.Reset()
	gtx := layout.Context{
		Ops: &w.ops, Now: time.Now(), Metric: unit.Metric{PxPerDp: 1, PxPerSp: 1},
		Constraints: layout.Exact(w.size), Source: w.router.Source(),
	}
	animating := w.Draw(gtx, scale)
	w.router.Frame(&w.ops)
	if mime, txt, ok := w.router.WriteClipboard(); ok {
		_ = mime
		SetClipboardText(string(txt))
	}
	if w.router.ClipboardRequested() {
		w.router.Queue(transfer.DataEvent{Type: "application/text", Open: func() io.ReadCloser {
			return io.NopCloser(strings.NewReader(ClipboardText()))
		}})
		animating = true
	}
	w.ime = w.router.EditorState()
	if err := w.tgt.present(&w.ops); err != nil {
		logf("present: %v", err)
		if errors.Is(err, ErrLost) {
			w.rebuild(err)
			animating = true
		}
	}
	w.frames.Add(1)
	w.updateCursor()
	if animating {
		time.AfterFunc(16*time.Millisecond, w.Invalidate)
	}
}

func (w *Window) now() time.Duration { return time.Since(w.t0) }

// trace is HOVER_TRACE=1: what the window sees of the keyboard and focus, for the CI run.
var tracing = os.Getenv("HOVER_TRACE") != ""

func (w *Window) trace(format string, a ...any) {
	if tracing {
		logf("win %s: "+format, append([]any{w.opts.Class + w.opts.Title}, a...)...)
	}
}

func mods() key.Modifiers {
	var m key.Modifiers
	down := func(vk uintptr) bool { return int16(call(pGetKeyState, vk)) < 0 }
	if down(vkLWin) || down(vkRWin) {
		m |= key.ModSuper
	}
	if down(vkMenu) {
		m |= key.ModAlt
	}
	if down(vkControl) {
		m |= key.ModCtrl
	}
	if down(vkShift) {
		m |= key.ModShift
	}
	return m
}

func (w *Window) pointer(kind pointer.Kind, lp uintptr, buttons pointer.Buttons) {
	w.router.Queue(pointer.Event{
		Kind: kind, Source: pointer.Mouse, Buttons: buttons, Modifiers: mods(),
		Position: f32.Pt(float32(lo16(lp)), float32(hi16(lp))), Time: w.now(),
	})
	w.Invalidate()
}

func buttonsOf(wp uintptr) pointer.Buttons {
	var b pointer.Buttons
	if wp&mkLButton != 0 {
		b |= pointer.ButtonPrimary
	}
	if wp&mkRButton != 0 {
		b |= pointer.ButtonSecondary
	}
	if wp&mkMButton != 0 {
		b |= pointer.ButtonTertiary
	}
	return b
}

// editorInsert is Gio's EditorInsert: the text replaces the selection, then the caret goes
// after it. The selection is kept here between frames, so two characters typed before the
// next frame go in order.
func (w *Window) editorInsert(s string) {
	sel := w.ime.Selection.Range
	start, end := min(sel.Start, sel.End), max(sel.Start, sel.End)
	w.router.Queue(key.EditEvent{Range: key.Range{Start: start, End: end}, Text: s})
	caret := start + utf8.RuneCountInString(s)
	w.router.Queue(key.SelectionEvent{Start: caret, End: caret})
	w.ime.Selection.Range = key.Range{Start: caret, End: caret}
	w.Invalidate()
}

func (w *Window) updateCursor() {
	c := w.router.Cursor()
	if c == w.cursor && w.cursorH != 0 {
		return
	}
	w.cursor = c
	w.cursorH = cursorHandle(c)
	if w.inside {
		call(pSetCursor, w.cursorH)
	}
}

func wndProc(hwnd, m, wp, lp uintptr) uintptr {
	if hwnd == loop.hub {
		if m == wmDo {
			drain()
			return 0
		}
		return call(pDefWindowProcW, hwnd, uintptr(m), wp, lp)
	}
	w := loop.wins[hwnd]
	if w == nil {
		return call(pDefWindowProcW, hwnd, m, wp, lp)
	}
	if tracing && uint32(m) == wmActivate {
		w.trace("activate %#x foreground=%#x me=%#x", wp, Foreground(), w.HWND)
	}
	if w.Msg != nil {
		if r, ok := w.Msg(uint32(m), wp, lp); ok {
			return r
		}
	}
	switch uint32(m) {
	case wmFrame:
		w.paint()
		return 0
	case wmPaint:
		var ps [72]byte
		pBeginPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps[0])))
		pEndPaint.Call(hwnd, uintptr(unsafe.Pointer(&ps[0])))
		w.paint()
		return 0
	case wmEraseBkgnd:
		return 1
	case wmSize:
		w.sync()
		w.paint()
		if w.OnState != nil {
			w.OnState()
		}
		return 0
	case wmShowWindow:
		if w.OnState != nil {
			defer w.OnState()
		}
	case wmDpiChanged:
		if w.Kind != KindNotch {
			r := (*rect)(ptr(lp))
			call(pSetWindowPos, w.HWND, 0, uintptr(r.Left), uintptr(r.Top), uintptr(r.w()), uintptr(r.h()), swpNoZOrder|swpNoActivate)
		}
		w.Invalidate()
		return 0
	case wmNcCalcSize:
		if w.Kind == KindFrame {
			// No frame: the client area is the whole window, except that a maximised one
			// overhangs the screen by the frame's width (Raymond Chen's fix: give it the
			// monitor's work area).
			if wp != 0 && w.Maximized() {
				p := (*nccalcsizeParams)(ptr(lp))
				mon := monitorOf(w.HWND)
				p.Rgrc[0] = mon.work
			}
			return 0
		}
	case wmGetMinMaxInfo:
		if w.minW > 0 || w.minH > 0 {
			mm := (*minMaxInfo)(ptr(lp))
			s := w.Scale()
			mm.MinTrack = point{int32(float64(w.minW) * s), int32(float64(w.minH) * s)}
			return 0
		}
	case wmClose:
		if w.OnClose != nil && !w.OnClose() {
			return 0
		}
	case wmDestroy:
		w.release()
		return 0
	case wmSetCursor:
		w.inside = lp&0xFFFF == htClient
		if w.inside {
			if w.cursorH == 0 {
				w.cursorH = cursorHandle(w.cursor)
			}
			call(pSetCursor, w.cursorH)
			return 1
		}
	case wmSetFocus, wmKillFocus:
		on := uint32(m) == wmSetFocus
		w.trace("focus %v", on)
		w.focused = on
		if !on {
			w.router.Queue(pointer.Event{Kind: pointer.Cancel})
		}
		w.router.Queue(key.FocusEvent{Focus: on})
		w.Invalidate()
		if w.OnFocus != nil {
			w.OnFocus(on)
		}
	case wmCancelMode:
		w.router.Queue(pointer.Event{Kind: pointer.Cancel})
		w.downs = 0
		w.Invalidate()
	case wmCaptureChanged:
		if w.downs > 0 {
			w.downs = 0
			w.router.Queue(pointer.Event{Kind: pointer.Cancel})
			w.Invalidate()
		}
	case wmMouseMove:
		if !w.tracked {
			tme := trackMouse{Size: uint32(unsafe.Sizeof(trackMouse{})), Flags: tmeLeave, Track: hwnd}
			call(pTrackMouseEvent, uintptr(unsafe.Pointer(&tme)))
			w.tracked = true
		}
		w.pointer(pointer.Move, lp, buttonsOf(wp))
		return 0
	case wmMouseLeave:
		w.tracked = false
		// Off every area: hover ends.
		w.router.Queue(pointer.Event{Kind: pointer.Move, Source: pointer.Mouse, Position: f32.Pt(-1e5, -1e5), Time: w.now()})
		w.Invalidate()
		return 0
	case wmLButtonDown, wmRButtonDown, wmMButtonDown:
		if w.OnPress != nil {
			w.OnPress()
		}
		w.downs++
		if w.downs == 1 {
			call(pSetCapture, hwnd)
		}
		b := buttonsOf(wp)
		switch uint32(m) {
		case wmLButtonDown:
			b |= pointer.ButtonPrimary
		case wmRButtonDown:
			b |= pointer.ButtonSecondary
		default:
			b |= pointer.ButtonTertiary
		}
		w.pointer(pointer.Press, lp, b)
		return 0
	case wmLButtonUp, wmRButtonUp, wmMButtonUp:
		w.pointer(pointer.Release, lp, buttonsOf(wp))
		if w.downs > 0 {
			w.downs--
		}
		if w.downs == 0 {
			call(pReleaseCapture)
		}
		return 0
	case wmMouseWheel, wmMouseHWheel:
		// The wheel's position is the screen's.
		p := point{int32(lo16(lp)), int32(hi16(lp))}
		call(pScreenToClient, hwnd, uintptr(unsafe.Pointer(&p)))
		// A notch is 120 and scrolls 60 logical pixels, as Slint's winit backend does.
		d := float32(hi16(wp)) / 120 * 60 * float32(w.Scale())
		var sp f32.Point
		if uint32(m) == wmMouseHWheel {
			sp.X = d
		} else if mods()&key.ModShift != 0 {
			sp.X = -d
		} else {
			sp.Y = -d
		}
		w.router.Queue(pointer.Event{Kind: pointer.Scroll, Source: pointer.Mouse, Position: f32.Pt(float32(p.X), float32(p.Y)),
			Buttons: buttonsOf(uintptr(wp & 0xFFFF)), Scroll: sp, Modifiers: mods(), Time: w.now()})
		w.Invalidate()
		return 0
	case wmKeyDown, wmKeyUp, wmSysKeyDown, wmSysKeyUp:
		if n, ok := keyName(wp); ok {
			w.trace("key %q msg %#x foreground=%v", n, m, Foreground() == w.HWND)
			st := key.Press
			if m == wmKeyUp || m == wmSysKeyUp {
				st = key.Release
			}
			w.key(key.Event{Name: n, Modifiers: mods(), State: st})
			return 0
		}
	case wmChar, wmUniChar:
		if m == wmUniChar && wp == 0xFFFF {
			return 1 // UNICODE_NOCHAR: say WM_UNICHAR is understood
		}
		u := rune(wp)
		switch {
		case m == wmChar && u >= 0xD800 && u < 0xDC00:
			w.surr = uint16(u)
			return 0
		case m == wmChar && u >= 0xDC00 && u < 0xE000:
			if w.surr != 0 {
				u = utf16.DecodeRune(rune(w.surr), u)
			}
			w.surr = 0
		}
		if unicode.IsPrint(u) && u != utf8.RuneError {
			w.editorInsert(string(u))
		}
		return 1
	case 0x106: // WM_SYSCHAR: Alt+letter is not a menu here.
		return 0
	}
	return call(pDefWindowProcW, hwnd, m, wp, lp)
}

// key queues a key event. Tab moves the keyboard between the things that take it when
// nothing used the key, as Gio's own window does.
func (w *Window) key(e key.Event) {
	dir := key.FocusDirection(-1)
	if e.State == key.Press {
		switch {
		case e.Name == key.NameTab && e.Modifiers == 0:
			dir = key.FocusForward
		case e.Name == key.NameTab && e.Modifiers == key.ModShift:
			dir = key.FocusBackward
		}
	}
	if dir != -1 {
		w.router.Queue(input.SystemEvent{Event: e})
		if _, handled := w.router.WakeupTime(); !handled {
			w.router.MoveFocus(dir)
		}
	} else {
		w.router.Queue(e)
	}
	w.Invalidate()
}

// Poke asks for a frame from the message loop's thread: for callbacks that run on it.
func (w *Window) Poke() { w.Invalidate() }

func (w *Window) setIcons() {
	big := icon(int(call(pGetSystemMetrics, smCxIcon)))
	small := icon(int(call(pGetSystemMetrics, smCxSmIcon)))
	call(pSendMessageW, w.HWND, wmSetIcon, 1, big)
	call(pSendMessageW, w.HWND, wmSetIcon, 0, small)
}

// Pre-declared so window.go needs no other file's imports.
var (
	pBeginPaint = user32.NewProc("BeginPaint")
	pEndPaint   = user32.NewProc("EndPaint")
)
