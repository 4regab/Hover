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
	r := &rig{hv: hv, win: &fakeWin{}}
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
		NewNotch: func() (shell.Window, shell.NotchPlat, error) { return r.win, plain{}, nil },
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
	r.pump()
	r.s.UpdateRest()
	img, err := render(1200, h, 1, color.NRGBA{R: 0x3a, G: 0x4a, B: 0x5e, A: 255}, r.win.draw)
	if err != nil {
		return err
	}
	return save(filepath.Join(dir, name), img)
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
	r.hold(false)
	return nil
}

var _ = layout.Context{}
var _ = notch.Rect{}
