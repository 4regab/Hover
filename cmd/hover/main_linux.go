//go:build linux

// Command hover is the product on Linux: the notch at the top centre of the display, the
// office in it, the app window, Settings, the tray icon and the shortcut. Wayland only (no
// X11, no XWayland); app/src/main.rs.
//
//	hover                 run
//	hover --version       print the version
//	hover --toggle        open or fold the notch in the running copy (for a compositor's
//	                      own key binding, where it has no GlobalShortcuts portal)
//	hover --shots DIR     render every view headless into DIR, then exit (a build with -tags shots)
//
// Build it with Gio's EGL only (the Linux window is drawn off screen and handed over in
// shared memory): go build -tags nowayland,nox11,novulkan ./cmd/hover
package main

import (
	"fmt"
	"os"
	"os/signal"
	"sync/atomic"
	"syscall"

	"github.com/4regab/Hover/internal/app"
	"github.com/4regab/Hover/internal/core"
	"github.com/4regab/Hover/internal/shell"
	"github.com/4regab/Hover/internal/ui"
)

// toggleToken is what a second launch with --toggle passes the running copy in the place of
// an activation token.
const toggleToken = "hover:toggle"

// runShots is set by the build that has the pictures (-tags shots).
var runShots func(dir string) error

func main() {
	toggle := false
	for i, a := range os.Args[1:] {
		switch a {
		case "--version":
			fmt.Println("Hover", shell.Version)
			return
		case "--shots":
			if runShots == nil || i+2 >= len(os.Args) {
				fmt.Fprintln(os.Stderr, "hover: --shots needs a build with -tags shots, and a folder")
				os.Exit(2)
			}
			if err := runShots(os.Args[i+2]); err != nil {
				core.Logf("--shots: %v", err)
				os.Exit(1)
			}
			return
		case "--selftest":
			core.Logf("--selftest: the product has none; run notch-spike --selftest on Windows")
			return
		case "--toggle":
			toggle = true
		}
	}
	// Gio draws off screen through EGL: with no X to ask, the render node.
	if os.Getenv("EGL_PLATFORM") == "" {
		os.Setenv("EGL_PLATFORM", "surfaceless")
	}
	if toggle {
		// Handed to the running copy in place of the activation token.
		os.Unsetenv("XDG_ACTIVATION_TOKEN")
		os.Setenv("DESKTOP_STARTUP_ID", toggleToken)
	}
	if err := shell.InitLinux(); err != nil {
		core.Logf("wayland: %v", err)
		fmt.Fprintln(os.Stderr, "hover:", err)
		os.Exit(1)
	}
	ui.Warm()
	var current atomic.Pointer[shell.Shell]
	inst, err := core.Claim(func(token *string) {
		shell.UIDoLinux(func() {
			s := current.Load()
			if s == nil {
				return
			}
			if token != nil && *token == toggleToken {
				core.Logf("another launch: toggling the notch")
				s.Toggle()
				return
			}
			core.Logf("another launch: opening the app window")
			s.OpenDashboard(false)
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
		fmt.Fprintln(os.Stderr, "hover:", err)
		os.Exit(1)
	}
	current.Store(s)
	s.Start()
	core.WatchLook(func() { shell.UIDoLinux(func() { s.LookChanged(core.SystemLook()) }) })
	// A signal ends it as Quit does, so the tools are stopped and the history is saved.
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	go func() { <-sig; shell.UIDoLinux(env.Quit) }()
	core.Logf("started")
	// Kiro Web tasks that were still working in the cloud when Hover closed are followed on.
	hover.Sessions.ReattachCutOff()
	if err := shell.RunLinux(); err != nil {
		core.Logf("the window loop ended: %v", err)
	}
	core.Logf("quitting")
	s.VoiceQuit()
	// Stop the agents before anything is torn down; then the history and the settings.
	hover.Shutdown()
	env.TrayStop()
	core.Logf("quit: tools shut down, history and settings flushed")
}
