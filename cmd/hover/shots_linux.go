//go:build linux && shots

package main

import "github.com/4regab/Hover/internal/shots"

func init() { runShots = shots.Run }
