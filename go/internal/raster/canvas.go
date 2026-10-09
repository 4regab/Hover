// Package raster draws the chat into a pixel buffer, as Rust's hover-chat painter does
// with tiny-skia: rounded rectangles, strokes, glows, images, scrollbars, and text from
// glyph outlines (unhinted, like Chromium's DirectWrite path), all anti-aliased. The
// buffer is a plain image, so it can be shown by any toolkit and written to a file.
package raster

import (
	"image"
	"image/color"
	"image/draw"
	"math"

	"github.com/go-text/typesetting/font"
	xdraw "golang.org/x/image/draw"
	"golang.org/x/image/vector"

	"github.com/4regab/Hover/go/internal/text"
)

// Canvas is a premultiplied RGBA buffer and the caches drawing it needs.
type Canvas struct {
	Img    *image.RGBA
	ras    vector.Rasterizer
	glyphs map[glyphKey]*glyphMask
}

func NewCanvas(w, h int) *Canvas {
	return &Canvas{Img: image.NewRGBA(image.Rect(0, 0, max(w, 1), max(h, 1))), glyphs: map[glyphKey]*glyphMask{}}
}

// Resize makes a new buffer of the size, keeping the glyph cache.
func (c *Canvas) Resize(w, h int) {
	w, h = max(w, 1), max(h, 1)
	if c.Img.Bounds().Dx() != w || c.Img.Bounds().Dy() != h {
		c.Img = image.NewRGBA(image.Rect(0, 0, w, h))
	}
}

// Clear fills the whole buffer with one colour, replacing what is there.
func (c *Canvas) Clear(col color.NRGBA) {
	draw.Draw(c.Img, c.Img.Bounds(), image.NewUniform(col), image.Point{}, draw.Src)
}

// Bounds is the whole buffer, the clip when there is none.
func (c *Canvas) Bounds() image.Rectangle { return c.Img.Bounds() }

// fill composites col through the path build draws, within the box x0,y0..x1,y1 (px).
func (c *Canvas) fill(x0, y0, x1, y1 float32, col color.NRGBA, clip image.Rectangle, build func(r *vector.Rasterizer, ox, oy float32)) {
	if col.A == 0 || x1 <= x0 || y1 <= y0 {
		return
	}
	ir := image.Rect(int(math.Floor(float64(x0)))-1, int(math.Floor(float64(y0)))-1, int(math.Ceil(float64(x1)))+1, int(math.Ceil(float64(y1)))+1).
		Intersect(c.Img.Bounds()).Intersect(clip)
	if ir.Empty() {
		return
	}
	c.ras.Reset(ir.Dx(), ir.Dy())
	build(&c.ras, -float32(ir.Min.X), -float32(ir.Min.Y))
	c.ras.DrawOp = draw.Over
	c.ras.Draw(c.Img, ir, image.NewUniform(col), image.Point{})
}

// k is a quarter circle as a cubic: 1 - 0.5523.
const k = 0.447715

// rounded adds a rounded rectangle, clockwise (or the other way, to cut a hole).
func rounded(r *vector.Rasterizer, ox, oy, x, y, w, h float32, rad [4]float32, reverse bool) {
	x, y = x+ox, y+oy
	lim := min(w/2, h/2)
	var q [4]float32
	for i, v := range rad {
		q[i] = max(0, min(v, lim))
	}
	tl, tr, br, bl := q[0], q[1], q[2], q[3]
	if !reverse {
		r.MoveTo(x+tl, y)
		r.LineTo(x+w-tr, y)
		r.CubeTo(x+w-tr*k, y, x+w, y+tr*k, x+w, y+tr)
		r.LineTo(x+w, y+h-br)
		r.CubeTo(x+w, y+h-br*k, x+w-br*k, y+h, x+w-br, y+h)
		r.LineTo(x+bl, y+h)
		r.CubeTo(x+bl*k, y+h, x, y+h-bl*k, x, y+h-bl)
		r.LineTo(x, y+tl)
		r.CubeTo(x, y+tl*k, x+tl*k, y, x+tl, y)
	} else {
		r.MoveTo(x+tl, y)
		r.CubeTo(x+tl*k, y, x, y+tl*k, x, y+tl)
		r.LineTo(x, y+h-bl)
		r.CubeTo(x, y+h-bl*k, x+bl*k, y+h, x+bl, y+h)
		r.LineTo(x+w-br, y+h)
		r.CubeTo(x+w-br*k, y+h, x+w, y+h-br*k, x+w, y+h-br)
		r.LineTo(x+w, y+tr)
		r.CubeTo(x+w, y+tr*k, x+w-tr*k, y, x+w-tr, y)
	}
	r.ClosePath()
}

// FillRound fills a rounded rectangle; rad is the corners' radii: top left, top
// right, bottom right, bottom left.
func (c *Canvas) FillRound(x, y, w, h float32, rad [4]float32, col color.NRGBA, clip image.Rectangle) {
	if w <= 0 || h <= 0 {
		return
	}
	c.fill(x, y, x+w, y+h, col, clip, func(r *vector.Rasterizer, ox, oy float32) { rounded(r, ox, oy, x, y, w, h, rad, false) })
}

// Rect is FillRound with the same radius at every corner.
func (c *Canvas) Rect(x, y, w, h, rad float32, col color.NRGBA, clip image.Rectangle) {
	c.FillRound(x, y, w, h, [4]float32{rad, rad, rad, rad}, col, clip)
}

// StrokeRound draws the border of a rounded rectangle, sw wide and inside its edge.
func (c *Canvas) StrokeRound(x, y, w, h float32, rad [4]float32, sw float32, col color.NRGBA, clip image.Rectangle) {
	if w <= 2*sw || h <= 2*sw {
		c.FillRound(x, y, w, h, rad, col, clip)
		return
	}
	var in [4]float32
	for i, v := range rad {
		in[i] = max(v-sw, 0)
	}
	c.fill(x, y, x+w, y+h, col, clip, func(r *vector.Rasterizer, ox, oy float32) {
		rounded(r, ox, oy, x, y, w, h, rad, false)
		rounded(r, ox, oy, x+sw, y+sw, w-2*sw, h-2*sw, in, true)
	})
}

// Polyline strokes an open line of the given width with flat ends.
func (c *Canvas) Polyline(pts [][2]float32, width float32, col color.NRGBA, clip image.Rectangle) {
	if len(pts) < 2 {
		return
	}
	x0, y0, x1, y1 := pts[0][0], pts[0][1], pts[0][0], pts[0][1]
	for _, p := range pts {
		x0, y0, x1, y1 = min(x0, p[0]), min(y0, p[1]), max(x1, p[0]), max(y1, p[1])
	}
	h := width / 2
	c.fill(x0-h, y0-h, x1+h, y1+h, col, clip, func(r *vector.Rasterizer, ox, oy float32) {
		for i := 0; i+1 < len(pts); i++ {
			a, b := pts[i], pts[i+1]
			dx, dy := b[0]-a[0], b[1]-a[1]
			d := float32(math.Hypot(float64(dx), float64(dy)))
			if d == 0 {
				continue
			}
			nx, ny := -dy/d*h, dx/d*h
			r.MoveTo(a[0]+nx+ox, a[1]+ny+oy)
			r.LineTo(b[0]+nx+ox, b[1]+ny+oy)
			r.LineTo(b[0]-nx+ox, b[1]-ny+oy)
			r.LineTo(a[0]-nx+ox, a[1]-ny+oy)
			r.ClosePath()
		}
	})
}

// Glow is a round light that fades from col (at alpha 150, as the painter's) to nothing at r.
func (c *Canvas) Glow(cx, cy, r float32, col color.NRGBA, clip image.Rectangle) {
	ir := image.Rect(int(cx-r)-1, int(cy-r)-1, int(cx+r)+2, int(cy+r)+2).Intersect(c.Img.Bounds()).Intersect(clip)
	for y := ir.Min.Y; y < ir.Max.Y; y++ {
		for x := ir.Min.X; x < ir.Max.X; x++ {
			d := float32(math.Hypot(float64(float32(x)+0.5-cx), float64(float32(y)+0.5-cy))) / r
			if d >= 1 {
				continue
			}
			a := uint8(float32(150) * (1 - d))
			c.blend(x, y, color.NRGBA{col.R, col.G, col.B, a})
		}
	}
}

// blend puts col over one pixel.
func (c *Canvas) blend(x, y int, col color.NRGBA) {
	i := c.Img.PixOffset(x, y)
	p := c.Img.Pix[i : i+4 : i+4]
	a := uint32(col.A)
	ia := 255 - a
	p[0] = uint8((uint32(col.R)*a + uint32(p[0])*255*ia/255 + 127) / 255)
	p[1] = uint8((uint32(col.G)*a + uint32(p[1])*255*ia/255 + 127) / 255)
	p[2] = uint8((uint32(col.B)*a + uint32(p[2])*255*ia/255 + 127) / 255)
	p[3] = uint8(a + uint32(p[3])*ia/255)
}

// Image draws src scaled into the box, with rounded corners. cover fills the box and
// crops the picture to it (object-fit: cover); otherwise the picture is stretched to the box.
func (c *Canvas) Image(src image.Image, x, y, w, h, rad float32, cover bool, clip image.Rectangle) {
	sb := src.Bounds()
	if w < 1 || h < 1 || sb.Empty() {
		return
	}
	sr := sb
	if cover {
		s := max(w/float32(sb.Dx()), h/float32(sb.Dy()))
		cw, ch := int(math.Round(float64(w/s))), int(math.Round(float64(h/s)))
		cx, cy := sb.Min.X+(sb.Dx()-cw)/2, sb.Min.Y+(sb.Dy()-ch)/2
		sr = image.Rect(cx, cy, cx+max(cw, 1), cy+max(ch, 1)).Intersect(sb)
	}
	dr := image.Rect(int(math.Round(float64(x))), int(math.Round(float64(y))), int(math.Round(float64(x+w))), int(math.Round(float64(y+h))))
	vis := dr.Intersect(c.Img.Bounds()).Intersect(clip)
	if vis.Empty() {
		return
	}
	tmp := image.NewRGBA(image.Rect(0, 0, dr.Dx(), dr.Dy()))
	xdraw.CatmullRom.Scale(tmp, tmp.Bounds(), src, sr, draw.Src, nil)
	mask := image.NewAlpha(tmp.Bounds())
	var rr vector.Rasterizer
	rr.Reset(dr.Dx(), dr.Dy())
	rounded(&rr, 0, 0, 0, 0, float32(dr.Dx()), float32(dr.Dy()), [4]float32{rad, rad, rad, rad}, false)
	rr.Draw(mask, mask.Bounds(), image.Opaque, image.Point{})
	draw.DrawMask(c.Img, vis, tmp, vis.Min.Sub(dr.Min), mask, vis.Min.Sub(dr.Min), draw.Over)
}

// Blit puts a premultiplied picture at a pixel position, over what is there.
func (c *Canvas) Blit(src *image.RGBA, x, y int, clip image.Rectangle) {
	dr := image.Rectangle{Min: image.Pt(x, y), Max: image.Pt(x+src.Bounds().Dx(), y+src.Bounds().Dy())}.Intersect(clip)
	if dr.Empty() {
		return
	}
	draw.Draw(c.Img, dr, src, src.Bounds().Min.Add(dr.Min.Sub(image.Pt(x, y))), draw.Over)
}

// Layer is a transparent buffer the size of the canvas, drawn on and put down faded.
func (c *Canvas) Layer() *Canvas {
	return &Canvas{Img: image.NewRGBA(c.Img.Bounds()), glyphs: c.glyphs}
}

// Compose puts a layer down with an opacity, lowered by dy px (the answer's rise).
func (c *Canvas) Compose(l *Canvas, opacity, dy float32) {
	m := image.NewUniform(color.Alpha{A: uint8(math.Round(float64(opacity * 255)))})
	off := int(math.Round(float64(dy)))
	draw.DrawMask(c.Img, c.Img.Bounds().Add(image.Pt(0, off)), l.Img, l.Img.Bounds().Min, m, image.Point{}, draw.Over)
}

// glyphKey names a glyph mask: the face, the glyph, the size on screen and the quarter
// pixel it sits at. A variable font draws all its weights from one file, so the face
// (a copy per weight) is part of the key.
type glyphKey struct {
	face *font.Face
	gid  uint32
	size uint32
	sub  uint8
	skew bool
}

type glyphMask struct {
	left, top int
	m         *image.Alpha
}

// Shimmer colours a glyph by where it sits along its text box (the live step's band).
type Shimmer func(gx float32) color.NRGBA

// Layout draws a layout with its top left at x, y (CSS px) scaled by sc, within clip.
func (c *Canvas) Layout(l *text.Layout, x, y, sc float32, clip image.Rectangle, shim Shimmer) {
	for i := range l.Lines {
		ln := &l.Lines[i]
		base := y + ln.Baseline
		for r := range ln.Runs {
			run := &ln.Runs[r]
			if run.Box {
				continue
			}
			st := run.Style
			rx := x + run.X
			if st.Ink.Code {
				// .md code { background; padding: 1px 5px; border-radius: 5px }
				c.Rect((rx-5)*sc, (base-run.Ascent-1)*sc, (run.Adv+10)*sc, (run.Ascent+run.Descent+2)*sc, 5*sc, codeBG, clip)
			}
			col := st.Ink.Color
			for _, g := range run.Glyphs {
				gx := (rx + g.X) * sc
				gy := float32(math.Round(float64((base + g.Y) * sc)))
				if m := c.glyph(run, g.ID, sc, gx); m != nil {
					cc := col
					if shim != nil {
						cc = shim(g.X + run.X + g.Adv/2)
					}
					c.mask(m, int(math.Floor(float64(gx)))+m.left, int(gy)-m.top, cc, clip)
				}
			}
			if st.Underline {
				off := st.UnderlineOffset
				if off == 0 {
					off = -st.Size * 0.105
				}
				sz := max(st.Size*0.07, 1/sc)
				// text-underline-offset: 2px on links.
				c.Rect(rx*sc, (base-off+2)*sc, run.Adv*sc, sz*sc, 0, st.UnderlineInk, clip)
			}
			if st.Strike {
				c.Rect(rx*sc, (base-st.Size*0.3)*sc, run.Adv*sc, max(st.Size*0.07, 1/sc)*sc, 0, col, clip)
			}
		}
	}
}

var codeBG = color.NRGBA{255, 255, 255, 20}

// mask composites a coverage mask in one colour.
func (c *Canvas) mask(m *glyphMask, x, y int, col color.NRGBA, clip image.Rectangle) {
	dr := image.Rect(x, y, x+m.m.Bounds().Dx(), y+m.m.Bounds().Dy()).Intersect(c.Img.Bounds()).Intersect(clip)
	if dr.Empty() {
		return
	}
	draw.DrawMask(c.Img, dr, image.NewUniform(col), image.Point{}, m.m, dr.Min.Sub(image.Pt(x, y)), draw.Over)
}
