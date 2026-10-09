//go:build !windows

package core

import "os"

// ReadFile is os.ReadFile. (On Windows it lets others rename the file meanwhile; see
// readfile_windows.go. Elsewhere a rename over an open file is always allowed.)
func ReadFile(path string) ([]byte, error) { return os.ReadFile(path) }
