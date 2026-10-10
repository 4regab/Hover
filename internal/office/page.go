package office

import (
	"math"
	"strconv"
	"strings"
)

// CSS is a CSS colour as the canvas takes it: #rgb, #rrggbb or rgba(r,g,b,a), as 0..1 straight RGBA.
func CSS(s string) [4]float64 {
	if h, ok := strings.CutPrefix(s, "#"); ok {
		if len(h) == 3 {
			h = string([]byte{h[0], h[0], h[1], h[1], h[2], h[2]})
		}
		v, _ := strconv.ParseUint(h, 16, 32)
		return [4]float64{float64((v>>16)&255) / 255, float64((v>>8)&255) / 255, float64(v&255) / 255, 1}
	}
	inner := strings.TrimRight(strings.TrimPrefix(strings.TrimPrefix(s, "rgba("), "rgb("), ")")
	var p []float64
	for _, x := range strings.Split(inner, ",") {
		f, _ := strconv.ParseFloat(strings.TrimSpace(x), 64)
		p = append(p, f)
	}
	a := 1.0
	if len(p) > 3 {
		a = p[3]
	}
	return [4]float64{p[0] / 255, p[1] / 255, p[2] / 255, a}
}

// What page.html lays around and over the canvas in host mode: #office's radial background
// (night or day) and the ::after vignette. Since 639c01c the page drops #office's rounded
// corners and border in Hover (body.host #office), so the office fills its box edge to
// edge. CSS gradients are interpolated premultiplied in sRGB; ellipse radii are
// percentages of the box.

type stop struct {
	at float64
	c  [4]float64
}

func ramp(stops []stop, t float64) [4]float64 {
	i := 0
	for i+1 < len(stops) && stops[i+1].at < t {
		i++
	}
	a, b := stops[i], stops[min(i+1, len(stops)-1)]
	k := 0.0
	if b.at > a.at {
		k = math.Min(math.Max((t-a.at)/(b.at-a.at), 0), 1)
	}
	pa := [4]float64{a.c[0] * a.c[3], a.c[1] * a.c[3], a.c[2] * a.c[3], a.c[3]}
	pb := [4]float64{b.c[0] * b.c[3], b.c[1] * b.c[3], b.c[2] * b.c[3], b.c[3]}
	var o [4]float64
	for j := range o {
		o[j] = pa[j] + (pb[j]-pa[j])*k
	}
	return o
}

// radial is radial-gradient(rx% ry% at cx% cy%, stops): premultiplied RGBA at a pixel centre.
func radial(x, y, w, h float64, r, at [2]float64, stops []stop) [4]float64 {
	dx, dy := (x-at[0]*w)/(r[0]*w), (y-at[1]*h)/(r[1]*h)
	return ramp(stops, math.Hypot(dx, dy))
}

func bgStops(day bool) []stop {
	if day {
		return []stop{{0, CSS("#4a3530")}, {0.6, CSS("#241815")}, {1, CSS("#0e0a09")}}
	}
	return []stop{{0, CSS("#2a1824")}, {0.55, CSS("#150c14")}, {1, CSS("#07050a")}}
}

var vignette = []stop{{0, [4]float64{0, 0, 0, 0}}, {0.6, [4]float64{0, 0, 0, 0}}, {1, [4]float64{0, 0, 0, 0.45}}}

// Compose is the office as the page shows it: the frame (premultiplied RGBA8 from the
// renderer) over the background, and the vignette over both. RGB8.
func Compose(frame []byte, w, h int, day bool) []byte {
	bg := bgStops(day)
	fw, fh := float64(w), float64(h)
	out := make([]byte, 0, w*h*3)
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			px, py := float64(x)+0.5, float64(y)+0.5
			b := radial(px, py, fw, fh, [2]float64{1.2, 0.9}, [2]float64{0.5, 0.45}, bg)
			f := frame[(y*w+x)*4 : (y*w+x)*4+4]
			fa := float64(f[3]) / 255
			var c [3]float64
			for k := range c {
				c[k] = float64(f[k])/255 + b[k]*(1-fa)
			}
			v := radial(px, py, fw, fh, [2]float64{1.3, 1}, [2]float64{0.5, 0.5}, vignette)
			for k := range c {
				c[k] = v[k] + c[k]*(1-v[3])
				out = append(out, uint8(math.Min(math.Max(math.Round(c[k]*255), 0), 255)))
			}
		}
	}
	return out
}

// Composer is Compose with what doesn't change from frame to frame (the background, the
// vignette) worked out once per size and time of day. Kept as bytes, not floats: every
// value is a whole byte (or one from a byte difference), so the result is the same, at 7
// bytes a pixel instead of 28 (10 MB less at the default office size).
type Composer struct {
	w, h  int
	day   bool
	under [][3]uint8
	over  [][4]uint8
}

func (c *Composer) Compose(frame []byte, w, h int, day bool) []byte {
	return c.ComposeInto(frame, w, h, day, nil)
}

// ComposeInto is Compose into a buffer the caller keeps between frames.
func (c *Composer) ComposeInto(frame []byte, w, h int, day bool, out []byte) []byte {
	if c.w != w || c.h != h || c.day != day || len(c.under) == 0 {
		c.w, c.h, c.day = w, h, day
		// Two layers from Compose itself: the page with a clear frame (the background), and
		// with a white opaque one (what lies over the frame).
		clear := Compose(make([]byte, w*h*4), w, h, day)
		c.under = make([][3]uint8, w*h)
		for i := range c.under {
			c.under[i] = [3]uint8{clear[i*3], clear[i*3+1], clear[i*3+2]}
		}
		white := make([]byte, w*h*4)
		black := make([]byte, w*h*4)
		for i := range white {
			white[i] = 255
		}
		for i := 3; i < len(black); i += 4 {
			black[i] = 255
		}
		wh, bl := Compose(white, w, h, day), Compose(black, w, h, day)
		// Over the frame everything is linear in it: out = frame * k + c; k from white − black-opaque.
		c.over = make([][4]uint8, w*h)
		for i := range c.over {
			var d uint8
			if wh[i*3] > bl[i*3] {
				d = wh[i*3] - bl[i*3]
			}
			c.over[i] = [4]uint8{d, bl[i*3], bl[i*3+1], bl[i*3+2]}
		}
	}
	out = out[:0]
	for i := 0; i < len(frame)/4; i++ {
		p := frame[i*4 : i*4+4]
		a := float32(p[3]) / 255
		u, o := c.under[i], c.over[i]
		k := float32(o[0]) / 255
		for ch := 0; ch < 3; ch++ {
			// The frame over its background, then what lies over both; the part of the
			// background the frame lets through is under's, which already carries it. Each
			// product is its own float32 so no machine fuses them differently.
			t1 := float32(float32(p[ch]) * k)
			t2 := float32(float32(o[ch+1]) * a)
			t3 := float32(float32(u[ch]) * float32(1-a))
			v := float32(float32(t1+t2) + t3)
			out = append(out, uint8(math.Min(math.Max(math.Round(float64(v)), 0), 255)))
		}
	}
	return out
}
