// Package music is music.rs: the office's chill beats (web/office/main.js `beats`), a CC0
// lofi loop, off until switched on, remembered, faded in to 0.32 and out, and silent while
// no office is in view. It is decoded as it plays (Ogg Vorbis) into the system's output,
// and the device is let go while silent, so a quiet Hover holds no audio stream.
package music

import (
	"bytes"
	_ "embed"
	"io"
	"math"
	"sync"
	"sync/atomic"

	"github.com/jfreymuth/oggvorbis"

	"github.com/4regab/Hover/go/internal/audio"
	"github.com/4regab/Hover/go/internal/core"
)

//go:embed office-beats.ogg
var loop []byte

const Full = 0.32

// RampStep is the page's ramp, one step per animation frame: up by 0.02, down by 0.03,
// within 0 and 0.32, snapped to the target once within 0.02. It returns the new volume and
// whether the ramp has arrived. In doubles, as audio.volume is.
func RampStep(volume, to float64) (float64, bool) {
	d := -0.03
	if to > volume {
		d = 0.02
	}
	v := math.Max(0, math.Min(Full, volume+d))
	if math.Abs(v-to) > 0.02 {
		return v, false
	}
	return to, true
}

// Want is the button (remembered) and whether an office is in view: what should sound.
type Want struct{ Want, Seen bool }

func (w Want) Target() float64 {
	if w.Want && w.Seen {
		return Full
	}
	return 0
}

// Beats is the music.
type Beats struct {
	mu     sync.Mutex
	st     Want
	ramp   float64
	out    audio.Output
	volume atomic.Uint32 // the audio goroutine reads the ramp's volume as a float32
}

func New(want bool) *Beats { return &Beats{st: Want{Want: want, Seen: true}} }

func (b *Beats) Want() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.st.Want
}

// Volume is the ramp's own volume.
func (b *Beats) Volume() float64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.ramp
}

// Toggle is the button: on or off, remembered by the caller. False when no output could be
// opened (the page's play() rejecting): the button goes back off.
func (b *Beats) Toggle(want bool) bool {
	b.mu.Lock()
	b.st.Want = want
	b.mu.Unlock()
	return b.sync()
}

// Follow: an office came into view or went (the page's 'visible' message).
func (b *Beats) Follow(seen bool) {
	b.mu.Lock()
	b.st.Seen = seen
	b.mu.Unlock()
	b.sync()
}

func (b *Beats) sync() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.st.Target() > 0 && b.out == nil {
		dec, err := newDecoder()
		if err == nil {
			var out audio.Output
			out, err = audio.Open(b.player(dec))
			b.out = out
		}
		if err != nil {
			core.Logf("beats: no audio output — %v", err)
			b.st.Want = false
			return false
		}
	}
	return true
}

// player is the stream's fill: the loop at the ramp's volume, resampled to the output's
// rate by the nearest sample, which is enough for a quiet loop in the background.
func (b *Beats) player(dec *decoder) func([]float32) {
	pos := 0.0
	step := float64(dec.rate) / audio.Rate
	return func(out []float32) {
		vol := math.Float32frombits(b.volume.Load())
		for i := 0; i+1 < len(out); i += audio.Channels {
			l, r := dec.at(int(pos))
			pos += step
			if int(pos) >= len(dec.buf) {
				pos -= float64(dec.consume(int(pos)))
			}
			out[i], out[i+1] = l*vol, r*vol
		}
	}
}

// Frame is one animation frame of the fade (about 60 a second, as requestAnimationFrame).
// True while it still moves, so the caller keeps its timer only as long as that.
func (b *Beats) Frame() bool {
	b.mu.Lock()
	to := b.st.Target()
	now := b.ramp
	if now == to {
		b.mu.Unlock()
		return false
	}
	v, done := RampStep(now, to)
	b.ramp = v
	b.volume.Store(math.Float32bits(float32(v)))
	var out audio.Output
	if done && to == 0 {
		// audio.pause(): the stream and the device go.
		out, b.out = b.out, nil
	}
	b.mu.Unlock()
	if out != nil {
		out.Close()
	}
	return !done
}

// Chime is a short two-note chime on the default output (voice's screenshot), on its own
// goroutine; the device is let go when it ends. Nothing when there is no output.
func Chime() {
	go func() {
		n := 0
		var out audio.Output
		out, err := audio.Open(func(buf []float32) {
			for i := 0; i+1 < len(buf); i += audio.Channels {
				t := float64(n) / audio.Rate
				n++
				// 880 Hz then 1320 Hz, 90 ms each, each fading out; quiet, under the music.
				f, k := 880.0, t/0.09
				if t >= 0.09 {
					f, k = 1320, (t-0.09)/0.09
				}
				v := float32(0)
				if t < 0.18 {
					v = float32(math.Sin(t*f*2*math.Pi) * 0.12 * (1 - k))
				}
				buf[i], buf[i+1] = v, v
			}
		})
		if err != nil {
			core.Logf("chime: %v", err)
			return
		}
		<-timeAfter(260)
		out.Close()
	}()
}

// decoder is the loop, decoded a block at a time and started again at its end.
type decoder struct {
	r    *oggvorbis.Reader
	buf  [][2]float32
	rate int
}

func newDecoder() (*decoder, error) {
	r, err := oggvorbis.NewReader(bytes.NewReader(loop))
	if err != nil {
		return nil, err
	}
	return &decoder{r: r, rate: r.SampleRate()}, nil
}

func (d *decoder) fill(upto int) {
	empty := 0
	block := make([]float32, 4096)
	for len(d.buf) <= upto {
		n, err := d.r.Read(block)
		if n > 0 {
			empty = 0
			ch := d.r.Channels()
			for i := 0; i+ch <= n; i += ch {
				l := block[i]
				r := l
				if ch > 1 {
					r = block[i+1]
				}
				d.buf = append(d.buf, [2]float32{l, r})
			}
		}
		if err != nil && (err == io.EOF || n == 0) || n == 0 {
			// The end (or a broken page): from the top again, with a new reader. Twice with
			// nothing in between means nothing will come: silence, rather than a spin.
			empty++
			nr, nerr := oggvorbis.NewReader(bytes.NewReader(loop))
			if nerr != nil || empty >= 2 {
				for len(d.buf) <= upto {
					d.buf = append(d.buf, [2]float32{})
				}
				return
			}
			d.r = nr
		}
	}
}

func (d *decoder) at(i int) (float32, float32) { d.fill(i); return d.buf[i][0], d.buf[i][1] }

// consume drops what has been played; how many frames went.
func (d *decoder) consume(played int) int {
	n := min(played, len(d.buf))
	d.buf = d.buf[n:]
	return n
}
