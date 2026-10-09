//go:build linux

package shell

import (
	"fmt"
	"math"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/platform/linux"
	"github.com/4regab/Hover/go/internal/platform/wayland"
)

// The Linux build of Env: Wayland only (no X11, no XWayland). Windows are the wayland
// package's (the notch a layer-shell surface), the shortcut the desktop's GlobalShortcuts
// portal, the tray a StatusNotifierItem, the pickers the file chooser portal.

// linuxSys is what InitLinux made.
var linuxSys struct {
	d     *wayland.Display
	loop  *wayland.Loop
	hooks SysHooks
	tray  *linux.Tray
	menu  []linux.MenuItem
	keys  *linux.Shortcuts
	mu    sync.Mutex
}

// InitLinux connects to the compositor. Call it first, from main, before anything else
// here; the loop runs on the goroutine that calls RunLinux.
func InitLinux() error {
	d, err := wayland.Open()
	if err != nil {
		return err
	}
	d.Logf = core.Logf
	l, err := wayland.NewLoop(d)
	if err != nil {
		return err
	}
	linuxSys.d, linuxSys.loop = d, l
	linuxSys.keys = linux.NewShortcuts("")
	return nil
}

// RunLinux is the UI thread's loop: it returns when the app quits or the compositor goes.
func RunLinux() error { return linuxSys.loop.Run() }

// UIDoLinux runs f on the UI thread.
func UIDoLinux(f func()) { linuxSys.loop.UIDo(f) }

type window struct{ *wayland.Win }

func (w window) SetHandlers(h Handlers) {
	w.Win.SetHandlers(wayland.Handlers{OnClose: h.OnClose, OnFocus: h.OnFocus, OnState: h.OnState, OnPress: h.OnPress})
}

type timer struct{ *wayland.Timer }

func (t timer) Stop()         { t.Timer.Stop() }
func (t timer) Running() bool { return t.Timer.Running() }

func wrap(w *wayland.Win, err error) (Window, error) {
	if err != nil {
		return nil, err
	}
	return window{w}, nil
}

// SystemEnv is Env on Wayland.
func SystemEnv() Env {
	d, l := linuxSys.d, linuxSys.loop
	return Env{
		UIDo:  l.UIDo,
		After: func(dur time.Duration, f func()) Timer { return timer{l.After(dur, f)} },
		Every: func(dur time.Duration, f func()) Timer { return timer{l.Every(dur, f)} },
		Quit:  l.Quit,

		OpenURL:        func(u string) { _ = linux.OpenURL(u) },
		PickFolder:     linux.PickFolder,
		PickThemeFile:  linux.PickThemeFile,
		PickImage:      linux.PickImage,
		ClipboardImage: linux.ClipboardImage,
		SetClipboard:   func(t string) { _ = linux.SetClipboard(t) },

		NewNotch: func() (Window, NotchPlat, error) {
			w, err := d.NewWindow(wayland.Options{Kind: wayland.KindNotch, Title: "Hover notch", W: 120, H: 40})
			if err != nil {
				return nil, nil, err
			}
			p := &notchPlat{d: d, w: w}
			w.OnKeyboard = p.keyboard
			return window{w}, p, nil
		},
		NewDashboard: func() (Window, error) {
			return wrap(d.NewWindow(wayland.Options{Kind: wayland.KindFrame, Title: "Hover", W: 1200, H: 620, MinW: 880, MinH: 480}))
		},
		NewWarning: func(title string, w, h float32) (Window, error) {
			return wrap(d.NewWindow(wayland.Options{Kind: wayland.KindDialog, Title: title, W: w, H: h}))
		},

		Bind: func(h SysHooks) {
			linuxSys.mu.Lock()
			linuxSys.hooks = h
			linuxSys.mu.Unlock()
		},
		TrayStart: func() {
			linuxSys.mu.Lock()
			menu := linuxSys.menu
			linuxSys.mu.Unlock()
			tr, err := linux.StartTray("", linux.IconPixmaps(), menu, func(e linux.Event) {
				l.UIDo(func() {
					linuxSys.mu.Lock()
					h := linuxSys.hooks
					linuxSys.mu.Unlock()
					switch {
					case e.Activate && h.TrayLeft != nil:
						h.TrayLeft()
					case !e.Activate && h.TrayMenu != nil:
						h.TrayMenu(e.Item)
					}
				})
			})
			if err != nil {
				core.Logf("tray: %v", err)
				return
			}
			linuxSys.mu.Lock()
			linuxSys.tray = tr
			linuxSys.mu.Unlock()
		},
		TrayStop: func() {
			linuxSys.mu.Lock()
			tr := linuxSys.tray
			linuxSys.tray = nil
			linuxSys.mu.Unlock()
			if tr != nil {
				tr.Close()
			}
			linuxSys.keys.Close()
		},
		SetTrayMenu: func(m app.Menu) {
			items := make([]linux.MenuItem, len(m))
			for i, it := range m {
				if it == nil {
					items[i] = linux.MenuItem{Sep: true}
				} else {
					items[i] = linux.MenuItem{Label: it.Label, Check: it.Check}
				}
			}
			linuxSys.mu.Lock()
			linuxSys.menu = items
			tr := linuxSys.tray
			linuxSys.mu.Unlock()
			if tr != nil {
				tr.SetMenu(items)
			}
		},
		Notify: func(title, body string) {
			go func() {
				if _, err := linux.Notify("", title, body); err != nil {
					core.Logf("notification: %v", err)
				}
			}()
		},

		Hotkey: func(sc core.Shortcut) bool {
			err := bindShortcut("toggle", "Open the Agent Office", &sc, func() { hook(1) }, nil)
			if err != nil {
				core.Logf("hotkey %s: %v", sc.Label(), err)
			}
			return err == nil
		},
		VoiceHotkey: func(sc *core.Shortcut) (func() bool, error) {
			var s *linux.Shortcut
			err := bindShortcutVar("voice", "Hold to talk to Hover", sc, func() { hook(2) }, func() {}, &s)
			if err != nil {
				return nil, err
			}
			if s == nil {
				return nil, nil
			}
			return s.IsDown, nil
		},
	}
}

// hook runs a hotkey hook of the shell on the UI thread.
func hook(id int) {
	linuxSys.loop.UIDo(func() {
		linuxSys.mu.Lock()
		h := linuxSys.hooks
		linuxSys.mu.Unlock()
		if h.Hotkey != nil {
			h.Hotkey(id)
		}
	})
}

func bindShortcut(id, what string, sc *core.Shortcut, down, up func()) error {
	return bindShortcutVar(id, what, sc, down, up, nil)
}

// bindShortcutVar binds the chord (or lets go of id with a nil or unset one) in the desktop's
// portal, off the UI thread: the compositor may ask the user, which can take a while.
func bindShortcutVar(id, what string, sc *core.Shortcut, down, up func(), out **linux.Shortcut) error {
	if sc == nil || !sc.IsSet() {
		return linuxSys.keys.Set(id, nil)
	}
	trig, ok := triggerOf(*sc)
	if !ok {
		return fmt.Errorf("%s has no key the desktop can bind.", sc.Label())
	}
	s := &linux.Shortcut{ID: id, Description: what, Trigger: trig, Down: down, Up: up}
	// ponytail: asked on this thread and waited for; the portal answers at once unless it
	// shows its own dialog. Binding in the background is the upgrade.
	if err := linuxSys.keys.Set(id, s); err != nil {
		return err
	}
	if out != nil {
		*out = s
	}
	return nil
}

// triggerOf is the shortcuts spec's trigger for a chord: "CTRL+ALT+n".
func triggerOf(sc core.Shortcut) (string, bool) {
	name, ok := sc.Key.Name()
	if !ok {
		return "", false
	}
	var key string
	switch {
	case len(name) == 1 && name[0] >= 'A' && name[0] <= 'Z':
		key = strings.ToLower(name)
	case len(name) == 2 && name[0] == 'D' && name[1] >= '0' && name[1] <= '9':
		key = name[1:]
	case len(name) >= 2 && len(name) <= 3 && name[0] == 'F' && name[1] >= '0' && name[1] <= '9':
		key = name
	default:
		key, ok = map[string]string{"Space": "space", "Return": "Return", "Escape": "Escape", "Back": "BackSpace", "Tab": "Tab",
			"Left": "Left", "Right": "Right", "Up": "Up", "Down": "Down", "Delete": "Delete", "Insert": "Insert", "Home": "Home",
			"End": "End", "PageUp": "Prior", "Next": "Next", "Prior": "Prior", "OemComma": "comma", "OemPeriod": "period",
			"OemMinus": "minus", "OemPlus": "plus"}[name]
		if !ok {
			return "", false
		}
	}
	var parts []string
	for _, m := range []struct {
		m    core.Modifiers
		name string
	}{{core.ModControl, "CTRL"}, {core.ModAlt, "ALT"}, {core.ModShift, "SHIFT"}, {core.ModWindows, "LOGO"}} {
		if sc.Modifiers.Has(m.m) {
			parts = append(parts, m.name)
		}
	}
	return strings.Join(append(parts, key), "+"), true
}

// MARK: The notch's place

// notchPlat is NotchPlat on a layer-shell surface at the top of the primary display.
// Wayland lets a program see the pointer only over its own surfaces, so the surface takes
// the pointer only where the notch needs it (SetInput): its hover zone at rest, the panel
// open; the pointer's place is what the surface was last told of it.
type notchPlat struct {
	d    *wayland.Display
	w    *wayland.Win
	win  notch.Rect
	keys atomic.Bool // the notch asks for the keyboard
	had  atomic.Bool // and has had it since
	last []wayland.Rect
}

func (p *notchPlat) out() *wayland.Output { return p.d.Primary() }

func (p *notchPlat) Primary() (notch.Rect, float64) {
	o := p.out()
	return notch.Rect{Right: int(o.W), Bottom: int(o.H)}, float64(o.Scale)
}

func (p *notchPlat) Signature() string {
	var b strings.Builder
	for _, o := range p.d.Outputs() {
		fmt.Fprintf(&b, "%s:%dx%d@%d;", o.Name, o.W, o.H, o.Scale)
	}
	return b.String()
}

const farAway = -1 << 20

func (p *notchPlat) Cursor() (int, int) {
	x, y, in := p.w.PointerPos()
	if !in {
		return farAway, farAway
	}
	s := p.out().Scale
	return p.win.Left + int(x*float64(s)), p.win.Top + int(y*float64(s))
}

func (p *notchPlat) Buttons() bool { return p.w.ButtonDown() }

func (p *notchPlat) Place(r notch.Rect) {
	p.win = r
	s := float64(p.out().Scale)
	p.w.PlaceLayer(int(math.Ceil(float64(r.Width())/s)), int(math.Ceil(float64(r.Height())/s)))
}

func (p *notchPlat) Raise() {}

// SetAcceptsKeys: the open office needs the keyboard; the resting notch must never take it.
func (p *notchPlat) SetAcceptsKeys(on bool) {
	p.keys.Store(on)
	if on {
		p.w.SetKeyboard(wayland.KeyboardExclusive)
	} else {
		p.w.SetKeyboard(wayland.KeyboardNone)
		p.had.Store(false)
	}
}

func (p *notchPlat) RememberForeground() {}
func (p *notchPlat) RestoreForeground()  {}
func (p *notchPlat) Focus() {
	if p.keys.Load() {
		p.w.SetKeyboard(wayland.KeyboardExclusive)
	}
}
func (p *notchPlat) SetHit(over bool)       {}
func (p *notchPlat) ForegroundIsOurs() bool { return p.w.Focused() }

// keyboard is the surface gaining or losing the keyboard. It asks for it exclusively to
// get it; once it has it, it asks on demand, so a click on another window takes it away,
// which is how the open notch learns it was clicked away from (Deactivated).
func (p *notchPlat) keyboard(on bool) {
	if on {
		p.had.Store(true)
		if p.keys.Load() {
			p.w.SetKeyboard(wayland.KeyboardOnDemand)
		}
		return
	}
	if p.had.Swap(false) && p.keys.Load() {
		linuxSys.loop.UIDo(func() {
			linuxSys.mu.Lock()
			h := linuxSys.hooks
			linuxSys.mu.Unlock()
			if h.Deactivated != nil {
				h.Deactivated()
			}
		})
	}
}

// SetInput limits where the surface takes the pointer to these boxes, in device pixels of
// the display.
func (p *notchPlat) SetInput(boxes []notch.Rect) {
	s := p.out().Scale
	lw, lh := p.w.LogicalSize()
	var rs []wayland.Rect
	for _, b := range boxes {
		x0, y0 := floorDiv(b.Left-p.win.Left, int(s)), floorDiv(b.Top-p.win.Top, int(s))
		x1, y1 := -floorDiv(-(b.Right-p.win.Left), int(s)), -floorDiv(-(b.Bottom-p.win.Top), int(s))
		x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, lw), min(y1, lh)
		if x1 > x0 && y1 > y0 {
			rs = append(rs, wayland.Rect{X: int32(x0), Y: int32(y0), W: int32(x1 - x0), H: int32(y1 - y0)})
		}
	}
	if len(rs) == len(p.last) {
		same := true
		for i := range rs {
			same = same && rs[i] == p.last[i]
		}
		if same {
			return
		}
	}
	p.last = rs
	p.w.SetInput(rs)
}

func floorDiv(a, b int) int {
	q := a / b
	if a%b != 0 && (a < 0) != (b < 0) {
		q--
	}
	return q
}
