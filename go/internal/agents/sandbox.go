package agents

import (
	"os"
	"path/filepath"
	"runtime"
)

// TempRoot: temp files and Unix sockets go in a short folder of the tool's own (sockets
// have a 104-byte path limit), the only place sockets work besides CuaDriver's and
// Hover's browser's.
func TempRoot() string {
	if r := os.Getenv("HOVER_SANDBOX_TMP"); r != "" {
		return r
	}
	if runtime.GOOS == "darwin" {
		return "/private/tmp/claude"
	}
	return filepath.Join(os.TempDir(), "claude")
}
