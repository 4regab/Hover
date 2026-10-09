//go:build !windows

// Command hover on Linux is phase 6 of the port (docs/development/go-port.md).
package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "hover: this build is Windows only so far; Linux is phase 6 of the Go port")
	os.Exit(2)
}
