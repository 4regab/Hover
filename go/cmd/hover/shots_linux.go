//go:build linux && shots

package main

import "github.com/4regab/Hover/go/internal/shots"

func init() { runShots = shots.Run }
