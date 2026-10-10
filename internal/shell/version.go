package shell

import "runtime"

// Version is the number in VERSION (the repo root), which the installers and `hover --version`
// show. The builds stamp it (-ldflags "-X github.com/4regab/Hover/internal/shell.Version=...");
// this value is for `go run`, and CI checks that it equals VERSION.
var Version = "5.0.4"

var isWindows = runtime.GOOS == "windows"
