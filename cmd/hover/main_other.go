//go:build !windows && !linux

// Command hover on the Mac is the Swift app, with cmd/hover-backend (phase 7 of the port).
package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "hover: on the Mac the app is the Swift one; this build is for Windows and Linux")
	os.Exit(2)
}
