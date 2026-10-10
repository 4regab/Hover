package ui

import (
	"image/color"
	"math"
)

// bot.slint (Owl/Bot.cs): the Kiro bot drawn flat (BotGlyph) and the three rising dots
// (WorkDots), from the same numbers. t and since are seconds, stepped by the app's 30 fps
// clock only while one of them is on screen, and left at 0 with animations off.

// Bot is a BotGlyph's properties. Since is 9 unless set; Body and Dark are the Kiro
// purple and its shade unless set.
type Bot struct {
	Live, Finished bool
	T, Since       float32
	Motion         bool
	Body, Dark     color.NRGBA
}

func NewBot() Bot {
	return Bot{Since: 9, Motion: true, Body: RGB(0x9b6bff), Dark: RGB(0x553a8c)}
}

func sinf(v float32) float32 { return float32(math.Sin(float64(v))) }

func modf(a, b float32) float32 { return float32(math.Mod(float64(a), float64(b))) }

// BotGlyph draws the bot in the box (x, y, w, h).
func (c *Ctx) BotGlyph(b Bot, x, y, w, h float32) {
	still := !b.Motion || !(b.Live || b.Finished)
	tt := If[float32](still, 0, b.T)
	s := min(w, h/0.9)
	k := b.Since / 0.9
	hop := b.Finished && !still && b.Since < 0.9
	var dy float32
	switch {
	case hop:
		dy = -sinf(k*math.Pi) * s * 0.18
	case b.Live:
		dy = sinf(tt*5.2) * s * 0.035
	}
	var squash float32
	if hop && k > 0.8 {
		squash = sinf((k-0.8)/0.2*math.Pi) * 0.08
	}
	x0 := (w - s) / 2
	top := h - s*0.78 + dy + s*squash*0.78
	headH := s * 0.78 * (1 - squash)
	pulse := float32(1)
	if b.Live && !still {
		pulse = 0.5 + 0.5*sinf(tt*6)
	}
	bulb := If(b.Finished, RGB(0x4ade80), RGB(0xffd24a))
	cyc := modf(tt, 4.2)
	working := b.Live && !still
	var look, down float32
	if working {
		switch {
		case cyc < 2.4:
			look = -0.05 + 0.1*(cyc/2.4)
		case cyc < 3.6:
			look, down = 0.02, 0.05
		default:
			look = 0.05 - 0.07*((cyc-3.6)/0.6)
		}
	}
	blink := float32(1)
	if working && modf(tt, 3.3) < 0.12 {
		blink = 0.15
	}
	vx, vy, vh := x0+s*0.18, top+headH*0.26, headH*0.51

	at := c.At(x, y)
	defer at.Pop()
	// The antenna's bulb pulsing in a soft halo.
	if b.Live || b.Finished {
		c.RadialGlow(x0+s*0.67-s*0.26, top-s*0.13-s*0.26, s*0.52, bulb, (90+110*pulse)/255)
	}
	c.Box(x0+s*0.64, top-s*0.1, s*0.06, s*0.12, R(0), b.Dark)
	bulbC := bulb
	if b.Live {
		bulbC = Mix(bulb, RGB(0x9b6bff), 1-0.25*(1-pulse))
	}
	c.Box(x0+s*0.6, top-s*0.2, s*0.14, s*0.14, R(s*0.03), bulbC)
	// Headphone pads, head and visor.
	c.Box(x0, top+headH*0.28, s, headH*0.44, R(s*0.05), b.Dark)
	c.Box(x0+s*0.07, top, s*0.86, headH, R(s*0.18), b.Body)
	c.Box(vx, vy, s*0.64, vh, R(s*0.1), RGB(0x121018))
	// Eyes: pixels while it works, ^ ^ once it has finished.
	for _, ex := range [2]float32{0.34, 0.58} {
		if !b.Finished {
			eh := vh * 0.5 * blink
			c.Box(x0+s*(ex+look), vy+vh*0.25+(vh*0.5-eh)/2+s*down, s*0.09, max(0.6, eh), R(s*0.02), RGB(0xaaf6ff))
			continue
		}
		cx, cy := x0+s*(ex+0.045), vy+vh*0.55
		d := pathf("M %g %g L %g %g L %g %g", cx-s*0.06, cy+s*0.04, cx, cy-s*0.03, cx+s*0.06, cy+s*0.04)
		c.strokeOnce(d, s*0.07, RGB(0xaaf6ff))
	}
}

// WorkDots are the three rising dots, 14 x 12.
func (c *Ctx) WorkDots(t float32, motion bool, x, y float32) {
	for i := 0; i < 3; i++ {
		var k float32
		if motion {
			k = max(0, sinf(t*6-float32(i)*0.9))
		}
		c.opacity(0.45+0.55*k, func() {
			c.Box(x+2+float32(i)*5-1.5, y+12.0/2+1.5-k*2.5-1.5, 3, 3, R(1.5), RGB(0xc4a2ff))
		})
	}
}
