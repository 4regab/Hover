//go:build windows

package shell

import (
	"time"

	"github.com/4regab/Hover/go/internal/app"
	"github.com/4regab/Hover/go/internal/core"
	"github.com/4regab/Hover/go/internal/platform/win"
)

// window is a *win.Window as the shell's Window: the handlers' type is the shell's.
type window struct{ *win.Window }

func (w window) SetHandlers(h Handlers) {
	w.Window.SetHandlers(win.Handlers{OnClose: h.OnClose, OnFocus: h.OnFocus, OnState: h.OnState, OnPress: h.OnPress})
}

func wrap(w *win.Window, err error) (Window, error) {
	if err != nil {
		return nil, err
	}
	return window{w}, nil
}

// SystemEnv is Env on Win32 (platform_start in main.rs): the notch's own window with its
// shortcut and tray, the app window, the system's file dialogs.
func SystemEnv() Env {
	var notchWin *win.Window
	owner := func() uintptr {
		if notchWin != nil {
			return notchWin.HWND
		}
		return 0
	}
	return Env{
		UIDo:  win.UIDo,
		After: func(d time.Duration, f func()) Timer { return win.After(d, f) },
		Every: func(d time.Duration, f func()) Timer { return win.Every(d, f) },
		Quit:  win.Quit,

		OpenURL:       win.OpenURL,
		PickFolder:    func() (string, bool) { return win.PickFolder(owner()) },
		PickThemeFile: func() (string, bool) { return win.PickThemeFile(owner()) },
		PickImage:     func() (string, bool) { return win.PickImage(owner()) },

		NewNotch: func() (Window, NotchPlat, error) {
			w, err := win.NewWindow(win.Options{Kind: win.KindNotch, Title: "Hover notch", W: 120, H: 40})
			if err != nil {
				return nil, nil, err
			}
			notchWin = w
			return window{w}, win.NewNotchPlat(w), nil
		},
		NewDashboard: func() (Window, error) {
			return wrap(win.NewWindow(win.Options{Kind: win.KindFrame, Title: "Hover", W: 1200, H: 620, MinW: 880, MinH: 480, Icon: true}))
		},
		NewWarning: func(title string, w, h float32) (Window, error) {
			return wrap(win.NewWindow(win.Options{Kind: win.KindDialog, Title: title, W: w, H: h, Icon: true}))
		},

		Bind: func(h SysHooks) {
			notchWin.Hook(win.Hooks{
				Hotkey:      h.Hotkey,
				Deactivated: h.Deactivated,
				TrayLeft:    h.TrayLeft,
				TrayMenu:    h.TrayMenu,
			})
		},
		TrayStart: func() { notchWin.TrayStart() },
		TrayStop:  func() { notchWin.TrayStop() },
		Hotkey: func(sc core.Shortcut) bool {
			// HotKeys.Register: MOD_NOREPEAT and the chord's modifiers; false when Windows
			// refuses (reserved, or another app has it), logged with the error as the C# does.
			if notchWin == nil {
				return false
			}
			notchWin.UnregisterHotkey(1)
			if !sc.IsSet() {
				return true
			}
			vk, ok := app.VK(sc.Key)
			if !ok {
				core.Logf("hotkey %s has no Windows virtual-key mapping", sc.Label())
				return false
			}
			var mods uint32
			if sc.Modifiers.Has(core.ModControl) {
				mods |= win.ModControl
			}
			if sc.Modifiers.Has(core.ModAlt) {
				mods |= win.ModAlt
			}
			if sc.Modifiers.Has(core.ModShift) {
				mods |= win.ModShift
			}
			if sc.Modifiers.Has(core.ModWindows) {
				mods |= win.ModWin
			}
			if err := notchWin.RegisterHotkey(1, vk, mods); err != nil {
				core.Logf("hotkey %s could not be registered (%v)", sc.Label(), err)
				return false
			}
			return true
		},
		SetTrayMenu: func(m app.Menu) {
			items := make([]win.MenuItem, len(m))
			for i, it := range m {
				if it == nil {
					items[i] = win.MenuItem{Sep: true}
				} else {
					items[i] = win.MenuItem{Label: it.Label, Check: it.Check}
				}
			}
			win.SetTrayMenu(items)
		},
		Notify: func(title, body string) {
			if notchWin != nil {
				notchWin.TrayNotify(title, body)
			}
		},
	}
}
