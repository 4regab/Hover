//go:build !windows

package audio

import "errors"

// ponytail: Linux's output is phase 6, and it talks to PipeWire (not PulseAudio, which is legacy).
func open(func([]float32)) (Output, error) {
	return nil, errors.New("no audio output on this system yet")
}
