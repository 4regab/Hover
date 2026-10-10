//go:build !windows && !linux

package screen

import (
	"errors"
	"image"
)

// ponytail: the Mac app has its own panel (Screen.swift); Linux is screen_linux.go.
func supported() bool { return false }

const unsupportedNote = "The screen panel isn’t available here."

var errNone = errors.New("The screen panel isn’t available here.")

func desktop() (*image.RGBA, error)     { return nil, errNone }
func whole() (*image.RGBA, error)       { return nil, errNone }
func capture(Apps) (*image.RGBA, error) { return nil, errNone }
