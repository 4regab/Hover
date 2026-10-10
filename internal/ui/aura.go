package ui

import (
	"image"
	"math"
)

// app/src/aura.rs: voice's aura, a Siri-style orb on the listening and working cards, in
// place of their words. It is drawn on the CPU into a small image (66 DIPs, about 17,000
// pixels at 2x: a millisecond or two a frame).
//
// The orb is a dark glass ball with soft blobs of light swirling inside it, a bright rim
// and a faint glow round it. The blobs' colours are made from the one picked in Settings →
// Voice (it, the hues either side of it and one further round; a pale tint of it lights
// the glass). Listening, it swirls and the voice swells it; working on what was said, it
// swirls slowly and breathes.
//
// The voice is read against the room: the quietest level heard is taken as the room's noise
// and the loudest lately as full voice, so speech uses the whole range whatever the
// microphone's gain.

// AuraMode is what the card is doing.
type AuraMode int

const (
	// AuraListening: recording; the voice's level swells the orb.
	AuraListening AuraMode = iota
	// AuraWorking: starting local speech, transcribing, cleaning up, finding the project.
	AuraWorking
)

const auraBlobs = 4

type auraLook struct{ speed, breath, period, bright float32 }

func auraLookOf(m AuraMode) auraLook {
	if m == AuraListening {
		return auraLook{1.1, 0.025, 2.4, 1.0}
	}
	return auraLook{0.55, 0.05, 2.0, 0.85}
}

// Aura is the aura between frames: its eased parameters and its phase.
type Aura struct {
	lastT                                float32
	hasLast                              bool
	phase, speed, breath, period, bright float32
	level                                float32
	// floor and peak: the room's noise and the loudest lately, in the microphone's own 0..1,
	// once heard.
	floor, peak float32
	heard       bool
	init        bool
}

func (a *Aura) setup() {
	if !a.init {
		l := auraLookOf(AuraListening)
		a.speed, a.breath, a.period, a.bright = l.speed, l.breath, l.period, l.bright
		a.init = true
	}
}

// Reset forgets the last frame: the next one starts at its state's look, not eased into it.
func (a *Aura) Reset() { a.hasLast, a.heard = false, false }

func easeTo(v *float32, to, k float32) { *v += (to - *v) * k }

// turnHue is rgb (0..1) with its hue turned by deg degrees, keeping its brightness and
// saturation.
func turnHue(rgb [3]float32, deg float32) [3]float32 {
	mx := max(rgb[0], rgb[1], rgb[2])
	mn := min(rgb[0], rgb[1], rgb[2])
	c := mx - mn
	// Grey has no hue to turn: white stays white.
	if c < 1e-4 {
		return rgb
	}
	rem := func(a, b float64) float64 { return a - b*math.Floor(a/b) }
	var h float64
	switch mx {
	case rgb[0]:
		h = rem(float64((rgb[1]-rgb[2])/c), 6)
	case rgb[1]:
		h = float64((rgb[2]-rgb[0])/c) + 2
	default:
		h = float64((rgb[0]-rgb[1])/c) + 4
	}
	h = rem(h+float64(deg)/60, 6)
	x := c * float32(1-math.Abs(math.Mod(h, 2)-1))
	var r, g, b float32
	switch int(h) {
	case 0:
		r, g, b = c, x, 0
	case 1:
		r, g, b = x, c, 0
	case 2:
		r, g, b = 0, c, x
	case 3:
		r, g, b = 0, x, c
	case 4:
		r, g, b = x, 0, c
	default:
		r, g, b = c, 0, x
	}
	return [3]float32{r + mn, g + mn, b + mn}
}

// againstRoom is the microphone's level against the room: 0 at the room's noise, 1 at the
// loudest lately. The noise falls to a quieter level at once and follows a louder room over
// about twelve seconds; the loudest falls back over about three.
func (a *Aura) againstRoom(raw, dt float32) float32 {
	if !a.heard {
		// Started mid-sentence, the room is taken as no louder than a quiet one.
		a.floor, a.peak, a.heard = min(raw, 0.3), min(raw, 0.3)+0.3, true
	}
	dt = clamp(dt, 0, 0.5)
	if raw < a.floor {
		a.floor = raw
	} else {
		a.floor += (raw - a.floor) * (1 - float32(math.Exp(float64(-dt/12))))
	}
	if raw > a.peak {
		a.peak = raw
	} else {
		a.peak += (a.floor + 0.3 - a.peak) * (1 - float32(math.Exp(float64(-dt/3))))
	}
	// The gap is never under 0.2 (-12 dB), so a silent room doesn't blow its hiss up.
	v := clamp((raw-a.floor)/max(a.peak-a.floor, 0.2), 0, 1)
	// Quiet speech counts for more than loud: the curve lifts the low end.
	return float32(math.Pow(float64(v), 0.7))
}

// Frame is one frame at t seconds (the notch's clock), px pixels square, in color (Settings
// → Voice → Aura colour). level is the microphone's (0 to 1, listening only). snap: no
// easing (animations off, shots).
func (a *Aura) Frame(mode AuraMode, t, level float32, px int, snap bool, color [3]uint8) *image.RGBA {
	a.setup()
	dt := float32(-1)
	if a.hasLast {
		dt = t - a.lastT
	}
	a.lastT, a.hasLast = t, true
	to := auraLookOf(mode)
	lv := float32(0)
	if mode == AuraListening {
		lv = a.againstRoom(clamp(level, 0, 1), max(dt, 0))
	}
	if snap || !(dt >= 0 && dt <= 0.5) {
		a.speed, a.breath, a.period, a.bright, a.level = to.speed, to.breath, to.period, to.bright, lv
		a.phase = t * a.speed
	} else {
		k := 1 - float32(math.Exp(float64(-dt/0.15)))
		easeTo(&a.speed, to.speed, k)
		easeTo(&a.breath, to.breath, k)
		easeTo(&a.period, to.period, k)
		easeTo(&a.bright, to.bright, k)
		// The voice comes up quickly and falls away more slowly, as a meter does.
		tau := float32(0.25)
		if lv > a.level {
			tau = 0.04
		}
		easeTo(&a.level, lv, 1-float32(math.Exp(float64(-dt/tau))))
		// The voice swirls it faster too.
		a.phase += dt * a.speed * (1 + 1.6*a.level)
	}
	breath := float32(math.Sin(float64(t / a.period * 2 * math.Pi)))
	f := func(c uint8) float32 { return float32(c) / 255 }
	return a.draw(max(px, 8), breath, [3]float32{f(color[0]), f(color[1]), f(color[2])})
}

type auraBlob struct {
	cx, cy, ax, ay, long, short float32
	col                         [3]float32
	strength                    float32
}

// draw: breath is -1..1, where the orb is in its breathing.
func (a *Aura) draw(px int, breath float32, color [3]float32) *image.RGBA {
	ph, lv := a.phase, a.level
	sin := func(v float32) float32 { return float32(math.Sin(float64(v))) }
	cos := func(v float32) float32 { return float32(math.Cos(float64(v))) }
	exp := func(v float32) float32 { return float32(math.Exp(float64(v))) }
	// The orb's radius, in the picture's 0.5: the voice swells it, the breath moves it.
	radius := 0.34 * (1 + 0.13*lv) * (1 + a.breath*breath)
	// It brightens as it breathes in, and with the voice.
	bright := a.bright * (1 + 0.12*breath*a.breath/0.05) * (1 + 0.5*lv)
	// The family of colours: the picked one, its neighbours either side and one further
	// round for the blobs, and a pale tint for the glass.
	var pale [3]float32
	for i, c := range color {
		pale[i] = c*0.45 + 0.55
	}
	cols := [auraBlobs][3]float32{color, turnHue(color, 38), turnHue(color, -38), turnHue(color, 80)}
	// Each blob: centre, its long axis (along the way it moves), its two sizes, its colour
	// and strength. They circle the middle at their own rates, nearer and further in turn,
	// and spread out as the voice rises.
	var blobs [auraBlobs]auraBlob
	for i := range blobs {
		k := float32(i)
		way := float32(1)
		if i%2 != 0 {
			way = -0.8
		}
		ang := ph*way*(0.8+0.23*k) + k*1.7 + 0.3*k*k
		r := radius * (0.44 + 0.14*sin(ph*(0.6+0.17*k)+1.9*k)) * (1 + 0.35*lv)
		long := radius * (0.58 + 0.10*sin(ph*0.9+k))
		short := radius * (0.17 + 0.04*cos(ph*1.3+2*k))
		st := float32(0.9)
		if i == 3 {
			st = 0.6
		}
		blobs[i] = auraBlob{cos(ang) * r, sin(ang) * r, -sin(ang) * way, cos(ang) * way, long, short, cols[i], st}
	}
	// The middle turns a little against the rim, which twists the blobs into ribbons.
	swirl := 1.1*sin(ph*0.37) + 0.6*lv
	n := px
	img := image.NewRGBA(image.Rect(0, 0, n, n))
	// One pixel, in the picture's units: the rim's edge is smoothed over it.
	pix := 1 / float32(n)
	q8 := func(f float32) uint8 { return uint8(clamp(float32(math.Round(float64(f*255))), 0, 255)) }
	for y := 0; y < n; y++ {
		for x := 0; x < n; x++ {
			dx, dy := (float32(x)+0.5)/float32(n)-0.5, (float32(y)+0.5)/float32(n)-0.5
			rho := float32(math.Sqrt(float64(dx*dx + dy*dy)))
			q := rho / radius
			// Outside the orb only its glow shows, fading out before the picture's edge.
			edgeFade := clamp((0.5-rho)/0.1, 0, 1)
			over := max(q-1, 0) / 0.3
			halo := 0.22 * bright * exp(-over*over) * edgeFade * edgeFade
			inside := clamp((radius-rho)/pix+0.5, 0, 1)
			var v [3]float32
			if inside > 0 {
				om := max(1-q, 0)
				s := swirl * om * om
				sn, cs := sin(s), cos(s)
				rx, ry := dx*cs-dy*sn, dx*sn+dy*cs
				for _, b := range blobs {
					ox, oy := rx-b.cx, ry-b.cy
					u, w := (ox*b.ax+oy*b.ay)/b.long, (-ox*b.ay+oy*b.ax)/b.short
					g := b.strength * exp(-(u*u + w*w))
					for c := 0; c < 3; c++ {
						v[c] += g * b.col[c]
					}
				}
				// The glass: a rim in the pale tint that brightens toward the edge, and a soft
				// white highlight up and to the left.
				rim := 0.45 * float32(math.Pow(float64(q), 12))
				hx, hy := dx/radius+0.36, dy/radius+0.42
				shine := 0.16 * exp(-(hx*hx/0.07 + hy*hy/0.035))
				for c := 0; c < 3; c++ {
					v[c] = (v[c]*1.25 + 0.07*color[c] + rim*pale[c] + shine) * bright * inside
				}
			}
			// Light adds up to white where it is brightest, the colour round it.
			var out [3]float32
			for c := 0; c < 3; c++ {
				out[c] = 1 - exp(-(v[c] + halo*(1-inside)*color[c]*1.4))
			}
			// The ball itself is dark glass over whatever is behind (the notch is black).
			al := max(out[0], out[1], out[2], 0.92*inside)
			i := (y*n + x) * 4
			img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = q8(out[0]), q8(out[1]), q8(out[2]), q8(al)
		}
	}
	return img
}
