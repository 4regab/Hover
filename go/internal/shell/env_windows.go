//go:build windows

package shell

import (
	"fmt"
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

		OpenURL:        win.OpenURL,
		PickFolder:     func() (string, bool) { return win.PickFolder(owner()) },
		PickThemeFile:  func() (string, bool) { return win.PickThemeFile(owner()) },
		PickImage:      func() (string, bool) { return win.PickImage(owner()) },
		ClipboardImage: win.ClipboardImage,
		SetClipboard:   func(t string) { win.SetClipboardText(t) },

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
		VoiceHotkey: func(sc *core.Shortcut) (func() bool, error) {
			// register_hold in win.rs: the chord is id 2; letting go of the keys is polled.
			if notchWin == nil {
				return nil, nil
			}
			notchWin.UnregisterHotkey(2)
			if sc == nil || !sc.IsSet() {
				return nil, nil
			}
			vk, ok := app.VK(sc.Key)
			if !ok {
				return nil, fmt.Errorf("%s has no key Windows can register.", sc.Label())
			}
			var mods uint32
			var keys [][]uint16
			if sc.Modifiers.Has(core.ModControl) {
				mods |= win.ModControl
				keys = append(keys, []uint16{0x11})
			}
			if sc.Modifiers.Has(core.ModAlt) {
				mods |= win.ModAlt
				keys = append(keys, []uint16{0x12})
			}
			if sc.Modifiers.Has(core.ModShift) {
				mods |= win.ModShift
				keys = append(keys, []uint16{0x10})
			}
			if sc.Modifiers.Has(core.ModWindows) {
				mods |= win.ModWin
				keys = append(keys, []uint16{0x5B, 0x5C})
			}
			if err := notchWin.RegisterHotkey(2, vk, mods); err != nil {
				var code uint32
				if he, ok := err.(win.HotkeyError); ok {
					code = he.Code & 0xFFFF
				}
				core.Logf("voice hotkey %s could not be registered (Win32 error %d)", sc.Label(), code)
				if code == 1409 {
					return nil, fmt.Errorf("%s is already in use by another app, by Windows or by Hover’s own shortcut.", sc.Label())
				}
				return nil, fmt.Errorf("%s couldn’t be registered (Windows error %d).", sc.Label(), code)
			}
			return func() bool {
				if !win.KeyDown(vk) {
					return false
				}
				for _, ks := range keys {
					any := false
					for _, k := range ks {
						any = any || win.KeyDown(k)
					}
					if !any {
						return false
					}
				}
				return true
			}, nil
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
