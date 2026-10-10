//go:build darwin

package core

// platform/macos.rs: ~/Library/Application Support (where the C# build kept its data there
// too), the login Keychain for note.key with a 0600 file when there is none, a LaunchAgent
// for launch at login. The look (dark or light) is the Swift app's, so there is none here.
// The logic is in macos.go; the Keychain calls are keychain_darwin.go.

import (
	"errors"
	"os"
	"path/filepath"
)

// AppData is ~/Library/Application Support: the data folder's base, and where Electron apps
// (Code, Cursor) keep their settings.
func AppData() string {
	h := Home()
	if h == "" {
		return ""
	}
	return filepath.Join(h, "Library", "Application Support")
}

func ConfigDir() string { return AppData() }

// SystemKeyGuard is the login Keychain.
type SystemKeyGuard struct{}

func (SystemKeyGuard) Wrap(key []byte) ([]byte, error) { return KeychainGuard{Login{}}.Wrap(key) }
func (SystemKeyGuard) Unwrap(stored []byte) ([]byte, *KeyError) {
	return KeychainGuard{Login{}}.Unwrap(stored)
}
func (SystemKeyGuard) Inherited() ([]byte, *KeyError) { return KeychainGuard{Login{}}.Inherited() }

// MARK: Launch at login

func launchAgents() string {
	h := Home()
	if h == "" {
		return ""
	}
	return filepath.Join(h, "Library", "LaunchAgents")
}

// SystemAutostart is ~/Library/LaunchAgents/dev.hover.desktop.plist.
type SystemAutostart struct{}

func (SystemAutostart) Enabled() bool {
	d := launchAgents()
	return d != "" && agentEnabled(d, LaunchAgentID)
}

func (SystemAutostart) Set(on bool) error {
	d := launchAgents()
	if d == "" {
		return errors.New("no $HOME")
	}
	if !on {
		return setAgent(d, LaunchAgentID, nil)
	}
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	return setAgent(d, LaunchAgentID, &exe)
}

// MARK: The desktop's look

// Look is what Theme.SystemDark and Animator.Still read on Windows and Linux. A Mac has the
// Swift app's own, so these say light with animations on and never change.
type Look struct{ Dark, Animations bool }

func SystemLook() Look { return Look{Animations: true} }

func WatchLook(changed func()) {}
