//go:build !windows

package screen

import (
	"errors"
	"image"
)

// ponytail: Linux's capture is phase 6, on Wayland through the portal and PipeWire (no X11); the Mac app has its own panel (Screen.swift).
func supported() bool { return false }

var errNone = errors.New("The screen panel isn’t available here.")

func desktop() (*image.RGBA, error)     { return nil, errNone }
func whole() (*image.RGBA, error)       { return nil, errNone }
func capture(Apps) (*image.RGBA, error) { return nil, errNone }
