package ui

import (
	"image"
	"image/color"

	"gioui.org/op/clip"
	"gioui.org/op/paint"
)

// office.slint's Glass and Graphite, and the blur behind the glass.

// Blur is backdrop-filter: blur(18px) saturate(1.4), at a quarter of the size (a blur that
// wide loses nothing at that scale): three box passes each way make it near Gaussian
// (sigma 18 px is a box of about 17 px at full size, 4 at a quarter). Its buffers are kept
// between frames.
type Blur struct {
	small, line [][3]float32
	Out         []byte
	W, H        int
}

// Of blurs an RGB picture w x h into Out (RGBA, W x H).
func (b *Blur) Of(rgb []byte, w, h int) {
	sw, sh := (w+3)/4, (h+3)/4
	if cap(b.small) < sw*sh {
		b.small = make([][3]float32, sw*sh)
	}
	b.small = b.small[:sw*sh]
	for y := 0; y < sh; y++ {
		for x := 0; x < sw; x++ {
			var acc [3]float32
			for dy := 0; dy < 4; dy++ {
				for dx := 0; dx < 4; dx++ {
					px, py := min(x*4+dx, w-1), min(y*4+dy, h-1)
					i := (py*w + px) * 3
					for k := 0; k < 3; k++ {
						acc[k] += float32(rgb[i+k])
					}
				}
			}
			b.small[y*sw+x] = [3]float32{acc[0] / 16, acc[1] / 16, acc[2] / 16}
		}
	}
	const r = 2
	for pass := 0; pass < 3; pass++ {
		for _, horizontal := range [2]bool{true, false} {
			lines, n := sh, sw
			if !horizontal {
				lines, n = sw, sh
			}
			at := func(line, i int) int {
				if horizontal {
					return line*sw + i
				}
				return i*sw + line
			}
			for line := 0; line < lines; line++ {
				b.line = b.line[:0]
				for i := 0; i < n; i++ {
					b.line = append(b.line, b.small[at(line, i)])
				}
				for i := 0; i < n; i++ {
					var acc [3]float32
					for d := -r; d <= r; d++ {
						s := b.line[max(0, min(n-1, i+d))]
						for k := 0; k < 3; k++ {
							acc[k] += s[k]
						}
					}
					b.small[at(line, i)] = [3]float32{acc[0] / (2*r + 1), acc[1] / (2*r + 1), acc[2] / (2*r + 1)}
				}
			}
		}
	}
	b.Out = b.Out[:0]
	for _, s := range b.small {
		// saturate(1.4), with the filter's luminance weights.
		l := 0.2126*s[0] + 0.7152*s[1] + 0.0722*s[2]
		for k := 0; k < 3; k++ {
			b.Out = append(b.Out, uint8(clamp(l+(s[k]-l)*1.4, 0, 255)))
		}
		b.Out = append(b.Out, 255)
	}
	b.W, b.H = sw, sh
}

// Backdrop is the blurred picture of the scene and where the scene is, in the coordinates
// the glass is drawn in (Backdrop global). Nil Img: glass is a solid panel.
type Backdrop struct {
	Img        paint.ImageOp
	Have       bool
	W, H       int
	OX, OY     float32
	SceneW     float32
	SceneH     float32
	SolidPanel bool
}

// Set makes the picture from a Blur.
func (b *Backdrop) Set(bl *Blur) {
	im := image.NewRGBA(image.Rect(0, 0, bl.W, bl.H))
	copy(im.Pix, bl.Out)
	b.Img, b.Have, b.W, b.H = paint.NewImageOp(im), true, bl.W, bl.H
}

// Glass draws a .glass panel: rgba(22,15,30,.72) over the scene blurred (a solid panel
// over the chat view, where there is no office to blur), a 1 px line unless edge is off,
// and its shadow.
func (c *Ctx) Glass(bd *Backdrop, x, y, w, h, radius float32, edge bool) {
	if w <= 0 || h <= 0 {
		return
	}
	c.Shadow(x, y, w, h, R(radius), 36, 0, 12, RGBA(0x00000066))
	cl := c.RRect(x, y, w, h, R(radius)).Push(c.Ops)
	if bd != nil && bd.Have && !bd.SolidPanel {
		// The blurred picture, scaled over the scene's place.
		sx, sy := bd.SceneW*c.K/float32(bd.W), bd.SceneH*c.K/float32(bd.H)
		bd.Img.Filter = paint.FilterLinear
		c.imageScaled(bd.Img, bd.OX*c.K, bd.OY*c.K, sx, sy)
	}
	fill := RGBA(0x160f1eb8)
	if bd != nil && bd.SolidPanel {
		fill = RGBA(0x1c1a24f7)
	}
	paint.FillShape(c.Ops, fill, clip.Rect(c.irect(x, y, w, h)).Op())
	cl.Pop()
	if edge {
		c.Border(x, y, w, h, R(radius), 1, RGBA(0xffffff17))
	}
}

// Graphite is the chat's and the side panel's panel: a neutral graphite, not glass.
func (c *Ctx) Graphite(x, y, w, h float32) {
	c.Shadow(x, y, w, h, R(20), 60, 0, 30, RGBA(0x000000b3))
	c.Gradient(x, y, w, h, R(20), 180, RGBA(0x131116f7), RGBA(0x0d0c0ffa))
	c.Border(x, y, w, h, R(20), 1, RGBA(0xffffff14))
}

var _ color.NRGBA
