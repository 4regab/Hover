//go:build windows

package main

// notch-proto's self-test, step for step and at the same times: it drives the notch the
// way a person does and watches it from another process. The helper window underneath
// gets the clicks that pass through, the screen shows what DWM composed, and the
// foreground says who has the keyboard. The check names are notch-proto's, so the two
// report.json files compare line for line.

import (
	"bufio"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"

	"github.com/4regab/Hover/internal/notch"
)

type step struct {
	at   time.Duration
	run  func()
	done bool
}

type selftest struct {
	s          *spike
	out        string
	report     map[string]any
	helper     *exec.Cmd
	mu         sync.Mutex
	lines      []string
	helperHwnd uintptr
	markClicks int
	markFrames uint64
	markCPU    float64
	steps      []*step
	t0         time.Time
	finished   bool
}

func (t *selftest) clicks() int {
	t.mu.Lock()
	defer t.mu.Unlock()
	n := 0
	for _, l := range t.lines {
		if strings.HasPrefix(l, "click") {
			n++
		}
	}
	return n
}

func (t *selftest) check(name string, pass bool, detail map[string]any) {
	word := "FAIL"
	if pass {
		word = "PASS"
	}
	d, _ := json.Marshal(detail)
	fmt.Fprintf(os.Stderr, "%s %s %s\n", word, name, d)
	t.report[name] = map[string]any{"pass": pass, "detail": detail}
}

func (t *selftest) shot(name string, r notch.Rect) *image.RGBA {
	img := capture(toRect(r))
	if f, err := os.Create(filepath.Join(t.out, name+".png")); err == nil {
		png.Encode(f, img)
		f.Close()
	}
	return img
}

func px(img *image.RGBA, x, y int) [3]uint8 {
	if !(image.Point{x, y}.In(img.Bounds())) {
		return [3]uint8{}
	}
	c := img.RGBAAt(x, y)
	return [3]uint8{c.R, c.G, c.B}
}

func magenta(c [3]uint8) bool { return c[0] > 230 && c[1] < 30 && c[2] > 230 }
func dark(c [3]uint8) bool    { return int(c[0])+int(c[1])+int(c[2]) < 60 }

// geometry in device pixels: the window, the scale, the centre x and the open size.
func (t *selftest) g() (notch.Rect, float64, int, notch.Size) {
	s := t.s
	return s.win, s.scale, s.win.Left + s.win.Width()/2, s.openSize
}

func clickAt(x, y int) {
	moveTo(x, y)
	time.Sleep(30 * time.Millisecond)
	click(x, y)
}

// click presses where the pointer already is.
func click(x, y int) {
	// Evidence for the click-on-pill check: whether the window was still click-through
	// (WS_EX_TRANSPARENT) when the click went in, and what Windows puts under it.
	if app != nil {
		ex := exStyle(app.hwnd)
		app.logf("click at %d,%d: exstyle %#x, click-through %v, window under it is the notch %v",
			x, y, ex, ex&wsExTransparent != 0, windowFromPoint(x, y) == app.hwnd)
	}
	sendMouse(mouseInput{Typ: inputMouse, Flags: mouseLeftDown}, mouseInput{Typ: inputMouse, Flags: mouseLeftUp})
}

func startSelftest(s *spike, dir string) *selftest {
	os.MkdirAll(dir, 0o755)
	t := &selftest{s: s, out: dir, report: map[string]any{}, t0: time.Now()}
	at := func(ms int, f func()) {
		t.steps = append(t.steps, &step{at: time.Duration(ms) * time.Millisecond, run: f})
	}

	at(300, func() {
		r, _, _, _ := t.g()
		cover := fmt.Sprintf("%d,%d,%d,%d", r.Left-60, r.Top, r.Right+60, r.Bottom+120)
		exe, _ := os.Executable()
		cmd := exec.Command(exe, "--helper-bg", cover)
		out, err := cmd.StdoutPipe()
		if err == nil && cmd.Start() == nil {
			t.helper = cmd
			go func() {
				sc := bufio.NewScanner(out)
				for sc.Scan() {
					t.mu.Lock()
					t.lines = append(t.lines, sc.Text())
					t.mu.Unlock()
				}
			}()
		} else {
			s.logf("helper did not start: %v", err)
		}
	})
	at(2000, func() {
		t.mu.Lock()
		for _, l := range t.lines {
			if v, ok := strings.CutPrefix(l, "hwnd "); ok {
				n, _ := strconv.ParseUint(v, 10, 64)
				t.helperHwnd = uintptr(n)
			}
		}
		t.mu.Unlock()
		ex := exStyle(s.hwnd)
		actual := toNotch(windowRect(s.hwnd))
		m := primary()
		t.report["monitors"] = []any{map[string]any{"primary": true, "bounds": fmt.Sprint(m.bounds), "work": fmt.Sprint(m.work), "scale": m.scale}}
		t.report["adapter_note"] = fmt.Sprintf("Gio on Direct3D 11 (%s); the office stand-in on wgpu: %s", s.gfx.driver, s.office.adapter)
		t.check("placement_primary_work_area", actual == s.win, map[string]any{"expected": fmt.Sprint(s.win), "actual": fmt.Sprint(actual), "scale": m.scale})
		t.check("styles_at_rest", ex&0x80 != 0 && ex&0x0800_0000 != 0 && ex&0x8 != 0, map[string]any{"exstyle": fmt.Sprintf("%#x", ex), "want": "TOOLWINDOW|NOACTIVATE|TOPMOST"})
		fg := foreground()
		t.check("resting_notch_leaves_foreground", fg == t.helperHwnd && fg != 0, map[string]any{"foreground": fg, "helper": t.helperHwnd})
	})
	at(2300, func() {
		r, k, cx, _ := t.g()
		img := t.shot("rest", r)
		// notch-proto reads the pill at 12 dp, where the status text crosses the centre
		// (run 2 read a letter, 167,167,167). 28 dp is still the pill, under the text.
		pad, corner, shape := px(img, cx-r.Left, int((24+30)*k)), px(img, 4, r.Height()-4), px(img, cx-r.Left, int(28*k))
		t.check("transparent_over_desktop_at_rest", magenta(pad) && magenta(corner) && dark(shape), map[string]any{"below_pill": pad, "window_corner": corner, "pill": shape, "pill_read_at_dp": 28})
	})
	at(2600, func() {
		r, k, cx, _ := t.g()
		t.markClicks = t.clicks()
		x, y := cx, r.Top+int((24+60)*k)
		wfp := windowFromPoint(x, y)
		t.report["window_from_point_below_pill"] = map[string]any{"hwnd": wfp, "is_helper": wfp == t.helperHwnd}
		clickAt(x, y)
	})
	at(3000, func() {
		n := t.clicks() - t.markClicks
		t.check("click_through_empty_area", n == 1, map[string]any{"helper_clicks": n})
		t.markClicks = t.clicks()
	})
	// notch-proto moves and clicks in one step, 30 ms apart on this thread, so the 50 ms
	// poll can't run in between and the click passes through (run 2's log: click-through
	// true). Here the poll gets its turn first, and the click still lands before the
	// 120 ms dwell would peek.
	at(3100, func() {
		r, k, cx, _ := t.g()
		moveTo(cx, r.Top+int(12*k))
	})
	at(3170, func() {
		r, k, cx, _ := t.g()
		click(cx, r.Top+int(12*k))
	})
	at(3800, func() {
		n := t.clicks() - t.markClicks
		ex := exStyle(s.hwnd)
		fg := foreground()
		state := s.hover.State
		t.check("click_on_pill_opens_without_stealing_focus", n == 0 && state == notch.StateOpen && ex&0x0800_0000 == 0 && fg == t.helperHwnd,
			map[string]any{"helper_clicks": n, "state": state.String(), "exstyle": fmt.Sprintf("%#x", ex), "foreground_is_helper": fg == t.helperHwnd})
		r, k, cx, open := t.g()
		img := t.shot("open", r)
		corner, inside := px(img, 4, r.Height()-4), px(img, cx-r.Left, int(open.H*0.5*k))
		t.check("transparent_over_desktop_open", magenta(corner) && !magenta(inside), map[string]any{"window_corner": corner, "office": inside})
	})
	at(4000, func() {
		r, k, _, open := t.g()
		// The composer box: x 22..432, y open.h-70..open.h-22 inside the view.
		clickAt(r.Left+int((notch.Pad+22+200)*k), r.Top+int((open.H-46)*k))
	})
	at(4300, func() { typeText("hello ÅÎ日本") })
	at(4900, func() {
		fg := foreground()
		draft := s.editor.Text()
		t.check("composer_takes_focus_and_text", fg == s.hwnd && draft == "hello ÅÎ日本", map[string]any{"draft": draft, "foreground_is_notch": fg == s.hwnd})
		escape()
	})
	at(5400, func() {
		ex := exStyle(s.hwnd)
		fg := foreground()
		state := s.hover.State
		t.check("esc_collapses_and_restores_foreground", state == notch.StateRest && ex&0x0800_0000 != 0 && fg == t.helperHwnd,
			map[string]any{"state": state.String(), "exstyle": fmt.Sprintf("%#x", ex), "foreground_is_helper": fg == t.helperHwnd})
		r, k, cx, _ := t.g()
		img := t.shot("collapsed", r)
		t.check("transparent_after_collapse", magenta(px(img, cx-r.Left, int((24+30)*k))), map[string]any{})
		moveTo(cx, r.Top+1)
	})
	at(5900, func() {
		state := s.hover.State
		fg := foreground()
		t.check("hover_peeks_without_activating", state == notch.StatePeek && fg == t.helperHwnd, map[string]any{"state": state.String(), "foreground_is_helper": fg == t.helperHwnd})
		r, k, cx, open := t.g()
		moveTo(cx, r.Top+int((open.H+120)*k))
	})
	at(6700, func() {
		state := s.hover.State
		t.check("peek_folds_after_leave_grace", state == notch.StateRest, map[string]any{"state": state.String()})
		altN()
	})
	at(7300, func() { typeText("x") })
	at(7700, func() {
		fg := foreground()
		state := s.hover.State
		draft := s.editor.Text()
		t.check("hotkey_opens_with_keyboard", state == notch.StateOpen && fg == s.hwnd && strings.HasSuffix(draft, "x"),
			map[string]any{"state": state.String(), "foreground_is_notch": fg == s.hwnd, "draft": draft})
		escape()
	})
	at(8500, func() {
		t.markFrames, t.markCPU = s.frames, cpuMS()
	})
	at(11500, func() {
		frames := s.frames - t.markFrames
		cpu := cpuMS() - t.markCPU
		var ms runtime.MemStats
		runtime.ReadMemStats(&ms)
		t.check("no_redraws_while_resting", frames == 0, map[string]any{
			"frames_in_3s": frames, "cpu_ms_in_3s": cpu, "private_bytes": privateBytes(),
			"go_heap_inuse": ms.HeapInuse, "go_sys": ms.Sys,
		})
		t.finish()
	})
	// Go only: the office's GPU path and its shaders.
	// The centre of sceneWGSL's background is vec3f(0.165, 0.094, 0.141): 42, 24, 36.
	var centre [3]uint8
	if s.office.img != nil {
		centre = px(s.office.img, 552, 190)
	}
	near := func(a uint8, b int) bool { return int(a) >= b-4 && int(a) <= b+4 }
	t.check("office_standin_rendered", s.officeOK == nil && near(centre[0], 42) && near(centre[1], 24) && near(centre[2], 36),
		map[string]any{"error": fmt.Sprint(s.officeOK), "adapter": s.office.adapter, "centre": centre})
	compiled := len(s.office.shaders) == 2
	for _, msg := range s.office.shaders {
		compiled = compiled && msg == ""
	}
	t.check("office_wgsl_compiles", compiled, map[string]any{"shaders": s.office.shaders})

	call(pSetTimer, s.hwnd, timerSelftest, 20, 0)
	// A hard stop, whatever happens.
	call(pSetTimer, s.hwnd, timerHardStop, 40_000, 0)
	return t
}

func (t *selftest) tick() {
	now := time.Since(t.t0)
	for _, st := range t.steps {
		if !st.done && st.at <= now {
			st.done = true
			st.run()
		}
	}
}

func (t *selftest) finish() {
	if t.finished {
		return
	}
	t.finished = true
	t.report["log"] = t.s.log
	b, _ := json.MarshalIndent(t.report, "", "  ")
	os.WriteFile(filepath.Join(t.out, "report.json"), b, 0o644)
	if t.helper != nil && t.helper.Process != nil {
		t.helper.Process.Kill()
	}
	fails := 0
	for _, v := range t.report {
		if m, ok := v.(map[string]any); ok && m["pass"] == false {
			fails++
		}
	}
	fmt.Fprintf(os.Stderr, "self-test done: %d failed; report in %s\n", fails, t.out)
	t.s.exitCode = min(fails, 100)
	call(pPostQuitMessage, uintptr(t.s.exitCode))
}

// ---- the helper: "the app underneath", in its own process ----------------------------

var helperClicks int

func helperProc(h, m, wp, lp uintptr) uintptr {
	switch m {
	case wmLButtonDown:
		helperClicks++
		fmt.Printf("click %d %d %d\n", helperClicks, int16(lp&0xFFFF), int16(lp>>16&0xFFFF))
		return 0
	case wmActivate:
		fmt.Printf("active %v\n", wp&0xFFFF != 0)
	case wmEraseBkgnd:
		var r rect
		call(pGetClientRect, h, uintptr(unsafe.Pointer(&r)))
		b := call(pCreateSolidBrush, 0x00FF00FF)
		call(pFillRect, wp, uintptr(unsafe.Pointer(&r)), b)
		call(pDeleteObject, b)
		return 1
	case wmDestroy:
		call(pPostQuitMessage, 0)
		return 0
	}
	return call(pDefWindowProcW, h, m, wp, lp)
}

// runHelper is a plain magenta window at r, activated, printing each click to stdout.
func runHelper(spec string) {
	r, ok := parseRect(spec)
	if !ok {
		fmt.Fprintln(os.Stderr, "--helper-bg wants left,top,right,bottom")
		os.Exit(2)
	}
	inst := moduleHandle()
	class, _ := windows.UTF16PtrFromString("HoverNotchSpikeHelper")
	title, _ := windows.UTF16PtrFromString("Notch test: the app underneath")
	wc := wndClassEx{
		Size: uint32(unsafe.Sizeof(wndClassEx{})), WndProc: windows.NewCallback(helperProc),
		Instance: uintptr(inst), Cursor: call(pLoadCursorW, 0, idcArrow), ClassName: class,
	}
	call(pRegisterClassExW, uintptr(unsafe.Pointer(&wc)))
	h := call(pCreateWindowExW, 0, uintptr(unsafe.Pointer(class)), uintptr(unsafe.Pointer(title)), wsPopup|0x10000000,
		uintptr(r.Left), uintptr(r.Top), uintptr(r.Right-r.Left), uintptr(r.Bottom-r.Top), 0, 0, uintptr(inst), 0)
	call(pShowWindow, h, swShow)
	setForeground(h)
	fmt.Printf("hwnd %d\n", h)
	var m msg
	for call(pGetMessageW, uintptr(unsafe.Pointer(&m)), 0, 0, 0) > 0 {
		call(pTranslateMessage, uintptr(unsafe.Pointer(&m)))
		call(pDispatchMessageW, uintptr(unsafe.Pointer(&m)))
	}
}
