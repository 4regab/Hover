//go:build windows

// Command hover is the product (hoverai.exe): the notch at the top centre, the office in
// it, the app window, Settings, the tray icon and the shortcut. app/src/main.rs.
//
//	hoverai                 run
//	hoverai --version       print the version
//
// Build it as a window, not a console: go build -ldflags "-H=windowsgui" -o hoverai.exe
package main

import (
	"fmt"
	"os"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/platform/win"
	"github.com/4regab/Hover/go/internal/shell"
	"github.com/4regab/Hover/go/internal/ui"
)

func main() {
	for _, a := range os.Args[1:] {
		// Before the single-instance check, so it answers while Hover runs. (A Windows GUI
		// exe has no console: print shows from a terminal that pipes it.)
		if a == "--version" {
			fmt.Println("Hover", shell.Version)
			return
		}
	}
	if err := win.Init(); err != nil {
		core.Logf("windows: %v", err)
		os.Exit(1)
	}
	ui.Warm()
	// One notch is the point; two copies of the app is not. A second launch asks the
	// running copy to open its window, then exits.
	var current atomic.Pointer[shell.Shell]
	inst, err := core.Claim(func(*string) {
		win.UIDo(func() {
			if s := current.Load(); s != nil {
				core.Logf("another launch: opening the app window")
				s.OpenDashboard(false)
			}
		})
	})
	if err != nil {
		core.Logf("single instance: %v", err)
		return
	}
	if inst == nil {
		return
	}
	defer inst.Release()

	hover := app.Start()
	look := core.SystemLook()
	env := shell.SystemEnv()
	s, err := shell.New(hover, env, look)
	if err != nil {
		core.Logf("the notch window: %v", err)
		os.Exit(1)
	}
	current.Store(s)
	s.Start()
	core.WatchLook(func() { win.UIDo(func() { s.LookChanged(core.SystemLook()) }) })
	core.Logf("started")
	// Kiro Web tasks that were still working in the cloud when Hover closed are followed on.
	hover.Sessions.ReattachCutOff()
	win.Run()
	core.Logf("quitting")
	s.VoiceQuit()
	// Stop the agents before anything is torn down; then the history and the settings.
	hover.Shutdown()
	env.TrayStop()
	core.Logf("quit: tools shut down, history and settings flushed")
}
