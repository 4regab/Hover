//go:build !windows && !linux

package audio

import "errors"

// ponytail: the Mac app plays for itself; Linux is audio_linux.go.
func open(func([]float32)) (Output, error) {
	return nil, errors.New("no audio output on this system yet")
}
