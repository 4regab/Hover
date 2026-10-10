package office

// live.rs: the office on a goroutine of its own. The model and the renderer live there;
// the UI sends it what happens (Hover's state, the pointer, a resize, the drawer or a
// panel opening, being shown or hidden) and gets back each frame with the tags and what
// the pointer is over. requestAnimationFrame's role: frames are made only when the page's
// pacing wants one, and none while hidden.
//
// ponytail: the Rust office can keep the frame on the GPU when it shares the app's device
// (its slots, In::NoGpu). Gio has a device of its own, so here every frame is read back
// and composed on the CPU, as the Rust office does on Linux.

import (
	"math"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// In is what the UI tells the office: one of the In* types.
type In interface{ in() }

type (
	InState   struct{ J any }
	InResize  struct{ W, H uint32 }
	InPointer struct {
		P  [2]float64
		On bool // false: the pointer left
	}
	InDown        struct{ X, Y float64 }
	InUp          struct{}
	InWheel       struct{ DY, X, Y float64 }
	InKey         struct{ C rune }
	InDoubleClick struct{}
	InVisible     struct{ V bool }
	// InDrawer opens the drawer on a session (Open) or closes it.
	InDrawer struct {
		ID   int64
		Open bool
	}
	// InPanel is the panel open over the office ("board", "tv", "history"), "" for none.
	InPanel struct{ P string }
	// InDeskSel is the session whose desk card or panel is open (its bot shows as hot).
	InDeskSel struct {
		ID  int64
		Set bool
	}
	// InTime picks the time of day; nil follows the clock.
	InTime struct{ T *Time }
	// InView is the user's camera, when the page is made again (office.view).
	InView struct{ V [3]float64 }
	inQuit struct{}
)

func (InState) in()       {}
func (InResize) in()      {}
func (InPointer) in()     {}
func (InDown) in()        {}
func (InUp) in()          {}
func (InWheel) in()       {}
func (InKey) in()         {}
func (InDoubleClick) in() {}
func (InVisible) in()     {}
func (InDrawer) in()      {}
func (InPanel) in()       {}
func (InDeskSel) in()     {}
func (InTime) in()        {}
func (InView) in()        {}
func (inQuit) in()        {}

// Out is a frame and what goes with it.
type Out struct {
	W, H       uint32
	Tags       []Tag
	Hovered    Hover
	Hint       string
	Pointer    [2]float64
	HasPointer bool
	// Clicks is every click since the UI last took a frame, oldest first (a frame the UI
	// hadn't taken yet is replaced by the next, but its clicks carry over).
	Clicks  []Click
	Day     bool
	Frames  uint64
	Adapter string
	Error   string
	// View is the user's camera (office.view), kept by the app across a drop.
	View [3]float64
	// RGB is the page's picture: the frame over the background, with the vignette.
	// Empty when only clicks came. Hand it back with Live.Recycle once drawn.
	RGB []byte
}

// Live is the office's goroutine, seen from the UI.
type Live struct {
	tx    chan In
	done  chan struct{}
	mu    sync.Mutex
	out   Out
	spare []byte
	once  sync.Once
}

// StartLive starts the office. wake is called (off the UI goroutine) when a new frame
// is waiting.
func StartLive(w, h uint32, still bool, wake func()) *Live {
	l := &Live{tx: make(chan In, 1024), done: make(chan struct{})}
	go func() {
		defer close(l.done)
		l.run(w, h, still, wake)
	}()
	return l
}

// Send hands the office a message; after Close it is dropped.
func (l *Live) Send(m In) {
	select {
	case l.tx <- m:
	case <-l.done:
	}
}

// Take is the waiting frame (and empties it).
func (l *Live) Take() Out {
	l.mu.Lock()
	defer l.mu.Unlock()
	o := l.out
	l.out = Out{}
	return o
}

// Recycle hands a drawn frame's picture back: its buffer is used again for a later frame.
func (l *Live) Recycle(rgb []byte) {
	if cap(rgb) > 0 {
		l.mu.Lock()
		l.spare = rgb
		l.mu.Unlock()
	}
}

// Close stops the office and frees its GPU device. It doesn't wait for it.
func (l *Live) Close() { l.once.Do(func() { l.Send(inQuit{}) }) }

func hourNow() int64 { return int64(time.Now().Hour()) }

func (l *Live) run(w, h uint32, still bool, wake func()) {
	o := NewOffice(float64(w), float64(h), still)
	r, err := NewRenderer(w, h)
	if err != nil {
		l.mu.Lock()
		l.out.Error = err.Error()
		l.mu.Unlock()
		wake()
		return
	}
	defer r.Close()
	// On the CPU adapter (WARP on a VM or an RDP host with no GPU) a full-size frame took
	// about 75 ms of every core: the office at half size costs a quarter of that, and is
	// shown scaled up (pixelated, as the voxels are).
	scale := 1.0
	if r.Software {
		scale = 0.5
	}
	px := func(n uint32) uint32 { return max(uint32(math.Round(float64(n)*scale)), 1) }
	if scale != 1 {
		r.Resize(px(w), px(h))
	}
	o.ApplyTime(AutoTime(hourNow()))
	core.Logf("office: the frame is read back and composed on the CPU")
	t0 := time.Now()
	last := 0.0
	visible := true
	var down, prev [2]float64
	isDown := false
	var clicks []Click
	timeCheck := time.Now()
	var page Composer
	// The frame as read back, kept between frames.
	var rgba []byte
	const frameDur = 16667 * time.Microsecond // 60 fps continuous
	nextFrame := time.Now()
	timer := time.NewTimer(0)
	defer timer.Stop()
	for {
		timeout := 500 * time.Millisecond
		if visible {
			timeout = max(time.Until(nextFrame), 0)
		}
		var msgs []In
		if timeout == 0 {
			select {
			case m := <-l.tx:
				msgs = append(msgs, m)
			default:
			}
		} else {
			timer.Reset(timeout)
			select {
			case m := <-l.tx:
				msgs = append(msgs, m)
			case <-timer.C:
			}
		}
	drain:
		for {
			select {
			case m := <-l.tx:
				msgs = append(msgs, m)
			default:
				break drain
			}
		}
		for _, m := range msgs {
			o.Poke()
			switch m := m.(type) {
			case inQuit:
				return
			case InState:
				o.State(m.J)
			case InResize:
				if m.W > 0 && m.H > 0 {
					o.Resize(float64(m.W), float64(m.H))
					r.Resize(px(m.W), px(m.H))
				}
			case InPointer:
				if isDown && m.On {
					if !o.Dragging && math.Hypot(m.P[0]-down[0], m.P[1]-down[1]) > 6 && !o.DrawerOpen && o.Panel == "" {
						o.Dragging = true
					}
					if o.Dragging {
						o.Drag(m.P[0]-prev[0], m.P[1]-prev[1])
					}
				}
				if m.On {
					prev = m.P
				}
				o.Pointer, o.HasPointer = m.P, m.On
			case InDown:
				down, prev, isDown = [2]float64{m.X, m.Y}, [2]float64{m.X, m.Y}, true
				o.Dragging = false
			case InUp:
				was := o.Dragging
				o.Dragging = false
				isDown = false
				if !was {
					o.Pick()
					c := o.Click()
					if c.Kind == ClickTime {
						t := c.Time
						o.ManualTime = &t
						o.ApplyTime(t)
					}
					clicks = append(clicks, c)
				}
			case InWheel:
				if !o.DrawerOpen && o.Panel == "" {
					o.ZoomBy(math.Exp(-m.DY*0.0015), m.X-o.W/2, -(m.Y - o.H/2))
				}
			case InKey:
				switch m.C {
				case '+', '=':
					o.ZoomBy(1.25, 0, 0)
				case '-':
					o.ZoomBy(0.8, 0, 0)
				case '0':
					o.ResetView()
				}
			case InDoubleClick:
				if o.Hovered.Kind == HoverNone && !o.DrawerOpen && o.Panel == "" {
					o.ResetView()
				}
			case InVisible:
				visible = m.V
			case InDrawer:
				o.DrawerOpen = m.Open
				o.Sel, o.HasSel = m.ID, m.Open
				o.DrawTV(o.ClockT)
			case InPanel:
				o.Panel = m.P
			case InDeskSel:
				o.DeskOpen, o.HasDeskOpen = m.ID, m.Set
			case InView:
				o.User = m.V
				o.ClampView()
				o.Cam = [4]float64{m.V[0], 1.7, m.V[1], m.V[2]}
			case InTime:
				o.ManualTime = m.T
				if m.T != nil {
					o.ApplyTime(*m.T)
				} else {
					o.ApplyTime(AutoTime(hourNow()))
				}
			}
		}
		// The time of day follows the clock unless picked, checked every minute.
		if time.Since(timeCheck) > time.Minute {
			timeCheck = time.Now()
			if o.ManualTime == nil {
				if t := AutoTime(hourNow()); t != o.Time {
					o.ApplyTime(t)
				}
			}
		}
		if !visible {
			last = float64(time.Since(t0).Microseconds()) / 1000
			nextFrame = time.Now().Add(frameDur)
			continue
		}
		now := float64(time.Since(t0).Microseconds()) / 1000
		dt := now - last
		last = now
		if !o.Frame(now, dt) && len(clicks) == 0 {
			continue
		}
		if after := time.Now(); nextFrame.Add(frameDur).After(after) {
			nextFrame = nextFrame.Add(frameDur)
		} else {
			nextFrame = after
		}
		errText := ""
		if err := r.RenderInto(o, &rgba); err != nil {
			// Rust's wgpu would panic here and end the office's thread; the error is shown
			// and the office goes on.
			errText = err.Error()
		}
		// The buffer the UI gave back, or a new one.
		l.mu.Lock()
		rgb := l.spare
		l.spare = nil
		l.mu.Unlock()
		day := o.Time == Day
		rgb = page.ComposeInto(rgba, int(r.W), int(r.H), day, rgb)
		hint := ""
		if o.Hovered.Kind == HoverProp {
			if o.Hovered.Prop == PropClock {
				hint = "clock"
			} else {
				hint = o.Hint(o.Hovered.Prop)
			}
		}
		l.mu.Lock()
		// A frame the UI hasn't taken yet: its picture is replaced, its clicks are not.
		all := append(l.out.Clicks, clicks...)
		clicks = nil
		old := l.out.RGB
		l.out = Out{W: r.W, H: r.H, Tags: o.Tags(), Hovered: o.Hovered, Hint: hint, Pointer: o.Pointer, HasPointer: o.HasPointer,
			Clicks: all, Day: day, Frames: o.Frames, Adapter: r.AdapterName, Error: errText, View: o.User, RGB: rgb}
		if cap(old) > 0 && cap(l.spare) == 0 {
			l.spare = old
		}
		l.mu.Unlock()
		wake()
	}
}
