package voice

import (
	"sync"

	"github.com/4regab/Hover/internal/core"
)

// Remote is a Source fed from outside: the microphone belongs to a host (the Windows app
// records with its own audio API and sends 16 kHz mono 16-bit blocks to the backend), and
// the voice flow reads it as it reads the device. Feed adds a block, Fail says the device
// failed. Safe for one feeder and the voice worker.
type Remote struct {
	mu      sync.Mutex
	max     int
	samples []int16
	level   float32
	failed  string
	done    bool
}

// NewRemote is an empty recording that keeps at most max samples (it stops growing there).
func NewRemote(max int) *Remote { return &Remote{max: max} }

// Feed adds samples. The level the notch shows is the RMS of this block. Samples past the
// limit, or after Finish, are dropped.
func (r *Remote) Feed(block []int16) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.done || len(block) == 0 {
		return
	}
	r.level = levelOf16(block)
	if room := r.max - len(r.samples); len(block) > room {
		block = block[:max(room, 0)]
	}
	r.samples = append(r.samples, block...)
}

// Fail says why the device failed (unplugged, taken away); the recording then stops.
func (r *Remote) Fail(why string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.failed == "" {
		r.failed = why
	}
}

func (r *Remote) Level() float32 { r.mu.Lock(); defer r.mu.Unlock(); return r.level }
func (r *Remote) Samples() int   { r.mu.Lock(); defer r.mu.Unlock(); return len(r.samples) }
func (r *Remote) Failed() string { r.mu.Lock(); defer r.mu.Unlock(); return r.failed }

// Finish stops at once and hands the samples over.
func (r *Remote) Finish() []int16 {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.done = true
	out := r.samples
	r.samples = nil
	return out
}

// Since is a copy of the samples from `from` on.
func (r *Remote) Since(from int) []int16 {
	r.mu.Lock()
	defer r.mu.Unlock()
	if from < 0 {
		from = 0
	}
	if from >= len(r.samples) {
		return nil
	}
	return append([]int16(nil), r.samples[from:]...)
}

// NewWith is New with the audio given: open is called for each recording and returns the
// Source to read (a Remote the host feeds), in place of the system's microphone.
func NewWith(settings *core.Settings, secrets *core.Secrets, local Local, hooks Hooks, open Open) *Voice {
	Sweep()
	return build(settings, secrets, local, hooks, func(k, m string) Speech { return NewGroq(k, m) }, open, 0, false, MaxSamples)
}
