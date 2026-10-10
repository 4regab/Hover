package core

import (
	"errors"

	"golang.org/x/sys/windows"
)

var allowSetForegroundWindow = user32.NewProc("AllowSetForegroundWindow")

const asfwAny = ^uintptr(0) // ASFW_ANY, (DWORD)-1

func claim(name string, onShow func(*string)) (*Instance, error) {
	mutexName, showName := `Local\`+name+`RunningInstance`, `Local\`+name+`ShowApp`
	m16, _ := windows.UTF16PtrFromString(mutexName)
	s16, _ := windows.UTF16PtrFromString(showName)
	mutex, err := windows.CreateMutex(nil, true, m16)
	if errors.Is(err, windows.ERROR_ALREADY_EXISTS) {
		windows.CloseHandle(mutex)
		// This process was just launched by the user, so it may pass on the right to
		// take focus.
		if show, err := windows.OpenEvent(windows.EVENT_MODIFY_STATE, false, s16); err == nil {
			allowSetForegroundWindow.Call(asfwAny)
			windows.SetEvent(show)
			windows.CloseHandle(show)
		}
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	// Auto-reset, as EventResetMode.AutoReset. x/sys reports an event that already exists
	// (a late second launch still holding it) as an error with a good handle; Rust's
	// CreateEventW takes that handle, and so does this.
	event, err := windows.CreateEvent(nil, 0, 0, s16)
	if errors.Is(err, windows.ERROR_ALREADY_EXISTS) && event != 0 {
		err = nil
	}
	if err != nil {
		windows.CloseHandle(mutex)
		return nil, err
	}
	go func() {
		for {
			if ev, err := windows.WaitForSingleObject(event, windows.INFINITE); err != nil || ev != windows.WAIT_OBJECT_0 {
				return
			}
			onShow(nil)
		}
	}()
	return &Instance{release: func() {
		windows.ReleaseMutex(mutex)
		windows.CloseHandle(mutex)
	}}, nil
}
