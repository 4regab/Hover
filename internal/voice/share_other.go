//go:build !windows

package voice

import "os"

func openShared(p string) (*os.File, error) { return os.Open(p) }
