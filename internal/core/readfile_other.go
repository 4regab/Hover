//go:build !windows

package core

import "os"

// Elsewhere a file can be renamed over while it is open, so these are the os functions.
// On Windows they let others rename the file meanwhile and retry a refused rename; see
// readfile_windows.go.

func Open(path string) (*os.File, error)   { return os.Open(path) }
func ReadFile(path string) ([]byte, error) { return os.ReadFile(path) }
func Rename(from, to string) error         { return os.Rename(from, to) }
