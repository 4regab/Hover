//go:build !windows

package audio

import "errors"

// ponytail: Linux's output (PulseAudio, which PipeWire serves) is phase 6.
func open(func([]float32)) (Output, error) {
	return nil, errors.New("no audio output on this system yet")
}
