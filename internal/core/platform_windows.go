package core

// platform/windows.rs: %APPDATA% (the roaming known folder, as Environment.SpecialFolder.
// ApplicationData finds it), DPAPI for note.key, HKCU\…\Run for launch at login.

import (
	"os"
	"path/filepath"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
	"golang.org/x/sys/windows/registry"
)

func known(id *windows.KNOWNFOLDERID) string {
	p, err := windows.KnownFolderPath(id, 0)
	if err != nil {
		return ""
	}
	return p
}

// AppData is Environment.SpecialFolder.ApplicationData; "" when Windows has none.
func AppData() string { return known(windows.FOLDERID_RoamingAppData) }

// Home, LocalAppData and ProgramFiles are UserProfile, LocalApplicationData and
// ProgramFiles, as Palette.Installed asks for them.
func Home() string         { return known(windows.FOLDERID_Profile) }
func LocalAppData() string { return known(windows.FOLDERID_LocalAppData) }
func ProgramFiles() string { return known(windows.FOLDERID_ProgramFiles) }

// ConfigDir: only Linux keeps other apps' settings in one config folder.
func ConfigDir() string { return "" }

// FullPath is Path.GetFullPath: GetFullPathNameW, which filepath.Abs calls.
func FullPath(p string) string {
	if a, err := filepath.Abs(p); err == nil {
		return a
	}
	return p
}

// Look is Theme.SystemDark and Animator.Still.
type Look struct{ Dark, Animations bool }

var (
	user32               = windows.NewLazySystemDLL("user32.dll")
	systemParametersInfo = user32.NewProc("SystemParametersInfoW")
)

const spiGetClientAreaAnimation = 0x1042

// SystemLook: Windows keeps "app mode" per user; a missing value means the light default.
// Motion follows "Animation effects" (SystemParameters.ClientAreaAnimation).
func SystemLook() Look {
	dark := false
	if k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`, registry.QUERY_VALUE); err == nil {
		if v, _, err := k.GetIntegerValue("AppsUseLightTheme"); err == nil {
			dark = v == 0
		}
		k.Close()
	}
	on := int32(1)
	r, _, _ := systemParametersInfo.Call(spiGetClientAreaAnimation, 0, uintptr(unsafe.Pointer(&on)), 0)
	return Look{Dark: dark, Animations: r == 0 || on != 0}
}

// WatchLook is UserPreferenceChanged's stand-in: the look read again every second, and
// changed called when it differs (a registry read, no hidden window).
func WatchLook(changed func()) {
	go func() {
		last := SystemLook()
		for range time.Tick(time.Second) {
			if now := SystemLook(); now != last {
				last = now
				changed()
			}
		}
	}()
}

// SystemKeyGuard is ProtectedData with DataProtectionScope.CurrentUser: CryptProtectData
// with no entropy and CRYPTPROTECT_UI_FORBIDDEN, as .NET calls it, so either build opens
// the other's note.key.
type SystemKeyGuard struct{}

func dpapi(data []byte, protect bool) ([]byte, error) {
	var in windows.DataBlob
	if len(data) > 0 {
		in = windows.DataBlob{Size: uint32(len(data)), Data: &data[0]}
	}
	var out windows.DataBlob
	var err error
	if protect {
		err = windows.CryptProtectData(&in, nil, nil, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	} else {
		err = windows.CryptUnprotectData(&in, nil, nil, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	}
	if err != nil {
		return nil, err
	}
	defer windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(out.Data))))
	return append([]byte(nil), unsafe.Slice(out.Data, out.Size)...), nil
}

func (SystemKeyGuard) Wrap(key []byte) ([]byte, error) { return dpapi(key, true) }

// Unwrap: DPAPI refusing is for good (another user's blob, a reset password or profile).
func (SystemKeyGuard) Unwrap(stored []byte) ([]byte, *KeyError) {
	k, err := dpapi(stored, false)
	if err != nil {
		return nil, KeyNever(err.Error())
	}
	return k, nil
}

func (SystemKeyGuard) Inherited() ([]byte, *KeyError) { return nil, nil }

const (
	runKey   = `Software\Microsoft\Windows\CurrentVersion\Run`
	runValue = "Hover"
)

// SystemAutostart is Settings.LaunchAtLogin: HKCU\…\Run.
type SystemAutostart struct{}

func (SystemAutostart) Enabled() bool {
	k, err := registry.OpenKey(registry.CURRENT_USER, runKey, registry.QUERY_VALUE)
	if err != nil {
		return false
	}
	defer k.Close()
	v, typ, err := k.GetStringValue(runValue)
	// RegGetValueW with RRF_RT_REG_SZ takes REG_SZ and expands REG_EXPAND_SZ.
	return err == nil && (typ == registry.SZ || typ == registry.EXPAND_SZ) && v != ""
}

func (SystemAutostart) Set(on bool) error {
	if !on {
		// throwOnMissingValue: false.
		if k, err := registry.OpenKey(registry.CURRENT_USER, runKey, registry.SET_VALUE); err == nil {
			k.DeleteValue(runValue)
			k.Close()
		}
		return nil
	}
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	k, _, err := registry.CreateKey(registry.CURRENT_USER, runKey, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()
	// Plain quotes, as format!("\"{}\"") writes them: %q would double every backslash.
	return k.SetStringValue(runValue, `"`+exe+`"`)
}

// writePrivate is File.WriteAllBytes on Windows, where DPAPI does the protecting.
func writePrivate(file string, b []byte) error { return os.WriteFile(file, b, 0o644) }
