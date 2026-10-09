//go:build !windows

package voice

import "errors"

// ponytail: Linux's microphone is phase 6, and it talks to PipeWire (not PulseAudio, which
// is legacy). The Mac app records for itself.
func Microphones() []string { return nil }

func OpenMic(string, int) (Source, error) {
	return nil, errors.New("No microphone is available on this system yet.")
}
