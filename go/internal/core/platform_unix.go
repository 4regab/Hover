//go:build !windows

package core

// platform/linux.rs: $XDG_DATA_HOME (~/.local/share), note.key as a 0600 file, and an XDG
// autostart entry for launch at login. macOS takes these too until phase 7 ports
// platform/macos.rs.
//
// ponytail: no Secret Service and no settings portal yet (both D-Bus; phase 6). A note.key
// that names a Secret Service item is "not now": left alone, with no history this run,
// never replaced. A new key is kept in the 0600 file, as Rust does without a keyring.

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// Home is Environment.SpecialFolder.UserProfile.
func Home() string { return os.Getenv("HOME") }

// LocalAppData and ProgramFiles are Windows-only known folders (the editors' install
// folders); none here.
func LocalAppData() string { return "" }
func ProgramFiles() string { return "" }

// xdg is an XDG base directory: the variable when it holds an absolute path (the spec
// ignores a relative one), else the fallback under $HOME.
func xdg(name, fallback string) string {
	if v := os.Getenv(name); filepath.IsAbs(v) {
		return v
	}
	if h := Home(); h != "" {
		return filepath.Join(h, fallback)
	}
	return ""
}

func AppData() string { return xdg("XDG_DATA_HOME", ".local/share") }

// ConfigDir is $XDG_CONFIG_HOME (~/.config): where Electron apps, KDE and GTK keep their
// settings.
func ConfigDir() string { return xdg("XDG_CONFIG_HOME", ".config") }

func FullPath(p string) string {
	cwd, err := os.Getwd()
	if err != nil {
		cwd = "/"
	}
	return LexicalFullPath(p, cwd)
}

// secretMarker is what note.key holds when the key itself is in the Secret Service: this
// marker and the item's id.
const secretMarker = "hover-key:secret-service:"

type SystemKeyGuard struct{}

func (SystemKeyGuard) Wrap(key []byte) ([]byte, error) { return append([]byte(nil), key...), nil }

func (SystemKeyGuard) Unwrap(stored []byte) ([]byte, *KeyError) {
	if strings.HasPrefix(string(stored), secretMarker) {
		return nil, KeyNotNow("the key is in the Secret Service, which this build can't ask yet")
	}
	if len(stored) == 32 {
		return append([]byte(nil), stored...), nil
	}
	return nil, KeyNever("note.key is not a key this build can read (a Windows DPAPI key only opens on Windows)")
}

func (SystemKeyGuard) Inherited() ([]byte, *KeyError) { return nil, nil }

// writePrivate writes for this user only: 0600, since the file may hold the key itself.
func writePrivate(file string, b []byte) error {
	f, err := os.OpenFile(file, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return err
	}
	if err := f.Chmod(0o600); err != nil {
		f.Close()
		return err
	}
	if _, err := f.Write(b); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}

// MARK: Launch at login

// SystemAutostart is $XDG_CONFIG_HOME/autostart/hover.desktop, which every XDG desktop
// starts at login.
type SystemAutostart struct{}

func autostartFile() string {
	c := ConfigDir()
	if c == "" {
		return ""
	}
	return filepath.Join(c, "autostart", "hover.desktop")
}

// startExe is the program to start: the AppImage itself when run from one (its mount
// point changes each run), else this executable.
func startExe() (string, error) {
	if a := os.Getenv("APPIMAGE"); a != "" {
		return a, nil
	}
	return os.Executable()
}

// quoteExec quotes a desktop entry's Exec argument as the spec asks.
func quoteExec(p string) string {
	var s strings.Builder
	s.WriteByte('"')
	for _, c := range p {
		if c == '"' || c == '`' || c == '$' || c == '\\' {
			s.WriteByte('\\')
		}
		s.WriteRune(c)
	}
	s.WriteByte('"')
	return s.String()
}

func (SystemAutostart) Enabled() bool {
	f := autostartFile()
	if f == "" {
		return false
	}
	b, err := os.ReadFile(f)
	if err != nil {
		return false
	}
	exec, hidden := false, false
	for _, l := range strings.Split(string(b), "\n") {
		if v, ok := strings.CutPrefix(l, "Exec="); ok && strings.TrimSpace(v) != "" {
			exec = true
		}
		if strings.TrimSpace(l) == "Hidden=true" {
			hidden = true
		}
	}
	return exec && !hidden
}

func (SystemAutostart) Set(on bool) error {
	f := autostartFile()
	if f == "" {
		return errors.New("no $HOME")
	}
	if !on {
		if err := os.Remove(f); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		return nil
	}
	exe, err := startExe()
	if err != nil {
		return errors.New("no executable path")
	}
	if err := os.MkdirAll(filepath.Dir(f), 0o755); err != nil {
		return err
	}
	entry := "[Desktop Entry]\nType=Application\nName=Hover\nComment=The agent office in the notch\nExec=" + quoteExec(exe) +
		"\nIcon=hover\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
	return os.WriteFile(f, []byte(entry), 0o644)
}

// MARK: The desktop's look

// Look is what Theme.SystemDark and Animator.Still read on Windows.
type Look struct{ Dark, Animations bool }

// SystemLook reads the desktop's own files: GNOME through gsettings, KDE's kdeglobals,
// GTK's settings.ini. With nothing to go by: light, as Windows' missing value means, and
// animations on. (The settings portal comes first in Rust; phase 6.)
func SystemLook() Look {
	l := Look{Dark: false, Animations: true}
	if d, ok := filesDark(); ok {
		l.Dark = d
	}
	if a, ok := filesAnimations(); ok {
		l.Animations = a
	}
	return l
}

// WatchLook calls changed whenever the look may have changed: a look at the files every
// five seconds (Rust listens to the portal's SettingChanged where there is one).
func WatchLook(changed func()) {
	go func() {
		last := SystemLook()
		for range time.Tick(5 * time.Second) {
			if now := SystemLook(); now != last {
				last = now
				changed()
			}
		}
	}()
}

// gsettings is `gsettings get <schema> <key>`, when GNOME's tools are there.
func gsettings(schema, key string) (string, bool) {
	out, err := exec.Command("gsettings", "get", schema, key).Output()
	if err != nil {
		return "", false
	}
	return strings.Trim(strings.TrimSpace(string(out)), "'"), true
}

func readConfig(rel string) (string, bool) {
	c := ConfigDir()
	if c == "" {
		return "", false
	}
	b, err := os.ReadFile(filepath.Join(c, rel))
	return string(b), err == nil
}

func filesDark() (bool, bool) {
	if s, ok := gsettings("org.gnome.desktop.interface", "color-scheme"); ok {
		switch s {
		case "prefer-dark":
			return true, true
		case "prefer-light":
			return false, true
		}
	}
	if d, ok := gtkThemeDark(os.Getenv("GTK_THEME")); ok {
		return d, true
	}
	if t, ok := readConfig("kdeglobals"); ok {
		if d, ok := KDEDark(t); ok {
			return d, true
		}
	}
	for _, f := range []string{"gtk-4.0/settings.ini", "gtk-3.0/settings.ini"} {
		if t, ok := readConfig(f); ok {
			if d, ok := GTKDark(t); ok {
				return d, true
			}
		}
	}
	return false, false
}

func filesAnimations() (bool, bool) {
	if s, ok := gsettings("org.gnome.desktop.interface", "enable-animations"); ok {
		return s == "true", true
	}
	if t, ok := readConfig("kdeglobals"); ok {
		return KDEAnimations(t)
	}
	return false, false
}

// ini is one key of one [group] of an INI-style file (kdeglobals, settings.ini).
func ini(text, group, key string) (string, bool) {
	inside := false
	for _, l := range strings.Split(text, "\n") {
		l = strings.TrimSpace(l)
		if strings.HasPrefix(l, "[") {
			inside = l == "["+group+"]"
			continue
		}
		if !inside {
			continue
		}
		if k, v, ok := strings.Cut(l, "="); ok && strings.TrimSpace(k) == key {
			return strings.TrimSpace(v), true
		}
	}
	return "", false
}

// gtkThemeDark: "Adwaita:dark", "Arc-Dark": a GTK theme named for its dark variant.
func gtkThemeDark(name string) (bool, bool) {
	n := strings.ToLower(name)
	if n == "" {
		return false, false
	}
	return strings.HasSuffix(n, ":dark") || strings.HasSuffix(n, "-dark"), true
}

// KDEDark is the colour scheme's window background, else its name.
func KDEDark(text string) (bool, bool) {
	if bg, ok := ini(text, "Colors:Window", "BackgroundNormal"); ok {
		var c []float64
		for _, x := range strings.Split(bg, ",") {
			if f, err := strconv.ParseFloat(strings.TrimSpace(x), 64); err == nil {
				c = append(c, f)
			}
		}
		if len(c) >= 3 {
			return (0.2126*c[0]+0.7152*c[1]+0.0722*c[2])/255 < 0.5, true
		}
	}
	if s, ok := ini(text, "General", "ColorScheme"); ok {
		return strings.Contains(strings.ToLower(s), "dark"), true
	}
	return false, false
}

// KDEAnimations: "Animation speed" all the way to instant writes a factor of 0.
func KDEAnimations(text string) (bool, bool) {
	if f, ok := ini(text, "KDE", "AnimationDurationFactor"); ok {
		if v, err := strconv.ParseFloat(f, 64); err == nil {
			return v > 0, true
		}
	}
	return false, false
}

// GTKDark is gtk-application-prefer-dark-theme, else the theme's name.
func GTKDark(text string) (bool, bool) {
	if v, ok := ini(text, "Settings", "gtk-application-prefer-dark-theme"); ok {
		switch strings.ToLower(v) {
		case "1", "true", "yes":
			return true, true
		}
		return false, true
	}
	if n, ok := ini(text, "Settings", "gtk-theme-name"); ok {
		return gtkThemeDark(n)
	}
	return false, false
}
