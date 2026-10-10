package ui

import (
	"image/color"
	"time"
)

// Easing is one of Slint's easing curves: a CSS cubic Bézier (x1, y1, x2, y2), or nil
// for linear.
type Easing *[4]float32

var (
	Linear    Easing
	Ease      Easing = &[4]float32{0.25, 0.1, 0.25, 1}
	EaseIn    Easing = &[4]float32{0.42, 0, 1, 1}
	EaseOut   Easing = &[4]float32{0, 0, 0.58, 1}
	EaseInOut Easing = &[4]float32{0.42, 0, 0.58, 1}
)

// ease is the curve's y at x = t (bisection on x, as precise as a frame needs).
func ease(e Easing, t float32) float32 {
	if e == nil || t <= 0 || t >= 1 {
		return clamp(t, 0, 1)
	}
	b := func(a, c, s float32) float32 { u := 1 - s; return 3*u*u*s*a + 3*u*s*s*c + s*s*s }
	lo, hi := float32(0), float32(1)
	for i := 0; i < 24; i++ {
		m := (lo + hi) / 2
		if b(e[0], e[2], m) < t {
			lo = m
		} else {
			hi = m
		}
	}
	return b(e[1], e[3], (lo+hi)/2)
}

// Anim is a property with Slint's `animate`: when its target changes it goes there from
// where it is over the duration. The zero Anim starts at its first target.
type Anim struct {
	from, to float32
	start    time.Time
	set      bool
}

// Get is the value on the way to target; while it moves, the frame asks for another.
func (a *Anim) Get(c *Ctx, target float32, d time.Duration, e Easing) float32 {
	now := c.Now()
	if !a.set {
		a.from, a.to, a.start, a.set = target, target, now, true
	}
	if target != a.to {
		a.from, a.to, a.start = a.at(now, d, e), target, now
	}
	v := a.at(now, d, e)
	if v != a.to {
		c.Animating = true
	}
	return v
}

func (a *Anim) at(now time.Time, d time.Duration, e Easing) float32 {
	if d <= 0 {
		return a.to
	}
	k := float32(now.Sub(a.start)) / float32(d)
	if k >= 1 {
		return a.to
	}
	return a.from + (a.to-a.from)*ease(e, k)
}

// Dur is d with animations on, and none with them off (Pal.motion ? d : 0).
func (c *Ctx) Dur(d time.Duration) time.Duration {
	if c.Pal.Motion {
		return d
	}
	return 0
}

// AnimColor animates a colour channel by channel (Slint's `animate background`).
type AnimColor struct{ r, g, b, a Anim }

func (x *AnimColor) Get(c *Ctx, t color.NRGBA, d time.Duration, e Easing) color.NRGBA {
	ch := func(a *Anim, v uint8) uint8 { return uint8(clamp(a.Get(c, float32(v), d, e), 0, 255) + 0.5) }
	return color.NRGBA{ch(&x.r, t.R), ch(&x.g, t.G), ch(&x.b, t.B), ch(&x.a, t.A)}
}
