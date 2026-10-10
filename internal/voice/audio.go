package voice

import "math"

// The microphone, while the shortcut is held: the chosen input (or the system's default),
// brought to 16 kHz mono 16-bit as it comes, into one buffer that stops growing at ten
// minutes. The level the notch shows is the real RMS of what just came in. Finishing lets
// the device go at once, so it is let go the moment the key is released, the cap is reached
// or the voice is cancelled.

const (
	// Rate is the samples a second the engines take.
	Rate = 16_000
	// MaxSamples is ten minutes at 16 kHz: 9.6 M samples, 19.2 MB.
	MaxSamples = Rate * 600
)

// Source is a recording in progress, as the voice worker sees it (the microphone, or a
// test's fake).
type Source interface {
	// Level is 0..1 from the latest audio.
	Level() float32
	// Samples is how many 16 kHz samples are in so far.
	Samples() int
	// Failed says why the device failed (unplugged, taken away): the recording stops. "" is fine.
	Failed() string
	// Finish stops at once and hands the samples over.
	Finish() []int16
	// Since is a copy of the samples from `from` on, while it records (voice listens for
	// "take a screenshot" in it).
	Since(from int) []int16
}

// Open opens a source: the device's name ("" is the default) and the most samples to keep.
type Open func(device string, max int) (Source, error)

// level is the RMS of a block as 0..1 on a decibel scale: −60 dBFS and below is 0, full
// scale 1.
func level(x []float32) float32 {
	if len(x) == 0 {
		return 0
	}
	var sum float64
	for _, s := range x {
		sum += float64(s) * float64(s)
	}
	rms := math.Sqrt(sum / float64(len(x)))
	if rms <= 1e-6 {
		return 0
	}
	return float32(math.Max(0, math.Min(1, (20*math.Log10(rms)+60)/60)))
}

// levelOf16 is level for 16-bit samples.
func levelOf16(x []int16) float32 {
	f := make([]float32, len(x))
	for i, s := range x {
		f[i] = float32(s) / 32768
	}
	return level(f)
}

func floatBits(f float32) uint32     { return math.Float32bits(f) }
func floatFromBits(b uint32) float32 { return math.Float32frombits(b) }
