//go:build windows || shots

package main

import (
	"fmt"
	"image/color"
	"os"
	"path/filepath"
	"sync"
	"time"

	"gioui.org/layout"

	"github.com/4regab/Hover/go/internal/agents"
	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/notch"
	"github.com/4regab/Hover/go/internal/quota"
	"github.com/4regab/Hover/go/internal/shell"
)

// The open notch with the office in it: the real shell, the real office thread (wgpu-native
// on the machine's Vulkan or Direct3D) and a desktop that is not there. Names are the Rust
// shots' (notch-open-office, office-*), at 1 x on the notch's 1200 x 480 window.

type rig struct {
	s      *shell.Shell
	hv     *app.Hover
	win    *fakeWin
	dwin   *fakeWin
	pump   func()
	folder string
	hold   func(bool)
	done   func()
}

func newRig() (*rig, error) {
	data, err := os.MkdirTemp("", "hover-office-shots-")
	if err != nil {
		return nil, err
	}
	// The data folder is where the office keeps its view and time of day.
	os.Setenv("HOVER_DATA_DIR", data)
	settings := core.LoadSettings(filepath.Join(data, "settings.json"))
	settings.SetKiroNoticeSeen(true)
	var mu sync.Mutex
	held := true
	var runs int
	run := func(a agents.RunArgs) agents.KiroResult {
		mu.Lock()
		runs++
		sid := fmt.Sprintf("s%d", runs)
		mu.Unlock()
		a.Events(agents.KiroEvent{SessionID: &sid})
		for {
			mu.Lock()
			h := held
			mu.Unlock()
			if !h || a.Ct.IsCancelled() {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}
		return agents.NewResult(core.Completed, "## Imports tidied\n\nAll 14 files now sort their imports.")
	}
	hv := app.With(settings, nil, nil, run, func(id string) quota.Reading { return *reading(id) })
	r := &rig{hv: hv, win: &fakeWin{}, dwin: &fakeWin{w: 1200, h: 720}}
	var qmu sync.Mutex
	var queued []func()
	r.pump = func() {
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
		NewNotch:     func() (shell.Window, shell.NotchPlat, error) { return r.win, plain{}, nil },
		NewDashboard: func() (shell.Window, error) { return r.dwin, nil },
	}
	s, err := shell.New(hv, env, core.Look{Dark: true, Animations: false})
	if err != nil {
		return nil, err
	}
	r.s = s
	r.folder = filepath.Join(data, "project")
	if err := os.MkdirAll(r.folder, 0o755); err != nil {
		return nil, err
	}
	r.hold = func(on bool) { mu.Lock(); held = on; mu.Unlock() }
	r.done = func() { hv.Shutdown(); os.RemoveAll(data) }
	return r, nil
}

// open opens the notch and waits for the office's first picture.
func (r *rig) open() error {
	r.s.Expand(false, false)
	r.s.UpdateRest()
	// No GPU (the pictures made on a machine that cannot draw the room): the room is left
	// out and everything round it is drawn.
	if os.Getenv("HOVER_SHOTS_NOGPU") != "" {
		for i := 0; i < 20; i++ {
			r.pump()
			r.s.PushNow()
			time.Sleep(20 * time.Millisecond)
		}
		return nil
	}
	for i := 0; i < 600; i++ {
		r.pump()
		r.s.PushNow()
		if r.s.OfficeReady() {
			r.s.UpdateRest()
			r.pump()
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("the office drew nothing in 30 s")
}

// shot saves the notch's window (1200 x h logical) at 1x over the desktop colour.
func (r *rig) shot(dir, name string, h int) error {
	// What the view asks of the app while it draws (its size, say) is done between draws.
	for i := 0; i < 3; i++ {
		r.pump()
		r.s.UpdateRest()
		if _, err := render(1200, h, 1, color.NRGBA{A: 255}, r.win.draw); err != nil {
			return err
		}
	}
	r.pump()
	r.s.UpdateRest()
	img, err := render(1200, h, 1, color.NRGBA{R: 0x3a, G: 0x4a, B: 0x5e, A: 255}, r.win.draw)
	if err != nil {
		return err
	}
	return save(filepath.Join(dir, name), img)
}

// dshot saves the app window (1200 x 720) at 1x.
func (r *rig) dshot(dir, name string, w, h int) error {
	r.dwin.w, r.dwin.h = w, h
	for i := 0; i < 3; i++ {
		r.pump()
		r.s.UpdateRest()
		if _, err := render(w, h, 1, color.NRGBA{A: 255}, r.dwin.draw); err != nil {
			return err
		}
	}
	r.pump()
	img, err := render(w, h, 1, color.NRGBA{A: 255}, r.dwin.draw)
	if err != nil {
		return err
	}
	return save(filepath.Join(dir, name), img)
}

func (r *rig) settle(ms int) {
	for t := 0; t < ms; t += 20 {
		time.Sleep(20 * time.Millisecond)
		r.s.PushNow()
		r.pump()
	}
}

func officeShots(dir string) error {
	r, err := newRig()
	if err != nil {
		return err
	}
	defer r.done()
	r.win.w, r.win.h = 1200, 480
	for _, t := range []core.AgentTool{core.Kiro, core.Codex, core.Cursor, core.Claude} {
		r.hv.Sessions.Start(t, r.folder, "Tidy the imports in "+t.Name(), nil)
	}
	time.Sleep(400 * time.Millisecond)
	if err := r.open(); err != nil {
		return err
	}
	// The bots walk to their desks.
	for i := 0; i < 60; i++ {
		time.Sleep(100 * time.Millisecond)
		r.s.PushNow()
		r.pump()
	}
	if err := r.shot(dir, "notch-open-office.png", 480); err != nil {
		return err
	}
	// A session that has finished, opened in the drawer.
	r.hold(false)
	time.Sleep(600 * time.Millisecond)
	r.pump()
	all := r.hv.Sessions.All()
	if len(all) == 0 {
		return fmt.Errorf("no session to open")
	}
	r.s.OpenSession(all[0].ID)
	for i := 0; i < 8; i++ {
		time.Sleep(100 * time.Millisecond)
		r.pump()
	}
	if err := r.shot(dir, "office-drawer.png", 480); err != nil {
		return err
	}
	// Panels, as shots.rs takes them: the board, the overview, the history (with Kiro Web
	// sessions made elsewhere, and none), then long titles on the board.
	settle := func(ms int) {
		for t := 0; t < ms; t += 20 {
			time.Sleep(20 * time.Millisecond)
			r.s.PushNow()
			r.pump()
		}
	}
	r.s.CloseDrawer()
	for _, p := range [][2]string{{"board", "office-panel-board.png"}, {"tv", "office-panel-tv.png"}, {"history", "office-panel-history.png"}} {
		r.s.OpenPanel(p[0])
		settle(600)
		if err := r.shot(dir, p[1], 480); err != nil {
			return err
		}
	}
	ago := func(h float64) *core.Stamp { t := core.Now().AddSecs(-h * 3600); return &t }
	r.s.WebShot(nil, "Kiro listed 4 for Kiro Web and 4 for this computer. They are the same, so Hover can’t tell which are Kiro Web’s. It offers sessionSources: local/remote.")
	settle(200)
	if err := r.shot(dir, "office-panel-history-web-none.png", 480); err != nil {
		return err
	}
	r.s.WebShot([]agents.CloudSession{{ID: "w1", Title: "Fix the checkout total on mobile", Updated: ago(0.5)},
		{ID: "w2", Title: "Write the release notes for 3.7", Updated: ago(30)}, {ID: "w3"}}, "")
	settle(300)
	if err := r.shot(dir, "office-panel-history-web.png", 480); err != nil {
		return err
	}
	r.s.WebShot(nil, "")
	// Long titles wrap to two lines in the board's cards; the next card must start below.
	for _, x := range r.hv.Sessions.All() {
		r.hv.Sessions.Dismiss(x.ID)
	}
	r.hv.Sessions.Start(core.Kiro, r.folder, "is cloudflare good replacement for vercel since we cant use the free plan for a team project anymore", nil)
	time.Sleep(300 * time.Millisecond)
	r.hv.Sessions.Start(core.Kiro, r.folder, "Can you work on the Checker Project again on KiroWeb?", nil)
	time.Sleep(400 * time.Millisecond)
	r.s.OpenPanel("board")
	settle(600)
	if err := r.shot(dir, "office-panel-board-long.png", 480); err != nil {
		return err
	}
	r.s.OpenPanel("")
	if err := deskShots2(r, dir); err != nil {
		return err
	}
	return chatViewShots(r, dir)
}

var _ = layout.Context{}
var _ = notch.Rect{}
