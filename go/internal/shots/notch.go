//go:build windows || shots

package shots

import (
	"fmt"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"sync"
	"time"

	"gioui.org/gpu/headless"
	"gioui.org/io/input"
	"gioui.org/layout"
	"gioui.org/op"
	"gioui.org/unit"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/quota"
	"github.com/4regab/Hover/go/internal/shell"
)

// The notch's and the app window's views, rendered from the real shell (its island, its
// card, its Settings) with a desktop that is not there: windows that only keep what draws
// into them, and a main display of 1920 x 1080 at 100 %.

type fakeWin struct {
	draw   func(layout.Context, float32) bool
	router input.Router
	w, h   int
}

func (f *fakeWin) SetDraw(d func(layout.Context, float32) bool) { f.draw = d }
func (f *fakeWin) SetHandlers(shell.Handlers)                   {}
func (f *fakeWin) Invalidate()                                  {}
func (f *fakeWin) Show()                                        {}
func (f *fakeWin) Hide()                                        {}
func (f *fakeWin) Close()                                       {}
func (f *fakeWin) Gone() bool                                   { return false }
func (f *fakeWin) Visible() bool                                { return true }
func (f *fakeWin) Minimized() bool                              { return false }
func (f *fakeWin) Maximized() bool                              { return false }
func (f *fakeWin) Focused() bool                                { return true }
func (f *fakeWin) Minimize()                                    {}
func (f *fakeWin) ToggleMaximize()                              {}
func (f *fakeWin) DragMove()                                    {}
func (f *fakeWin) ResizeFrom(int)                               {}
func (f *fakeWin) Caption(bool, uint32)                         {}
func (f *fakeWin) Execute(input.Command)                        {}
func (f *fakeWin) Frames() uint64                               { return 0 }
func (f *fakeWin) Size() (int, int) {
	if f.w == 0 {
		return 1200, 480
	}
	return f.w, f.h
}
func (f *fakeWin) Scale() float64 { return 1 }

// plain is shots.rs's Plain: no platform at all.
type plain struct{}

func (plain) Primary() (notch.Rect, float64) { return notch.Rect{Right: 1920, Bottom: 1080}, 1 }
func (plain) Signature() string              { return "" }
func (plain) Cursor() (int, int)             { return -100, -100 }
func (plain) Buttons() bool                  { return false }
func (plain) Place(notch.Rect)               {}
func (plain) Raise()                         {}
func (plain) SetAcceptsKeys(bool)            {}
func (plain) RememberForeground()            {}
func (plain) RestoreForeground()             {}
func (plain) Focus()                         {}
func (plain) SetHit(bool)                    {}
func (plain) ForegroundIsOurs() bool         { return false }

type stopped struct{}

func (stopped) Stop()         {}
func (stopped) Running() bool { return false }

// render draws a window's callback into w x h physical pixels twice (the second sees the
// first's measurements) and reads it back, over a backdrop.
func render(w, h int, scale float32, backdrop color.NRGBA, draw func(layout.Context, float32) bool) (*image.RGBA, error) {
	win, err := headless.NewWindow(w, h)
	if err != nil {
		return nil, err
	}
	defer win.Release()
	var router input.Router
	var ops op.Ops
	for i := 0; i < 2; i++ {
		ops.Reset()
		gtx := layout.Context{Ops: &ops, Now: time.Now(), Metric: unit.Metric{PxPerDp: 1, PxPerSp: 1},
			Constraints: layout.Exact(image.Pt(w, h)), Source: router.Source()}
		draw(gtx, scale)
		router.Frame(&ops)
	}
	if err := win.Frame(&ops); err != nil {
		return nil, err
	}
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	if err := win.Screenshot(img); err != nil {
		return nil, err
	}
	// The notch is see-through: its shots lay it over a desktop colour, as shots.rs does.
	out := image.NewRGBA(img.Bounds())
	for i := 0; i < len(img.Pix); i += 4 {
		a := uint32(img.Pix[i+3])
		for c := 0; c < 3; c++ {
			b := uint32([3]uint8{backdrop.R, backdrop.G, backdrop.B}[c])
			out.Pix[i+c] = uint8(min(uint32(img.Pix[i+c])+b*(255-a)/255, 255))
		}
		out.Pix[i+3] = 255
	}
	return out, nil
}

func notchShots(dir string) error {
	data, err := os.MkdirTemp("", "hover-notch-shots-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(data)
	settings := core.LoadSettings(filepath.Join(data, "settings.json"))
	for _, id := range []string{"claude", "kiro", "codex", "cursor"} {
		settings.SetNotchItem(id, true)
	}
	// Sessions that work until told otherwise.
	var mu sync.Mutex
	hold := true
	var runs int
	run := func(a agents.RunArgs) agents.KiroResult {
		mu.Lock()
		runs++
		sid := fmt.Sprintf("s%d", runs)
		mu.Unlock()
		a.Events(agents.KiroEvent{SessionID: &sid})
		for {
			mu.Lock()
			h := hold
			mu.Unlock()
			if !h || a.Ct.IsCancelled() {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}
		return agents.NewResult(core.Completed, "## Imports tidied\n\nAll 14 files now sort their imports.")
	}
	hv := app.With(settings, nil, nil, run, func(id string) quota.Reading { return *reading(id) })
	var win fakeWin
	// What threads ask the UI thread to do waits here until the shots run it: Gio's text
	// shaper belongs to one goroutine.
	var qmu sync.Mutex
	var queued []func()
	pump := func() {
		qmu.Lock()
		q := queued
		queued = nil
		qmu.Unlock()
		for _, f := range q {
			f()
		}
	}
	env := shell.Env{
		UIDo:  func(f func()) { qmu.Lock(); queued = append(queued, f); qmu.Unlock() },
		After: func(time.Duration, func()) shell.Timer { return stopped{} },
		Every: func(time.Duration, func()) shell.Timer { return stopped{} },
		Quit:  func() {}, Headless: true,
		NewNotch: func() (shell.Window, shell.NotchPlat, error) { return &win, plain{}, nil },
	}
	s, err := shell.New(hv, env, core.Look{Dark: true, Animations: false})
	if err != nil {
		return err
	}
	hv.RefreshQuotas(true)
	time.Sleep(300 * time.Millisecond)
	pump()
	folder := filepath.Join(data, "project")
	if err := os.MkdirAll(folder, 0o755); err != nil {
		return err
	}
	desk := color.NRGBA{R: 0x3a, G: 0x4a, B: 0x5e, A: 255}
	// shot saves the top h logical pixels of the notch's 1200 wide window at 2x.
	shot := func(name string, h int) error {
		pump()
		img, err := render(1200*2, h*2, 2, desk, win.draw)
		if err != nil {
			return err
		}
		return save(filepath.Join(dir, name), img)
	}
	hv.Sessions.Start(core.Kiro, folder, "Tidy the imports", nil)
	hv.Sessions.Start(core.Codex, folder, "Look for dead code", nil)
	time.Sleep(300 * time.Millisecond)
	s.UpdateRest()
	s.SetClock(1.3)
	if err := shot("notch-rest-pill-2x.png", 60); err != nil {
		return err
	}
	// Ends nobody saw: the tool's logo with its badge, and the task.
	mu.Lock()
	hold = false
	mu.Unlock()
	time.Sleep(400 * time.Millisecond)
	s.UpdateRest()
	time.Sleep(500 * time.Millisecond)
	if err := shot("notch-rest-done-2x.png", 60); err != nil {
		return err
	}
	// Two agents at work and a question: the amber island, then its card.
	mu.Lock()
	hold = true
	mu.Unlock()
	hv.Sessions.Start(core.Kiro, folder, "Tidy the imports again", nil)
	hv.Sessions.Start(core.Cursor, folder, "Look for dead code again", nil)
	time.Sleep(300 * time.Millisecond)
	hv.Seen()
	s.UpdateRest()
	time.Sleep(500 * time.Millisecond)
	s.SetClock(0.4)
	if err := shot("notch-rest-working-2x.png", 60); err != nil {
		return err
	}
	var asker agents.KiroSession
	for _, x := range hv.Sessions.All() {
		if x.Busy() {
			asker = x
			break
		}
	}
	cmd := "npm install three@0.171.0"
	ask := agents.AgentAsk{ID: "n1", Kind: "execute", Title: "Run", Command: &cmd, Reason: "Installs packages or uses the network"}
	id := ""
	if asker.KiroID != nil {
		id = *asker.KiroID
	}
	hv.Sessions.Ask(asker.Tool, id, ask, agents.NewCancel(), func(agents.AskAnswer) {})
	s.UpdateRest()
	time.Sleep(700 * time.Millisecond)
	s.SetClock(0.4)
	if err := shot("notch-rest-ask-2x.png", 60); err != nil {
		return err
	}
	s.OpenCard()
	s.UpdateRest()
	if err := shot("notch-rest-card-2x.png", 220); err != nil {
		return err
	}
	s.AnswerAsked(agents.Deny)
	mu.Lock()
	hold = false
	mu.Unlock()
	time.Sleep(300 * time.Millisecond)
	hv.Shutdown()
	return nil
}
