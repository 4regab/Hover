package raster

import (
	"image"
	"image/draw"
	"math"

	"github.com/go-text/typesetting/font"
	ot "github.com/go-text/typesetting/font/opentype"
	"golang.org/x/image/vector"

	"github.com/4regab/Hover/go/internal/text"
)

// tan14 is the slant of Chromium's synthetic oblique, for a font with no italic.
var tan14 = float32(math.Tan(14 * math.Pi / 180))

// glyph is the coverage mask of one glyph at a size on screen, for the quarter pixel its
// origin falls in, or nil when it has no outline (a space, or a bitmap-only font).
func (c *Canvas) glyph(run *text.GlyphRun, gid font.GID, sc, gx float32) *glyphMask {
	size := run.Size * sc
	sub := uint8(min(int((gx-float32(math.Floor(float64(gx))))*4), 3))
	key := glyphKey{run.Face, uint32(gid), math.Float32bits(size), sub, run.Skew}
	if m, ok := c.glyphs[key]; ok {
		return m
	}
	m := renderGlyph(run.Face, gid, size, float32(sub)/4, 0, run.Skew, &c.ras)
	c.glyphs[key] = m
	return m
}

func renderGlyph(face *font.Face, gid font.GID, size, subx, suby float32, skew bool, ras *vector.Rasterizer) *glyphMask {
	if face == nil {
		return nil
	}
	out, ok := face.GlyphDataOutline(gid)
	if !ok || len(out.Segments) == 0 {
		return nil
	}
	s := size / float32(face.Upem())
	at := func(p ot.SegmentPoint) (float32, float32) {
		x := p.X * s
		if skew {
			x += p.Y * s * tan14
		}
		return x + subx, -p.Y*s + suby
	}
	x0, y0, x1, y1 := float32(math.MaxFloat32), float32(math.MaxFloat32), float32(-math.MaxFloat32), float32(-math.MaxFloat32)
	for i := range out.Segments {
		sg := &out.Segments[i]
		for _, p := range sg.ArgsSlice() {
			x, y := at(p)
			x0, y0, x1, y1 = min(x0, x), min(y0, y), max(x1, x), max(y1, y)
		}
	}
	left, top := int(math.Floor(float64(x0)))-1, int(math.Floor(float64(y0)))-1
	w, h := int(math.Ceil(float64(x1)))+1-left, int(math.Ceil(float64(y1)))+1-top
	if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
		return nil
	}
	ras.Reset(w, h)
	open := false
	pt := func(p ot.SegmentPoint) (float32, float32) {
		x, y := at(p)
		return x - float32(left), y - float32(top)
	}
	for i := range out.Segments {
		sg := &out.Segments[i]
		a := sg.Args
		switch sg.Op {
		case ot.SegmentOpMoveTo:
			if open {
				ras.ClosePath()
			}
			x, y := pt(a[0])
			ras.MoveTo(x, y)
			open = true
		case ot.SegmentOpLineTo:
			x, y := pt(a[0])
			ras.LineTo(x, y)
		case ot.SegmentOpQuadTo:
			bx, by := pt(a[0])
			cx, cy := pt(a[1])
			ras.QuadTo(bx, by, cx, cy)
		case ot.SegmentOpCubeTo:
			bx, by := pt(a[0])
			cx, cy := pt(a[1])
			dx, dy := pt(a[2])
			ras.CubeTo(bx, by, cx, cy, dx, dy)
		}
	}
	if open {
		ras.ClosePath()
	}
	m := image.NewAlpha(image.Rect(0, 0, w, h))
	ras.DrawOp = draw.Src
	ras.Draw(m, m.Bounds(), image.Opaque, image.Point{})
	return &glyphMask{left: left, top: -top, m: m}
}

// GlyphMask is the coverage of one glyph at size px with its origin (dx, dy) px into a
// pixel (both 0..1). The mask's top left pixel is at (left, top) from the origin's pixel;
// nil when the glyph has no outline.
func GlyphMask(face *font.Face, gid font.GID, size, dx, dy float32) (m *image.Alpha, left, top int) {
	var ras vector.Rasterizer
	g := renderGlyph(face, gid, size, dx, dy, false, &ras)
	if g == nil {
		return nil, 0, 0
	}
	return g.m, g.left, -g.top
}
