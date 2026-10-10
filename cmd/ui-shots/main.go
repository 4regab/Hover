//go:build windows || shots

// Command ui-shots renders the Go UI headless into PNGs, under the names hover --shots
// gives the Rust app's, so the two folders compare file by file (internal/shots).
//
//	ui-shots DIR
package main

import (
	"fmt"
	"os"

	"github.com/4regab/Hover/internal/shots"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: ui-shots DIR")
		os.Exit(2)
	}
	if err := shots.Run(os.Args[1]); err != nil {
		fmt.Fprintln(os.Stderr, "ui-shots:", err)
		os.Exit(1)
	}
}
