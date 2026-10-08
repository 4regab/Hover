//go:build !windows

// Command notch-spike is phase 0 of the Go port's Windows notch; see main.go.
package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "notch-spike drives the Windows desktop; it runs on Windows only")
	os.Exit(2)
}
