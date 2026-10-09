package shell

import "runtime"

// Version is Cargo.toml's [workspace.package] version, which the installers and
// `hover --version` read. The Go build keeps it here until the release job stamps it
// (-ldflags "-X github.com/4regab/Hover/go/internal/shell.Version=...").
var Version = "5.0.2"

var isWindows = runtime.GOOS == "windows"
