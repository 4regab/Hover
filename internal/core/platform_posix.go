//go:build !windows

package core

// What Linux and the Mac share of platform/linux.rs and platform/macos.rs: they are the same
// there. The rest is platform_unix.go (Linux) and platform_darwin.go (the Mac).

import "os"

// Home is Environment.SpecialFolder.UserProfile.
func Home() string { return os.Getenv("HOME") }

// LocalAppData and ProgramFiles are Windows-only known folders (the editors' install
// folders); none here.
func LocalAppData() string { return "" }
func ProgramFiles() string { return "" }

func FullPath(p string) string {
	cwd, err := os.Getwd()
	if err != nil {
		cwd = "/"
	}
	return LexicalFullPath(p, cwd)
}

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
