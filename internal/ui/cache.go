package ui

import (
	"image"
	"image/color"
	"math"
	"sync"

	"gioui.org/f32"
	"gioui.org/op"
	"gioui.org/op/clip"
	"gioui.org/op/paint"

	"github.com/4regab/Hover/internal/raster"
)

// What Gio can't draw (blur, gradients of more than two stops, the marks' fills) is drawn
// on the CPU at the size it shows at, once, and kept as an image Gio uploads once.
//
// ponytail: the cache forgets everything when it passes 512 images. A UI that showed
// more distinct shadows than that in one frame would draw them all again every frame;
// an LRU is the upgrade.
var images struct {
	sync.Mutex
	m map[any]paint.ImageOp
}

func cachedImage(key any, draw func() image.Image) paint.ImageOp {
	images.Lock()
	defer images.Unlock()
	if im, ok := images.m[key]; ok {
		return im
	}
	if images.m == nil || len(images.m) >= 512 {
		images.m = map[any]paint.ImageOp{}
	}
	im := paint.NewImageOp(draw())
	images.m[key] = im
	return im
}

// image draws im with its top left at physical (px, py) of the current origin, at its
// own size.
func (c *Ctx) image(im paint.ImageOp, px, py float32, opacity float32) {
	t := op.Affine(f32.Affine2D{}.Offset(f32.Pt(px, py))).Push(c.Ops)
	if opacity < 1 {
		o := paint.PushOpacity(c.Ops, opacity)
		defer o.Pop()
	}
	sz := im.Size()
	cl := clip.Rect(image.Rect(0, 0, sz.X, sz.Y)).Push(c.Ops)
	im.Add(c.Ops)
	paint.PaintOp{}.Add(c.Ops)
	cl.Pop()
	t.Pop()
}

// q is a physical length rounded to a quarter pixel, for cache keys.
func q(v float32) int32 { return int32(math.Round(float64(v * 4))) }

type shadowKey struct {
	w, h, blur int32
	r          [4]int32
	col        color.NRGBA
}

// Shadow is a Slint drop shadow: the rounded rectangle (x, y, w, h) moved by (ox, oy) and
// blurred by blur (a Gaussian of sigma blur/2, as CSS's box-shadow), in col.
func (c *Ctx) Shadow(x, y, w, h float32, r Radii, blur, ox, oy float32, col color.NRGBA) {
	if col.A == 0 || w <= 0 || h <= 0 {
		return
	}
	k := c.K
	sigma := blur * k / 2
	pad := float32(math.Ceil(float64(3*sigma))) + 1
	pw, ph := w*k, h*k
	key := shadowKey{q(pw), q(ph), q(sigma), [4]int32{q(r[0] * k), q(r[1] * k), q(r[2] * k), q(r[3] * k)}, col}
	im := cachedImage(key, func() image.Image {
		iw, ih := int(math.Ceil(float64(pw+2*pad))), int(math.Ceil(float64(ph+2*pad)))
		cv := raster.NewCanvas(iw, ih)
		cv.FillRound(pad, pad, pw, ph, [4]float32{r[0] * k, r[1] * k, r[2] * k, r[3] * k}, color.NRGBA{255, 255, 255, 255}, cv.Bounds())
		a := make([]float32, iw*ih)
		for i := range a {
			a[i] = float32(cv.Img.Pix[i*4+3]) / 255
		}
		blurAlpha(a, iw, ih, sigma)
		out := image.NewNRGBA(image.Rect(0, 0, iw, ih))
		for i, v := range a {
			out.Pix[i*4], out.Pix[i*4+1], out.Pix[i*4+2] = col.R, col.G, col.B
			out.Pix[i*4+3] = uint8(clamp(v*float32(col.A), 0, 255) + 0.5)
		}
		return out
	})
	c.image(im, (x+ox)*k-pad, (y+oy)*k-pad, 1)
}

// blurAlpha blurs a w x h coverage map by a Gaussian of sigma, rows then columns.
func blurAlpha(a []float32, w, h int, sigma float32) {
	if sigma < 0.1 {
		return
	}
	n := int(math.Ceil(float64(3 * sigma)))
	kern := make([]float32, 2*n+1)
	var sum float32
	for i := range kern {
		d := float64(i - n)
		kern[i] = float32(math.Exp(-d * d / (2 * float64(sigma) * float64(sigma))))
		sum += kern[i]
	}
	for i := range kern {
		kern[i] /= sum
	}
	tmp := make([]float32, max(w, h))
	pass := func(get func(i int) float32, set func(i int, v float32), m int) {
		for i := 0; i < m; i++ {
			var s float32
			for j, kv := range kern {
				if p := i + j - n; p >= 0 && p < m {
					s += get(p) * kv
				}
			}
			tmp[i] = s
		}
		for i := 0; i < m; i++ {
			set(i, tmp[i])
		}
	}
	for y := 0; y < h; y++ {
		row := a[y*w : (y+1)*w]
		pass(func(i int) float32 { return row[i] }, func(i int, v float32) { row[i] = v }, w)
	}
	for x := 0; x < w; x++ {
		pass(func(i int) float32 { return a[i*w+x] }, func(i int, v float32) { a[i*w+x] = v }, h)
	}
}

type radialKey struct {
	d   int32
	col color.NRGBA
}

// RadialGlow is a circle d across at (x, y) filled with @radial-gradient(circle, col 0%,
// col at alpha 0 100%), times opacity.
func (c *Ctx) RadialGlow(x, y, d float32, col color.NRGBA, opacity float32) {
	pd := d * c.K
	if pd < 1 || opacity <= 0 {
		return
	}
	im := cachedImage(radialKey{q(pd), col}, func() image.Image {
		n := int(math.Ceil(float64(pd)))
		out := image.NewNRGBA(image.Rect(0, 0, n, n))
		rr := pd / 2
		for py := 0; py < n; py++ {
			for px := 0; px < n; px++ {
				dist := float32(math.Hypot(float64(float32(px)+0.5-rr), float64(float32(py)+0.5-rr)))
				// Slint's default radius is the farthest corner's (half the diagonal);
				// the circle (border-radius) cuts it off at its edge, with a pixel of AA.
				t := dist / (rr * math.Sqrt2)
				edge := clamp(rr-dist+0.5, 0, 1)
				i := (py*n + px) * 4
				out.Pix[i], out.Pix[i+1], out.Pix[i+2] = col.R, col.G, col.B
				out.Pix[i+3] = uint8(clamp(1-t, 0, 1)*edge*float32(col.A) + 0.5)
			}
		}
		return out
	})
	c.image(im, x*c.K, y*c.K, opacity)
}
