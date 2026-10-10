package core

// single.rs (App.OnStartup): one copy of Hover at a time. A second launch asks the
// running copy to open its window, then exits. Windows uses the C# app's own names
// (Local\HoverRunningInstance, Local\HoverShowApp), so the builds also keep out of each
// other's way. Linux holds a lock on hover.lock in $XDG_RUNTIME_DIR and listens on
// hover.sock beside it; macOS does the same in $TMPDIR. The lock goes with the process,
// however it ends.

// Instance is the claim of the only copy; keep it for as long as the app runs.
type Instance struct{ release func() }

// Release gives the claim up (the process ending does too).
func (i *Instance) Release() {
	if i != nil && i.release != nil {
		i.release()
		i.release = nil
	}
}

// Claim claims the instance under the app's name. It returns nil, nil when another copy
// runs (and has been asked to show its window). onShow runs, on a goroutine of its own,
// whenever a later launch asks; its token is the launch's activation token on Linux
// (XDG_ACTIVATION_TOKEN or DESKTOP_STARTUP_ID), for the window to take focus.
func Claim(onShow func(token *string)) (*Instance, error) { return ClaimNamed("Hover", onShow) }

func ClaimNamed(name string, onShow func(token *string)) (*Instance, error) {
	return claim(name, onShow)
}

// socketFits: room in a Unix socket's path (sun_path is 104 bytes on macOS, 108 on Linux,
// with the NUL): a folder whose path leaves too little for the file is passed over.
func socketFits(dir, file string) bool { return len(dir)+1+len(file) < 100 }
