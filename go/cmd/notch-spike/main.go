//go:build windows

// Command notch-spike is phase 0 of the Go port (docs/development/go-port.md): the
// notch as tools/notch-proto draws it, on Gio instead of Slint, in a window of its own.
//
//	notch-spike                                   run it (Alt+N, hover the top centre, Esc)
//	notch-spike --selftest DIR [--wgsl DIR]       drive it from outside, write DIR/report.json
//
// The self-test makes the same checks as notch-proto's, under the same names, so the
// two reports compare line for line on the same machine.
package main

import (
	"flag"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"gioui.org/io/input"
	"gioui.org/io/key"
	"gioui.org/io/pointer"
	"gioui.org/op"
	"gioui.org/widget"
	"gioui.org/widget/material"
	"golang.org/x/sys/windows"

	"github.com/4regab/Hover/go/internal/notch"
)

const (
	shadowBlur  = 24.0
	shadowDepth = 4.0

	timerPoll     = 1
	timerAnim     = 2
	timerSelftest = 3
	timerHardStop = 4
)

type spike struct {
	hwnd         uintptr
	hover        notch.Hover
	open         notch.Openness
	rest         notch.Size
	openSize     notch.Size
	scale        float64
	work, win    notch.Rect
	t0           time.Time
	acceptsKeys  bool
	clickThrough bool
	frames       uint64
	animating    bool
	previous     uintptr
	log          []string

	gfx      *graphics
	ops      op.Ops
	router   input.Router
	theme    *material.Theme
	editor   widget.Editor
	shapeTag int
	office   officeResult
	officeOK error
	view     viewOps
	// editorDrawn: the editor was in the last frame. Gio's EditorState looks the focused
	// editor up among the last frame's handlers and has nothing to read otherwise.
	editorDrawn bool

	test     *selftest
	exitCode int
	surr     uint16 // a UTF-16 high surrogate waiting for its pair (WM_CHAR)
}

var app *spike

func (s *spike) now() float64 { return float64(time.Since(s.t0).Microseconds()) / 1000 }

func (s *spike) logf(format string, a ...any) {
	line := fmt.Sprintf(format, a...)
	fmt.Fprintf(os.Stderr, "[%7.0f ms] %s\n", s.now(), line)
	s.log = append(s.log, fmt.Sprintf("%.0f %s", s.now(), line))
}

func main() {
	selftestDir := flag.String("selftest", "", "drive the notch from outside and write report.json here")
	wgslDir := flag.String("wgsl", "", "folder with the office's office.wgsl and page.wgsl, to compile them")
	restKind := flag.String("rest", "pill", "what the resting notch shows: none, pill or alert")
	helperBg := flag.String("helper-bg", "", "internal: run the self-test's helper window at left,top,right,bottom")
	flag.Parse()
	runtime.LockOSThread()
	setDpiAware()
	if *helperBg != "" {
		runHelper(*helperBg)
		return
	}

	rest := notch.Rest{Kind: notch.RestPill, W: 150}
	switch *restKind {
	case "none":
		rest = notch.Rest{Kind: notch.RestNone}
	case "alert":
		// The question's card (the alert it replaced is gone).
		rest = notch.Rest{Kind: notch.RestCard, W: 500, H: 182}
	}
	s := &spike{
		hover: notch.NewHover(), open: notch.NewOpenness(), rest: notch.RestSize(rest),
		openSize: notch.Size{W: 1120, H: 440}, scale: 1, t0: time.Now(), clickThrough: true,
		theme: newTheme(), editor: widget.Editor{SingleLine: true},
	}
	app = s
	s.office, s.officeOK = renderOffice(*wgslDir)
	if s.officeOK != nil {
		s.logf("office stand-in: %v", s.officeOK)
	} else {
		s.logf("office stand-in drawn on %s", s.office.adapter)
	}
	if err := s.create(); err != nil {
		fmt.Fprintln(os.Stderr, "notch window:", err)
		os.Exit(3)
	}
	if *selftestDir != "" {
		s.test = startSelftest(s, *selftestDir)
	}
	var m msg
	for call(pGetMessageW, uintptr(unsafe.Pointer(&m)), 0, 0, 0) > 0 {
		call(pTranslateMessage, uintptr(unsafe.Pointer(&m)))
		call(pDispatchMessageW, uintptr(unsafe.Pointer(&m)))
	}
	os.Exit(s.exitCode)
}

func (s *spike) create() error {
	inst := moduleHandle()
	class, _ := windows.UTF16PtrFromString("HoverNotch")
	wc := wndClassEx{
		Size: uint32(unsafe.Sizeof(wndClassEx{})), WndProc: windows.NewCallback(wndProc),
		Instance: uintptr(inst), Cursor: call(pLoadCursorW, 0, idcArrow), ClassName: class,
	}
	if call(pRegisterClassExW, uintptr(unsafe.Pointer(&wc))) == 0 {
		return fmt.Errorf("RegisterClassExW failed")
	}
	m := primary()
	s.work = toNotch(m.work)
	s.scale = m.scale
	s.openSize = notch.OpenSize(notch.SizeDefault, notch.Size{W: float64(s.work.Width()) / s.scale, H: float64(s.work.Height()) / s.scale})
	s.win = notch.Placement(s.work, s.scale, s.openSize)
	title, _ := windows.UTF16PtrFromString("Hover notch")
	// Hidden until placed, as notch-proto keeps it off screen until then.
	s.hwnd = call(pCreateWindowExW,
		wsExNoRedirectionBitmap|wsExToolWindow|wsExTopmost|wsExNoActivate,
		uintptr(unsafe.Pointer(class)), uintptr(unsafe.Pointer(title)), wsPopup,
		uintptr(s.win.Left), uintptr(s.win.Top), uintptr(s.win.Width()), uintptr(s.win.Height()),
		0, 0, uintptr(inst), 0)
	if s.hwnd == 0 {
		return fmt.Errorf("CreateWindowExW failed")
	}
	g, err := newGraphics(s.hwnd, s.win.Width(), s.win.Height())
	if err != nil {
		return err
	}
	s.gfx = g
	s.logf("graphics: Direct3D 11 (%s) through DirectComposition, Gio's gpu package", g.driver)
	s.logf("hotkey Alt+N registered: %v", registerAltN(s.hwnd))
	s.frame()
	applyStyles(s.hwnd, false, true)
	disableTransitions(s.hwnd)
	place(s.hwnd, toRect(s.win))
	call(pSetTimer, s.hwnd, timerPoll, notch.PollMS, 0)
	return nil
}

func toNotch(r rect) notch.Rect {
	return notch.Rect{Left: int(r.Left), Top: int(r.Top), Right: int(r.Right), Bottom: int(r.Bottom)}
}
func toRect(r notch.Rect) rect {
	return rect{int32(r.Left), int32(r.Top), int32(r.Right), int32(r.Bottom)}
}

// frame lays the notch out, lets Gio's widgets see the input queued since the last one,
// and shows it.
func (s *spike) frame() {
	s.layout()
	if err := s.gfx.present(&s.ops); err != nil {
		s.logf("present: %v", err)
		return
	}
	s.frames++
}

func (s *spike) animate() {
	if s.animating {
		return
	}
	s.animating = true
	call(pSetTimer, s.hwnd, timerAnim, 16, 0)
}

// expand is NotchManager.Expand: NOACTIVATE comes off before the animation starts.
func (s *spike) expand(peek, focus bool) {
	if s.hover.State == notch.StateRest {
		s.previous = foreground()
		s.acceptsKeys = true
		applyStyles(s.hwnd, true, s.clickThrough)
		raise(s.hwnd)
	}
	s.hover.Opened(peek)
	s.open.Go(1, s.now())
	s.logf("expand -> %s (focus %v)", map[bool]string{true: "peek", false: "open"}[s.hover.State == notch.StatePeek], focus)
	s.animate()
	if focus {
		setForeground(s.hwnd)
		s.router.Source().Execute(key.FocusCmd{Tag: &s.editor})
	}
}

// collapse goes back to rest; the app that had the foreground gets it back.
func (s *spike) collapse() {
	if s.hover.State == notch.StateRest {
		return
	}
	s.hover.Collapsed()
	if foreground() == s.hwnd && s.previous != 0 && isWindow(s.previous) {
		setForeground(s.previous)
	}
	s.acceptsKeys = false
	applyStyles(s.hwnd, false, s.clickThrough)
	s.router.Source().Execute(key.FocusCmd{Tag: nil})
	s.open.Go(0, s.now())
	s.logf("collapse")
	s.animate()
}

func (s *spike) toggle() {
	if s.hover.State == notch.StateRest {
		s.expand(false, true)
	} else {
		s.collapse()
	}
}

// poll is the 50 ms poll (DispatcherPriority.Normal in the C#): pointer, hover rules,
// click-through.
func (s *spike) poll() {
	x, y := cursor()
	p := notch.Pointer{
		InZone:     notch.Zone(s.work, s.scale, s.rest).Contains(x, y),
		InPanel:    notch.PanelZone(s.work, s.scale, s.openSize).Contains(x, y),
		Buttons:    buttonsDown(),
		HoverOpens: true,
	}
	switch s.hover.Poll(int64(s.now()), p) {
	case notch.ActPeek:
		s.expand(true, false)
	case notch.ActCollapse:
		s.collapse()
	}
	// Click-through: the window takes the pointer only over the shape (and its shadow).
	f := notch.FrameAt(s.open.Value(s.now()), s.rest, s.openSize)
	dx, dy := float64(x-s.win.Left)/s.scale, float64(y-s.win.Top)/s.scale
	over := s.win.Contains(x, y) && notch.Hittable(dx, dy, s.openSize.W+2*notch.Pad, f, shadowBlur, shadowDepth)
	if through := !over; through != s.clickThrough {
		s.clickThrough = through
		applyStyles(s.hwnd, s.acceptsKeys, through)
	}
}

func (s *spike) pointerEvent(kind pointer.Kind, lp uintptr) {
	x, y := int16(lp&0xFFFF), int16(lp>>16&0xFFFF)
	var b pointer.Buttons
	if kind == pointer.Press || buttonsDown() {
		b = pointer.ButtonPrimary
	}
	s.router.Queue(pointer.Event{Kind: kind, Source: pointer.Mouse, Buttons: b, Position: fpt(float32(x), float32(y)), Time: time.Since(s.t0)})
}

func wndProc(hwnd, m, wp, lp uintptr) uintptr {
	s := app
	if s == nil || s.hwnd == 0 {
		return call(pDefWindowProcW, hwnd, m, wp, lp)
	}
	switch m {
	case wmTimer:
		switch wp {
		case timerPoll:
			s.poll()
		case timerAnim:
			s.frame()
			if !s.open.Animating(s.now()) {
				s.frame()
				s.animating = false
				call(pKillTimer, hwnd, timerAnim)
			}
		case timerSelftest:
			s.test.tick()
		case timerHardStop:
			s.test.finish()
		}
		return 0
	case wmHotKey:
		if wp == 1 {
			s.toggle()
		}
		return 0
	case wmActivate:
		// Click-away: only when the new foreground isn't ours (or owned by us).
		if wp&0xFFFF == waInactive && s.hover.State != notch.StateRest {
			if fg := foreground(); fg != 0 && !isOurs(fg, s.hwnd) {
				s.collapse()
			}
		}
	case wmMouseMove:
		s.pointerEvent(pointer.Move, lp)
		return 0
	case wmLButtonDown:
		s.pointerEvent(pointer.Press, lp)
		s.frame()
		return 0
	case wmLButtonUp:
		s.pointerEvent(pointer.Release, lp)
		s.frame()
		return 0
	case wmKeyDown:
		if wp == vkEscape {
			s.collapse()
			return 0
		}
	case wmChar:
		u := uint16(wp)
		switch {
		case u < 0x20 || u == 0x7F:
			return 0 // controls; Escape was handled as a key
		case u >= 0xD800 && u < 0xDC00:
			s.surr = u
			return 0
		}
		text := string(windows.UTF16ToString([]uint16{u}))
		if u >= 0xDC00 && u < 0xE000 && s.surr != 0 {
			text = windows.UTF16ToString([]uint16{s.surr, u})
		}
		s.surr = 0
		if !s.editorDrawn {
			return 0
		}
		// The text replaces the editor's own selection. Run 2 took the router's
		// EditorState instead and put every character at 0 ("本日ÎÅ olleh"); the log
		// line shows both, so the next run proves which one follows the caret.
		a, b := s.editor.Selection()
		rs := s.router.EditorState().Selection.Range
		s.logf("edit %q: editor's selection %d..%d, router's %d..%d", text, a, b, rs.Start, rs.End)
		s.router.Queue(key.EditEvent{Range: key.Range{Start: min(a, b), End: max(a, b)}, Text: text})
		s.frame()
		return 0
	case wmEraseBkgnd:
		return 1
	case wmPaint:
		// DirectComposition shows the last frame; there is nothing to repaint.
		var ps paintStruct
		call(pBeginPaint, hwnd, uintptr(unsafe.Pointer(&ps)))
		call(pEndPaint, hwnd, uintptr(unsafe.Pointer(&ps)))
		return 0
	case wmDestroy:
		call(pPostQuitMessage, 0)
		return 0
	}
	return call(pDefWindowProcW, hwnd, m, wp, lp)
}

func parseRect(s string) (rect, bool) {
	var v [4]int32
	parts := strings.Split(s, ",")
	if len(parts) != 4 {
		return rect{}, false
	}
	for i, p := range parts {
		n, err := strconv.Atoi(strings.TrimSpace(p))
		if err != nil {
			return rect{}, false
		}
		v[i] = int32(n)
	}
	return rect{v[0], v[1], v[2], v[3]}, true
}
