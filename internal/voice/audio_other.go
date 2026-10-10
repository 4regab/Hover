//go:build !windows && !linux

package voice

import "errors"

// ponytail: Linux is audio_linux.go (PipeWire). The Mac app records for itself.
func Microphones() []string { return nil }

func OpenMic(string, int) (Source, error) {
	return nil, errors.New("No microphone is available on this system yet.")
}
