// Package audio is the system's audio output, for the office's music and voice's chime:
// cpal in the Rust app. A stream asks fill for its next frames and plays until closed;
// the device is let go on Close, so a quiet Hover holds no audio stream.
package audio

// Rate and Channels are what an output plays: the system converts from them.
const (
	Rate     = 44100
	Channels = 2
)

// Output is an open stream.
type Output interface{ Close() }

// Open starts a stream; fill is called on a goroutine of its own to fill each buffer with
// interleaved float32 frames (Channels to a frame), and must not block.
func Open(fill func(out []float32)) (Output, error) { return open(fill) }
